//! Login autostart through the XDG autostart directory (directive §22).
//!
//! The only startup mechanism is one per-user `.desktop` file. There is no systemd unit and no
//! D-Bus activation file, so removing the file really does mean the daemon will not come back.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fsutil::atomic_write;

/// File name of the autostart entry.
pub const FILE_NAME: &str = "org.lucerna.Lucerna.Daemon.desktop";

/// What the autostart file currently says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutostartState {
    /// No file.
    Absent,
    /// A file exists and is not disabled.
    Enabled { exec: String },
    /// A file exists but is disabled (`Hidden=true` or `X-GNOME-Autostart-enabled=false`), for
    /// example from Cinnamon's *Startup Applications*. Lucerna respects that.
    Disabled,
}

impl AutostartState {
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled { .. })
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Enabled { .. } => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

/// Render the entry. `TryExec` makes it inert if the package is later uninstalled.
pub fn render(daemon_path: &Path) -> String {
    let exec = escape_exec(&daemon_path.to_string_lossy());
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Lucerna wallpaper service\n\
         Comment=Restores animated wallpapers after login\n\
         Exec={exec}\n\
         TryExec={exec}\n\
         Icon=org.lucerna.Lucerna\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n\
         X-Lucerna-Managed=true\n"
    )
}

/// Quote an `Exec` value per the Desktop Entry specification when it has spaces or specials.
fn escape_exec(path: &str) -> String {
    if path
        .chars()
        .any(|c| c.is_whitespace() || "\"'\\><~|&;$*?#()`".contains(c))
    {
        let escaped: String = path
            .chars()
            .flat_map(|c| {
                if "\"`$\\".contains(c) {
                    vec!['\\', c]
                } else {
                    vec![c]
                }
            })
            .collect();
        format!("\"{escaped}\"")
    } else {
        path.to_owned()
    }
}

fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry
            && let Some(rest) = line.strip_prefix(key)
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            return Some(value.trim());
        }
    }
    None
}

/// Inspect the autostart file at `path`.
pub fn state(path: &Path) -> AutostartState {
    let Ok(text) = fs::read_to_string(path) else {
        return AutostartState::Absent;
    };
    let hidden = value_of(&text, "Hidden").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let gnome_off = value_of(&text, "X-GNOME-Autostart-enabled")
        .is_some_and(|v| v.eq_ignore_ascii_case("false"));
    if hidden || gnome_off {
        AutostartState::Disabled
    } else {
        AutostartState::Enabled {
            exec: value_of(&text, "Exec").unwrap_or_default().to_owned(),
        }
    }
}

/// Enable autostart by writing the entry atomically.
pub fn enable(path: &Path, daemon_path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    atomic_write(path, render(daemon_path).as_bytes())
}

/// Disable autostart by removing our file. Removing an absent file is fine.
///
/// A file that some other tool turned into a disabled entry is left alone: it is already
/// disabled, and the user's own edit is not ours to delete.
pub fn disable(path: &Path) -> io::Result<()> {
    match state(path) {
        AutostartState::Enabled { .. } => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        },
        AutostartState::Absent | AutostartState::Disabled => Ok(()),
    }
}

/// The default location of the autostart file below an XDG config home.
pub fn path_in(xdg_config_home: &Path) -> PathBuf {
    xdg_config_home.join("autostart").join(FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lucerna-auto-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rendered_entry_is_a_valid_minimal_desktop_file() {
        let text = render(Path::new("/usr/bin/lucernad"));
        assert!(text.starts_with("[Desktop Entry]\n"));
        for line in [
            "Type=Application",
            "Exec=/usr/bin/lucernad",
            "TryExec=/usr/bin/lucernad",
            "Terminal=false",
            "X-GNOME-Autostart-enabled=true",
        ] {
            assert!(text.lines().any(|l| l == line), "missing {line}");
        }
    }

    #[test]
    fn exec_with_spaces_is_quoted_per_the_spec() {
        let text = render(Path::new("/opt/my apps/lucernad"));
        assert!(text.contains("Exec=\"/opt/my apps/lucernad\""));
        assert_eq!(escape_exec("/usr/bin/lucernad"), "/usr/bin/lucernad");
        assert_eq!(escape_exec("/a$b/x"), "\"/a\\$b/x\"");
    }

    #[test]
    fn enable_disable_round_trip() {
        let dir = tempdir("roundtrip");
        let file = dir.join("autostart").join(FILE_NAME);
        assert_eq!(state(&file), AutostartState::Absent);
        enable(&file, Path::new("/usr/bin/lucernad")).unwrap();
        assert_eq!(
            state(&file),
            AutostartState::Enabled {
                exec: "/usr/bin/lucernad".into()
            }
        );
        enable(&file, Path::new("/usr/bin/lucernad")).unwrap(); // idempotent
        disable(&file).unwrap();
        assert_eq!(
            state(&file),
            AutostartState::Absent,
            "disabling really removes the entry"
        );
        disable(&file).unwrap(); // idempotent
    }

    #[test]
    fn an_entry_disabled_elsewhere_is_reported_and_left_alone() {
        let dir = tempdir("hidden");
        let file = dir.join(FILE_NAME);
        fs::write(
            &file,
            "[Desktop Entry]\nType=Application\nExec=lucernad\nHidden=true\n",
        )
        .unwrap();
        assert_eq!(state(&file), AutostartState::Disabled);
        disable(&file).unwrap();
        assert!(file.exists(), "the user's own edit is not deleted");

        fs::write(
            &file,
            "[Desktop Entry]\nExec=lucernad\nX-GNOME-Autostart-enabled=false\n",
        )
        .unwrap();
        assert_eq!(state(&file), AutostartState::Disabled);
    }

    #[test]
    fn keys_outside_the_desktop_entry_group_are_ignored() {
        let dir = tempdir("groups");
        let file = dir.join(FILE_NAME);
        fs::write(
            &file,
            "[Desktop Entry]\nExec=lucernad\n\n[Desktop Action x]\nHidden=true\n",
        )
        .unwrap();
        assert!(state(&file).is_enabled());
    }

    #[test]
    fn path_is_below_the_autostart_directory() {
        assert_eq!(
            path_in(Path::new("/home/u/.config")),
            Path::new("/home/u/.config/autostart/org.lucerna.Lucerna.Daemon.desktop")
        );
    }
}
