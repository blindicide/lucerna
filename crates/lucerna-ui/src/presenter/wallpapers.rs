//! The wallpaper library page.

use lucerna_ipc::dto::WallpaperDto;

use crate::strings;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallpaperRow {
    pub id: String,
    pub title: String,
    /// The file path, shown under the title.
    pub subtitle: String,
    /// True when the file cannot be found right now; a "Missing" badge is shown.
    pub missing: bool,
    pub tooltip: String,
}

/// Library rows, sorted by name (case-insensitively), then path.
pub fn rows(list: &[WallpaperDto]) -> Vec<WallpaperRow> {
    let mut rows: Vec<WallpaperRow> = list
        .iter()
        .map(|w| WallpaperRow {
            id: w.id.clone(),
            title: if w.name.trim().is_empty() {
                strings::UNNAMED_WALLPAPER.to_owned()
            } else {
                w.name.clone()
            },
            subtitle: w.path.clone(),
            missing: !w.available,
            tooltip: if w.available {
                w.path.clone()
            } else {
                strings::missing_tooltip(&w.path)
            },
        })
        .collect();
    rows.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.subtitle.cmp(&b.subtitle))
    });
    rows
}

/// Heading and body of the confirmation shown before removing a library entry.
pub fn remove_confirmation(row: &WallpaperRow) -> (String, String) {
    (
        strings::remove_heading(&row.title),
        strings::REMOVE_BODY.to_owned(),
    )
}

/// MIME types offered by the file chooser's first filter.
pub const MIME_TYPES: &[&str] = &["video/*", "image/gif"];

/// File name suffixes that back up the MIME filter on systems with a sparse MIME database.
pub const SUFFIXES: &[&str] = &[
    "mp4", "webm", "mkv", "mov", "avi", "m4v", "ogv", "gif", "apng", "webp",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn wp(id: &str, name: &str, path: &str, available: bool) -> WallpaperDto {
        WallpaperDto {
            id: id.into(),
            name: name.into(),
            path: path.into(),
            available,
            ..WallpaperDto::default()
        }
    }

    #[test]
    fn rows_are_sorted_case_insensitively_and_flag_missing_files() {
        let rows = rows(&[
            wp("3", "zebra", "/v/z.mp4", true),
            wp("1", "Apple", "/v/a.mp4", false),
            wp("2", "banana", "/v/b.mp4", true),
        ]);
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["1", "2", "3"]
        );
        assert!(rows[0].missing && !rows[1].missing);
        assert!(rows[0].tooltip.contains("/v/a.mp4"));
        assert_eq!(rows[1].tooltip, "/v/b.mp4");
    }

    #[test]
    fn blank_names_get_a_placeholder() {
        assert_eq!(
            rows(&[wp("1", "  ", "/v/x.mp4", true)])[0].title,
            strings::UNNAMED_WALLPAPER
        );
    }

    #[test]
    fn the_removal_confirmation_promises_the_file_is_kept() {
        let row = rows(&[wp("1", "Rain", "/v/rain.webm", true)]).remove(0);
        let (heading, body) = remove_confirmation(&row);
        assert!(heading.contains("Rain"));
        assert!(body.contains("will not be deleted"), "{body}");
    }

    #[test]
    fn filters_cover_the_supported_containers() {
        for ext in ["mp4", "webm", "mkv", "gif"] {
            assert!(SUFFIXES.contains(&ext), "{ext}");
        }
        assert!(MIME_TYPES.contains(&"video/*"));
    }
}
