//! Configuration parsing, compatibility and writing (directive §18, §53).

use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

use super::*;
use crate::backend::OutputId;
use crate::config::load::load_with;
use crate::types::{FpsLimit, HwDecode, MediaType, ScalingMode};

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lucerna-cfg-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn parse(text: &str) -> (Config, Vec<String>) {
    let doc: DocumentMut = text.parse().unwrap();
    let mut warnings = Vec::new();
    let config = parse_doc(&doc, &mut warnings);
    (config, warnings)
}

const FULL: &str = r#"
schema_version = 1

[general]
pause_on_fullscreen = false
pause_on_lock = false
audio = true
hardware_decode = "disabled"
fps_limit = "30"

[renderer]
max_restarts = 5
restart_window_secs = 120

[x11]
stacking = "desktop-window"

[all_displays]
wallpaper = "w-1"
scaling = "fit"

[displays."edid:DEL-a0b1-7XJ2K3"]
wallpaper = "w-2"
scaling = "center"
last_seen = "DP-1 — 2560×1440"

[[wallpapers]]
id = "w-1"
name = "Rain"
path = "/home/user/Videos/rain.webm"
media_type = "video"
added = 2026-09-30T10:00:00Z
available = false

[[wallpapers]]
id = "w-2"
name = "Loop"
path = "/home/user/Videos/loop.gif"
media_type = "animated-image"
added = 2026-09-30T11:00:00Z
available = true
"#;

#[test]
fn parses_a_complete_file() {
    let (c, warnings) = parse(FULL);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!c.general.pause_on_fullscreen && !c.general.pause_on_lock && c.general.audio);
    assert_eq!(c.general.hardware_decode, HwDecode::Disabled);
    assert_eq!(c.general.fps_limit, FpsLimit::Fps30);
    assert_eq!(
        c.renderer,
        RendererSettings {
            max_restarts: 5,
            restart_window_secs: 120
        }
    );
    assert_eq!(c.x11.stacking, StackingSetting::DesktopWindow);
    assert_eq!(
        c.all_displays.wallpaper.as_ref().map(WallpaperId::as_str),
        Some("w-1")
    );
    assert_eq!(c.all_displays.scaling, ScalingMode::Fit);
    let d = &c.displays[&OutputId::new("edid:DEL-a0b1-7XJ2K3")];
    assert_eq!(d.scaling, Some(ScalingMode::Center));
    assert_eq!(d.last_seen.as_deref(), Some("DP-1 — 2560×1440"));
    assert_eq!(c.wallpapers.len(), 2);
    assert_eq!(c.wallpapers[0].added, "2026-09-30T10:00:00Z");
    assert!(!c.wallpapers[0].available);
    assert_eq!(c.wallpapers[1].media_type, MediaType::AnimatedImage);
}

#[test]
fn an_empty_document_gives_the_defaults_without_warnings() {
    let (c, warnings) = parse("");
    assert_eq!(c, Config::default());
    assert!(warnings.is_empty());
}

#[test]
fn unknown_keys_and_tables_are_ignored() {
    let (c, warnings) =
        parse("schema_version = 1\nfuture = 1\n[general]\nsparkles = true\n[brand_new]\nx = 1\n");
    assert_eq!(c, Config::default());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn unknown_enum_values_and_wrong_types_fall_back_with_warnings() {
    let (c, warnings) = parse(
        "[general]\nfps_limit = \"144\"\nhardware_decode = 7\npause_on_fullscreen = \"yes\"\naudio = 1\n\
         [renderer]\nmax_restarts = \"many\"\n[all_displays]\nscaling = \"zoom\"\nwallpaper = 5\n",
    );
    assert_eq!(c, Config::default());
    assert_eq!(warnings.len(), 7, "{warnings:#?}");
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("fps_limit") && w.contains("144"))
    );
}

#[test]
fn fps_limit_accepts_an_integer() {
    let (c, warnings) = parse("[general]\nfps_limit = 60\n");
    assert_eq!(c.general.fps_limit, FpsLimit::Fps60);
    assert!(warnings.is_empty());
}

#[test]
fn invalid_wallpaper_entries_are_skipped_with_a_warning() {
    let (c, warnings) = parse(
        "[[wallpapers]]\nname = \"no id\"\npath = \"/a.mp4\"\n\
         [[wallpapers]]\nid = \"rel\"\npath = \"relative.mp4\"\n\
         [[wallpapers]]\nid = \"ok\"\npath = \"/ok.mp4\"\nadded = 2026-01-01T00:00:00Z\n",
    );
    assert_eq!(c.wallpapers.len(), 1);
    assert_eq!(c.wallpapers[0].id.as_str(), "ok");
    assert_eq!(
        c.wallpapers[0].name, "ok",
        "missing name falls back to the file stem"
    );
    assert_eq!(warnings.iter().filter(|w| w.contains("invalid")).count(), 2);
}

// ------------------------------------------------------------------------------- load outcomes

#[test]
fn missing_file_yields_defaults_and_first_run() {
    let dir = tempdir("missing");
    let loaded = load(&dir.join("config.toml"), "T");
    assert_eq!(loaded.config, Config::default());
    assert!(loaded.first_run);
    assert_eq!(loaded.state, ConfigState::Ok);
    assert!(
        !dir.join("config.toml").exists(),
        "loading never creates the file"
    );
}

#[test]
fn missing_schema_version_is_treated_as_1_with_a_warning() {
    let dir = tempdir("noschema");
    fs::write(dir.join("config.toml"), "[general]\naudio = true\n").unwrap();
    let loaded = load(&dir.join("config.toml"), "T");
    assert!(loaded.config.general.audio);
    assert_eq!(loaded.state, ConfigState::Ok);
    assert!(loaded.warnings.iter().any(|w| w.contains("schema_version")));
}

#[test]
fn a_newer_schema_is_read_leniently_and_never_written() {
    let dir = tempdir("future");
    let path = dir.join("config.toml");
    let original = "schema_version = 7\n[general]\naudio = true\nfps_limit = \"144\"\n";
    fs::write(&path, original).unwrap();
    let mut loaded = load(&path, "T");
    assert_eq!(
        loaded.state,
        ConfigState::ReadOnlyNewerSchema { version: 7 }
    );
    assert!(loaded.state.is_read_only());
    assert!(loaded.config.general.audio, "known fields are still used");
    assert_eq!(loaded.config.general.fps_limit, FpsLimit::Native);

    loaded.config.general.audio = false;
    let err = save(&mut loaded, &path).unwrap_err();
    assert!(err.to_string().contains("newer Lucerna (schema 7)"));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original,
        "the file is untouched"
    );
}

#[test]
fn a_corrupt_file_is_moved_aside_and_defaults_are_used() {
    let dir = tempdir("corrupt");
    let path = dir.join("config.toml");
    fs::write(&path, "this is [not valid toml").unwrap();
    let loaded = load(&path, "20260930T100000Z");
    let backup = dir.join("config.toml.corrupt-20260930T100000Z");
    assert_eq!(
        loaded.state,
        ConfigState::DefaultsAfterCorruption {
            backup: backup.clone()
        }
    );
    assert_eq!(loaded.config, Config::default());
    assert!(!path.exists());
    assert_eq!(
        fs::read_to_string(&backup).unwrap(),
        "this is [not valid toml",
        "kept, never deleted"
    );
    let notice = loaded.notice.unwrap();
    assert!(notice.contains("could not be parsed") && notice.contains("config.toml.corrupt-"));

    // The user can carry on: saving writes a fresh, valid file and the backup stays.
    let mut loaded = load(&path, "later");
    assert!(loaded.first_run);
    loaded.config.general.audio = true;
    save(&mut loaded, &path).unwrap();
    assert!(backup.exists());
    assert!(load(&path, "x").config.general.audio);
}

#[test]
fn a_file_that_cannot_be_read_is_never_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir("unreadable");
    let path = dir.join("config.toml");
    fs::write(&path, "[general]\naudio = true\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_to_string(&path).is_ok() {
        return; // running as root: permissions do not apply
    }
    let mut loaded = load(&path, "T");
    assert!(matches!(loaded.state, ConfigState::Unreadable { .. }));
    assert!(save(&mut loaded, &path).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "[general]\naudio = true\n"
    );
}

#[test]
fn an_older_schema_is_backed_up_migrated_and_written_atomically() {
    fn v0_to_v1(doc: &mut DocumentMut) -> Result<(), MigrationError> {
        let mute = doc["general"]["mute"].as_bool().unwrap_or(true);
        doc["general"]["audio"] = toml_edit::value(!mute);
        doc["general"].as_table_mut().map(|t| t.remove("mute"));
        Ok(())
    }
    let dir = tempdir("migrate");
    let path = dir.join("config.toml");
    let old = "schema_version = 0\n# my note\n[general]\nmute = false\n";
    fs::write(&path, old).unwrap();

    // The shipped v1 table is empty, so use an injected v0 -> v1 migration as the plan describes.
    // (`load_with` starts from the file's version and stamps the current one.)
    let loaded = load_with(&path, "S", &[v0_to_v1]);
    assert_eq!(loaded.state, ConfigState::Ok);
    assert!(loaded.config.general.audio, "the migrated value is in use");
    assert_eq!(
        fs::read_to_string(dir.join("config.toml.bak-v0-S")).unwrap(),
        old,
        "backup made first"
    );
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("schema_version = 1") && written.contains("# my note"));
    assert!(
        loaded
            .notice
            .unwrap()
            .contains("upgraded from schema 0 to 1")
    );
}

#[test]
fn a_failed_migration_leaves_the_file_alone() {
    fn broken(_: &mut DocumentMut) -> Result<(), MigrationError> {
        Err(MigrationError::Failed {
            from: 0,
            reason: "nope".into(),
        })
    }
    let dir = tempdir("badmigrate");
    let path = dir.join("config.toml");
    let old = "schema_version = 0\n";
    fs::write(&path, old).unwrap();
    let mut loaded = load_with(&path, "S", &[broken]);
    assert!(matches!(loaded.state, ConfigState::Unreadable { .. }));
    assert_eq!(fs::read_to_string(&path).unwrap(), old);
    assert!(save(&mut loaded, &path).is_err());
}

#[test]
fn stale_temp_files_are_cleaned_at_load() {
    let dir = tempdir("stale");
    fs::write(dir.join(".config.toml.tmp-1-0"), "junk").unwrap();
    let _ = load(&dir.join("config.toml"), "T");
    assert!(!dir.join(".config.toml.tmp-1-0").exists());
}

// ------------------------------------------------------------------------------ writing back

#[test]
fn save_round_trips_everything() {
    let dir = tempdir("roundtrip");
    let path = dir.join("config.toml");
    fs::write(&path, FULL).unwrap();
    let mut loaded = load(&path, "T");
    let before = loaded.config.clone();
    loaded.config.general.audio = false;
    loaded.config.general.audio = true; // change and change back: no net difference
    save(&mut loaded, &path).unwrap();
    assert_eq!(load(&path, "T").config, before);
}

#[test]
fn a_save_preserves_unknown_keys_comments_and_unrecognised_values() {
    let dir = tempdir("preserve");
    let path = dir.join("config.toml");
    fs::write(
        &path,
        "schema_version = 1\n# tuned by hand\nfuture_top_level = \"keep me\"\n\n[general]\n\
         fps_limit = \"144\" # a newer Lucerna's value\nsparkles = 3\naudio = false\n\n[brand_new]\nx = 1\n",
    )
    .unwrap();
    let mut loaded = load(&path, "T");
    loaded.config.general.audio = true; // the only field the user changes
    save(&mut loaded, &path).unwrap();

    let text = fs::read_to_string(&path).unwrap();
    for keep in [
        "# tuned by hand",
        "future_top_level = \"keep me\"",
        "fps_limit = \"144\" # a newer Lucerna's value",
        "sparkles = 3",
        "[brand_new]",
        "x = 1",
    ] {
        assert!(text.contains(keep), "lost {keep:?} in:\n{text}");
    }
    assert!(text.contains("audio = true"));
    assert!(load(&path, "T").config.general.audio);
}

#[test]
fn only_changed_fields_are_rewritten() {
    let dir = tempdir("minimal");
    let path = dir.join("config.toml");
    let original = "schema_version = 1\n\n[general]\npause_on_fullscreen = true   # spaced oddly\naudio = false\n";
    fs::write(&path, original).unwrap();
    let mut loaded = load(&path, "T");
    save(&mut loaded, &path).unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original,
        "an unchanged config is byte-identical"
    );
}

#[test]
fn creating_a_first_config_writes_a_valid_file() {
    let dir = tempdir("first");
    let path = dir.join("nested").join("config.toml");
    let mut loaded = load(&path, "T");
    assert!(loaded.first_run);
    loaded.config.general.audio = true;
    save(&mut loaded, &path).unwrap();
    assert!(!loaded.first_run);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("schema_version = 1"));
    assert!(load(&path, "T").config.general.audio);
}

#[test]
fn library_and_assignment_changes_survive_a_round_trip() {
    let dir = tempdir("library");
    let path = dir.join("config.toml");
    let media = dir.join("rain.webm");
    fs::write(&media, b"x").unwrap();

    let mut loaded = load(&path, "T");
    let id = loaded
        .config
        .add_wallpaper(&media, "Rain", "2026-09-30T10:00:00Z")
        .unwrap();
    loaded.config.all_displays.wallpaper = Some(id.clone());
    loaded.config.all_displays.scaling = ScalingMode::Fit;
    let display = OutputId::new("edid:DEL-a0b1-7XJ2K3");
    loaded.config.displays.insert(
        display.clone(),
        DisplayConfig {
            wallpaper: Some(id.clone()),
            scaling: Some(ScalingMode::Center),
            last_seen: Some("DP-1 — 1920×1080".into()),
        },
    );
    save(&mut loaded, &path).unwrap();

    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("[[wallpapers]]") && text.contains("added = 2026-09-30T10:00:00Z"),
        "{text}"
    );
    let reloaded = load(&path, "T");
    assert_eq!(reloaded.config, loaded.config);
    assert!(reloaded.warnings.is_empty(), "{:?}", reloaded.warnings);

    // Removing the wallpaper removes the entry and every assignment from the file, but not the media.
    let mut loaded = reloaded;
    loaded.config.remove_wallpaper(&id).unwrap();
    save(&mut loaded, &path).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains("[[wallpapers]]") && !text.contains(id.as_str()),
        "{text}"
    );
    assert!(media.exists());
    let reloaded = load(&path, "T");
    assert!(reloaded.config.wallpapers.is_empty());
    assert_eq!(reloaded.config.displays[&display].wallpaper, None);
}

#[test]
fn invalid_wallpaper_entries_survive_saves_of_other_changes() {
    let dir = tempdir("keepinvalid");
    let path = dir.join("config.toml");
    fs::write(
        &path,
        "[[wallpapers]]\nname = \"hand written, no id\"\npath = \"/a.mp4\"\n",
    )
    .unwrap();
    let mut loaded = load(&path, "T");
    let media = dir.join("b.mp4");
    fs::write(&media, b"x").unwrap();
    loaded.config.add_wallpaper(&media, "", "t").unwrap();
    save(&mut loaded, &path).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("hand written, no id"), "{text}");
    assert_eq!(load(&path, "T").config.wallpapers.len(), 1);
}

#[test]
fn saved_config_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir("mode");
    let path = dir.join("config.toml");
    let mut loaded = load(&path, "T");
    save(&mut loaded, &path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[allow(dead_code)]
fn _paths_are_used(_: &Path) {}
