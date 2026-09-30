//! Reading a typed [`Config`] out of a TOML document, and writing changes back into it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value, value};

use super::{
    AllDisplays, Config, DisplayConfig, GeneralSettings, RendererSettings, Wallpaper, WallpaperId,
    X11Settings,
};
use crate::backend::OutputId;
use crate::types::{FpsLimit, HwDecode, MediaType, ScalingMode};

fn type_name(item: &Item) -> &'static str {
    item.type_name()
}

/// Read a boolean setting; wrong type falls back to `default` with a warning.
fn read_bool(
    table: Option<&Table>,
    section: &str,
    key: &str,
    default: bool,
    w: &mut Vec<String>,
) -> bool {
    match table.and_then(|t| t.get(key)) {
        None => default,
        Some(item) => item.as_bool().unwrap_or_else(|| {
            w.push(format!(
                "[{section}] {key} should be true or false (found {}); using {default}",
                type_name(item)
            ));
            default
        }),
    }
}

fn read_enum<T: FromStr + Copy>(
    table: Option<&Table>,
    section: &str,
    key: &str,
    default: T,
    w: &mut Vec<String>,
) -> T {
    let Some(item) = table.and_then(|t| t.get(key)) else {
        return default;
    };
    // Integers are accepted where the spelling is numeric (fps_limit = 60).
    let text = match (item.as_str(), item.as_integer()) {
        (Some(s), _) => s.to_owned(),
        (None, Some(n)) => n.to_string(),
        _ => {
            w.push(format!(
                "[{section}] {key} has the wrong type ({}); using the default",
                type_name(item)
            ));
            return default;
        }
    };
    text.parse().unwrap_or_else(|_| {
        w.push(format!(
            "[{section}] {key} = {text:?} is not recognised by this version; using the default"
        ));
        default
    })
}

fn read_uint(
    table: Option<&Table>,
    section: &str,
    key: &str,
    default: u64,
    w: &mut Vec<String>,
) -> u64 {
    match table.and_then(|t| t.get(key)) {
        None => default,
        Some(item) => item
            .as_integer()
            .and_then(|n| u64::try_from(n).ok())
            .unwrap_or_else(|| {
                w.push(format!(
                    "[{section}] {key} should be a non-negative integer; using {default}"
                ));
                default
            }),
    }
}

fn read_opt_id(
    table: Option<&Table>,
    section: &str,
    key: &str,
    w: &mut Vec<String>,
) -> Option<WallpaperId> {
    let item = table?.get(key)?;
    match item.as_str().and_then(WallpaperId::parse) {
        Some(id) => Some(id),
        None => {
            w.push(format!(
                "[{section}] {key} should be a wallpaper id string; ignoring it"
            ));
            None
        }
    }
}

fn datetime_text(item: &Item) -> Option<String> {
    match item.as_value()? {
        Value::Datetime(d) => Some(d.value().to_string()),
        Value::String(s) => Some(s.value().clone()),
        _ => None,
    }
}

/// Build the typed view of `doc`, leniently. Problems become warnings, never errors.
pub fn parse_doc(doc: &DocumentMut, warnings: &mut Vec<String>) -> Config {
    let root = doc.as_table();
    let section = |name: &str| root.get(name).and_then(Item::as_table);
    let defaults = Config::default();

    let general_table = section("general");
    let general = GeneralSettings {
        pause_on_fullscreen: read_bool(
            general_table,
            "general",
            "pause_on_fullscreen",
            defaults.general.pause_on_fullscreen,
            warnings,
        ),
        pause_on_lock: read_bool(
            general_table,
            "general",
            "pause_on_lock",
            defaults.general.pause_on_lock,
            warnings,
        ),
        audio: read_bool(
            general_table,
            "general",
            "audio",
            defaults.general.audio,
            warnings,
        ),
        hardware_decode: read_enum(
            general_table,
            "general",
            "hardware_decode",
            HwDecode::Auto,
            warnings,
        ),
        fps_limit: read_enum(
            general_table,
            "general",
            "fps_limit",
            FpsLimit::Native,
            warnings,
        ),
    };

    let renderer_table = section("renderer");
    let renderer = RendererSettings {
        max_restarts: u32::try_from(read_uint(
            renderer_table,
            "renderer",
            "max_restarts",
            3,
            warnings,
        ))
        .unwrap_or(u32::MAX),
        restart_window_secs: read_uint(
            renderer_table,
            "renderer",
            "restart_window_secs",
            60,
            warnings,
        ),
    };

    let x11 = X11Settings {
        stacking: read_enum(
            section("x11"),
            "x11",
            "stacking",
            Default::default(),
            warnings,
        ),
    };

    let all_table = section("all_displays");
    let all_displays = AllDisplays {
        wallpaper: read_opt_id(all_table, "all_displays", "wallpaper", warnings),
        scaling: read_enum(
            all_table,
            "all_displays",
            "scaling",
            ScalingMode::Fill,
            warnings,
        ),
    };

    let mut displays = BTreeMap::new();
    if let Some(table) = section("displays") {
        for (key, item) in table.iter() {
            let Some(display) = item.as_table() else {
                warnings.push(format!("[displays.{key:?}] should be a table; ignoring it"));
                continue;
            };
            let name = format!("displays.\"{key}\"");
            let scaling = display
                .get("scaling")
                .is_some()
                .then(|| read_enum(Some(display), &name, "scaling", ScalingMode::Fill, warnings));
            displays.insert(
                OutputId::new(key),
                DisplayConfig {
                    wallpaper: read_opt_id(Some(display), &name, "wallpaper", warnings),
                    scaling,
                    last_seen: display
                        .get("last_seen")
                        .and_then(Item::as_str)
                        .map(str::to_owned),
                },
            );
        }
    }

    let mut wallpapers = Vec::new();
    if let Some(tables) = root.get("wallpapers").and_then(Item::as_array_of_tables) {
        for (index, entry) in tables.iter().enumerate() {
            match parse_wallpaper(entry, warnings) {
                Some(w) => wallpapers.push(w),
                None => warnings.push(format!(
                    "[[wallpapers]] entry #{} is invalid and was skipped (it is kept in the file)",
                    index + 1
                )),
            }
        }
    }

    Config {
        general,
        renderer,
        x11,
        all_displays,
        displays,
        wallpapers,
    }
}

fn parse_wallpaper(entry: &Table, warnings: &mut Vec<String>) -> Option<Wallpaper> {
    let id = entry
        .get("id")
        .and_then(Item::as_str)
        .and_then(WallpaperId::parse)?;
    let path = PathBuf::from(entry.get("path").and_then(Item::as_str)?);
    if !path.is_absolute() {
        return None;
    }
    let name = entry
        .get("name")
        .and_then(Item::as_str)
        .map(str::to_owned)
        .filter(|n| !n.trim().is_empty())
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))?;
    let media_type = match entry.get("media_type").and_then(Item::as_str) {
        Some("video") => MediaType::Video,
        Some("animated-image") => MediaType::AnimatedImage,
        Some(_) | None => MediaType::from_path(&path),
    };
    let added = entry
        .get("added")
        .and_then(datetime_text)
        .unwrap_or_else(|| {
            warnings.push(format!("wallpaper {id} has no valid 'added' date"));
            String::new()
        });
    let available = entry
        .get("available")
        .and_then(Item::as_bool)
        .unwrap_or(true);
    Some(Wallpaper {
        id,
        name,
        path,
        media_type,
        added,
        available,
    })
}

// ---------------------------------------------------------------------------------------------
// Writing changes back
// ---------------------------------------------------------------------------------------------

fn ensure_table<'a>(root: &'a mut Table, name: &str) -> &'a mut Table {
    if !root.get(name).is_some_and(Item::is_table) {
        let mut table = Table::new();
        table.set_implicit(false);
        root.insert(name, Item::Table(table));
    }
    // INVARIANT: the entry was just ensured to be a table.
    #[allow(clippy::expect_used)]
    root.get_mut(name)
        .and_then(Item::as_table_mut)
        .expect("table was just inserted")
}

fn set_bool(t: &mut Table, key: &str, v: bool) {
    t.insert(key, value(v));
}

fn set_str(t: &mut Table, key: &str, v: &str) {
    t.insert(key, value(v));
}

fn set_or_remove_id(t: &mut Table, key: &str, id: &Option<WallpaperId>) {
    match id {
        Some(id) => set_str(t, key, id.as_str()),
        None => {
            t.remove(key);
        }
    }
}

/// Write every field of `new` that differs from what `doc` currently yields, leaving everything
/// else (unknown keys, comments, unrecognised values) untouched.
pub fn apply_to_doc(new: &Config, doc: &mut DocumentMut) {
    let old = parse_doc(doc, &mut Vec::new());
    let root = doc.as_table_mut();

    if old.general != new.general {
        let t = ensure_table(root, "general");
        if old.general.pause_on_fullscreen != new.general.pause_on_fullscreen {
            set_bool(t, "pause_on_fullscreen", new.general.pause_on_fullscreen);
        }
        if old.general.pause_on_lock != new.general.pause_on_lock {
            set_bool(t, "pause_on_lock", new.general.pause_on_lock);
        }
        if old.general.audio != new.general.audio {
            set_bool(t, "audio", new.general.audio);
        }
        if old.general.hardware_decode != new.general.hardware_decode {
            set_str(t, "hardware_decode", new.general.hardware_decode.as_str());
        }
        if old.general.fps_limit != new.general.fps_limit {
            set_str(t, "fps_limit", new.general.fps_limit.as_str());
        }
    }

    if old.renderer != new.renderer {
        let t = ensure_table(root, "renderer");
        if old.renderer.max_restarts != new.renderer.max_restarts {
            t.insert("max_restarts", value(i64::from(new.renderer.max_restarts)));
        }
        if old.renderer.restart_window_secs != new.renderer.restart_window_secs {
            t.insert(
                "restart_window_secs",
                value(i64::try_from(new.renderer.restart_window_secs).unwrap_or(i64::MAX)),
            );
        }
    }

    if old.x11 != new.x11 {
        set_str(
            ensure_table(root, "x11"),
            "stacking",
            new.x11.stacking.as_str(),
        );
    }

    if old.all_displays != new.all_displays {
        let t = ensure_table(root, "all_displays");
        if old.all_displays.wallpaper != new.all_displays.wallpaper {
            set_or_remove_id(t, "wallpaper", &new.all_displays.wallpaper);
        }
        if old.all_displays.scaling != new.all_displays.scaling {
            set_str(t, "scaling", new.all_displays.scaling.as_str());
        }
    }

    if old.displays != new.displays {
        let displays = ensure_table(root, "displays");
        displays.set_implicit(true);
        for id in old
            .displays
            .keys()
            .filter(|id| !new.displays.contains_key(*id))
        {
            displays.remove(id.as_str());
        }
        for (id, cfg) in &new.displays {
            if old.displays.get(id) == Some(cfg) {
                continue;
            }
            let t = ensure_table(displays, id.as_str());
            let before = old.displays.get(id);
            if before.map(|b| &b.wallpaper) != Some(&cfg.wallpaper) {
                set_or_remove_id(t, "wallpaper", &cfg.wallpaper);
            }
            if before.map(|b| b.scaling) != Some(cfg.scaling) {
                match cfg.scaling {
                    Some(mode) => set_str(t, "scaling", mode.as_str()),
                    None => {
                        t.remove("scaling");
                    }
                }
            }
            if before.map(|b| &b.last_seen) != Some(&cfg.last_seen) {
                match &cfg.last_seen {
                    Some(label) => set_str(t, "last_seen", label),
                    None => {
                        t.remove("last_seen");
                    }
                }
            }
        }
    }

    if old.wallpapers != new.wallpapers {
        apply_wallpapers(root, &old.wallpapers, &new.wallpapers);
    }
}

fn write_wallpaper(t: &mut Table, w: &Wallpaper, old: Option<&Wallpaper>) {
    if old.is_none_or(|o| o.id != w.id) {
        set_str(t, "id", w.id.as_str());
    }
    if old.is_none_or(|o| o.name != w.name) {
        set_str(t, "name", &w.name);
    }
    if old.is_none_or(|o| o.path != w.path) {
        set_str(t, "path", &w.path.to_string_lossy());
    }
    if old.is_none_or(|o| o.media_type != w.media_type) {
        set_str(t, "media_type", w.media_type.as_str());
    }
    if old.is_none_or(|o| o.added != w.added) {
        match toml_edit::Datetime::from_str(&w.added) {
            Ok(dt) => {
                t.insert(
                    "added",
                    Item::Value(Value::Datetime(toml_edit::Formatted::new(dt))),
                );
            }
            Err(_) => set_str(t, "added", &w.added),
        }
    }
    if old.is_none_or(|o| o.available != w.available) {
        set_bool(t, "available", w.available);
    }
}

fn apply_wallpapers(root: &mut Table, old: &[Wallpaper], new: &[Wallpaper]) {
    if !root.get("wallpapers").is_some_and(Item::is_array_of_tables) {
        root.insert("wallpapers", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    let Some(tables) = root
        .get_mut("wallpapers")
        .and_then(Item::as_array_of_tables_mut)
    else {
        return;
    };

    // Drop entries that were valid before and are gone now. Invalid entries were never part of the
    // typed view, so they are preserved.
    let removed: Vec<&str> = old
        .iter()
        .filter(|o| !new.iter().any(|n| n.id == o.id))
        .map(|o| o.id.as_str())
        .collect();
    let mut index = 0;
    while index < tables.len() {
        let is_removed = tables
            .get(index)
            .and_then(|t| t.get("id"))
            .and_then(Item::as_str)
            .is_some_and(|id| removed.contains(&id));
        if is_removed {
            tables.remove(index);
        } else {
            index += 1;
        }
    }

    for wallpaper in new {
        let before = old.iter().find(|o| o.id == wallpaper.id);
        if before == Some(wallpaper) {
            continue;
        }
        let existing = tables
            .iter_mut()
            .find(|t| t.get("id").and_then(Item::as_str) == Some(wallpaper.id.as_str()));
        match existing {
            Some(table) => write_wallpaper(table, wallpaper, before),
            None => {
                let mut table = Table::new();
                write_wallpaper(&mut table, wallpaper, None);
                tables.push(table);
            }
        }
    }
}
