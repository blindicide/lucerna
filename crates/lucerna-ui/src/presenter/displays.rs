//! The displays page.

use lucerna_ipc::dto::{DisplayDto, WallpaperDto};

use crate::strings;

/// One entry of a wallpaper drop-down. `id` is `None` for "no wallpaper" / "same as all displays".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: Option<String>,
    pub label: String,
}

/// The wallpaper drop-down entries. Per-display drop-downs start with "Same as all displays";
/// the all-displays drop-down starts with "None".
pub fn wallpaper_choices(wallpapers: &[WallpaperDto], for_all_displays: bool) -> Vec<Choice> {
    let first = Choice {
        id: None,
        label: if for_all_displays {
            strings::NO_WALLPAPER
        } else {
            strings::SAME_AS_ALL
        }
        .to_owned(),
    };
    let mut named: Vec<Choice> = wallpapers
        .iter()
        .map(|w| Choice {
            id: Some(w.id.clone()),
            label: if w.available {
                w.name.clone()
            } else {
                strings::missing_choice(&w.name)
            },
        })
        .collect();
    named.sort_by_key(|c| c.label.to_lowercase());
    std::iter::once(first).chain(named).collect()
}

/// Index of `wallpaper_id` in `choices`; the first entry if it is empty or not found.
pub fn selected_index(choices: &[Choice], wallpaper_id: &str) -> u32 {
    if wallpaper_id.is_empty() {
        return 0;
    }
    choices
        .iter()
        .position(|c| c.id.as_deref() == Some(wallpaper_id))
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0)
}

/// Scaling modes in drop-down order: (id sent over D-Bus, label).
pub const SCALING: &[(&str, &str)] = &[
    ("fill", "Fill (crop to cover)"),
    ("fit", "Fit (letterbox)"),
    ("stretch", "Stretch"),
    ("center", "Center (original size)"),
];

pub fn scaling_index(mode: &str) -> u32 {
    SCALING
        .iter()
        .position(|(id, _)| *id == mode)
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0)
}

pub fn scaling_id(index: u32) -> &'static str {
    usize::try_from(index)
        .ok()
        .and_then(|i| SCALING.get(i))
        .map_or("fill", |(id, _)| id)
}

/// Per-display scaling entries: the first follows the all-displays mode.
pub const DISPLAY_SCALING: &[(&str, &str)] = &[
    ("inherit", "Same as all displays"),
    ("fill", "Fill (crop to cover)"),
    ("fit", "Fit (letterbox)"),
    ("stretch", "Stretch"),
    ("center", "Center (original size)"),
];

pub fn display_scaling_index(scaling: &str, source: &str) -> u32 {
    if source != "display" {
        return 0;
    }
    DISPLAY_SCALING
        .iter()
        .position(|(id, _)| *id == scaling)
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0)
}

pub fn display_scaling_id(index: u32) -> &'static str {
    usize::try_from(index)
        .ok()
        .and_then(|i| DISPLAY_SCALING.get(i))
        .map_or("inherit", |(id, _)| id)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayRow {
    pub id: String,
    /// For example `HDMI-1 — 1920×1080` or `eDP-1 — 1920×1080 — Primary`.
    pub title: String,
    /// What is playing there, in words.
    pub subtitle: String,
    pub connected: bool,
    /// Index into the per-display wallpaper choices (0 = same as all displays).
    pub wallpaper_index: u32,
    /// Index into [`DISPLAY_SCALING`].
    pub scaling_index: u32,
}

/// One row per display (connected first), naming the wallpaper it shows.
pub fn rows(displays: &[DisplayDto], wallpapers: &[WallpaperDto]) -> Vec<DisplayRow> {
    let name_of = |id: &str| {
        wallpapers
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.name.clone())
    };
    let choices = wallpaper_choices(wallpapers, false);
    let mut rows: Vec<DisplayRow> = displays
        .iter()
        .map(|d| {
            let subtitle = match (d.wallpaper_id.as_str(), d.wallpaper_source.as_str()) {
                ("", _) => strings::NO_WALLPAPER_ASSIGNED.to_owned(),
                (id, "all") => strings::inherits(&name_of(id).unwrap_or_else(|| id.to_owned())),
                (id, _) => name_of(id).unwrap_or_else(|| id.to_owned()),
            };
            // Only an override selects a wallpaper here; otherwise the entry is "same as all".
            let own = if d.wallpaper_source == "display" {
                d.wallpaper_id.as_str()
            } else {
                ""
            };
            DisplayRow {
                id: d.id.clone(),
                title: d.label.clone(),
                subtitle,
                connected: d.connected,
                wallpaper_index: selected_index(&choices, own),
                scaling_index: display_scaling_index(&d.scaling, &d.scaling_source),
            }
        })
        .collect();
    rows.sort_by_key(|r| !r.connected);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wp(id: &str, name: &str, available: bool) -> WallpaperDto {
        WallpaperDto {
            id: id.into(),
            name: name.into(),
            available,
            ..WallpaperDto::default()
        }
    }

    #[test]
    fn per_display_and_global_choices_start_differently() {
        let list = [wp("b", "Beta", true), wp("a", "alpha", false)];
        let global = wallpaper_choices(&list, true);
        assert_eq!(
            global[0],
            Choice {
                id: None,
                label: strings::NO_WALLPAPER.into()
            }
        );
        let per = wallpaper_choices(&list, false);
        assert_eq!(per[0].label, strings::SAME_AS_ALL);
        assert_eq!(
            per.iter()
                .skip(1)
                .map(|c| c.id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(per[1].label.contains("missing"), "{}", per[1].label);
    }

    #[test]
    fn selection_falls_back_to_the_first_entry() {
        let choices = wallpaper_choices(&[wp("a", "A", true), wp("b", "B", true)], true);
        assert_eq!(selected_index(&choices, ""), 0);
        assert_eq!(selected_index(&choices, "b"), 2);
        assert_eq!(selected_index(&choices, "gone"), 0);
    }

    #[test]
    fn scaling_ids_and_indices_round_trip() {
        for (i, (id, _)) in SCALING.iter().enumerate() {
            assert_eq!(scaling_index(id), u32::try_from(i).unwrap());
            assert_eq!(scaling_id(u32::try_from(i).unwrap()), *id);
        }
        assert_eq!(scaling_index("nonsense"), 0);
        assert_eq!(scaling_id(99), "fill");
    }

    #[test]
    fn rows_name_the_wallpaper_and_show_connected_displays_first() {
        let displays = [
            DisplayDto {
                id: "gone".into(),
                label: "DP-2 — 1920×1080 (disconnected)".into(),
                connected: false,
                wallpaper_id: "w".into(),
                wallpaper_source: "display".into(),
                scaling: "center".into(),
                scaling_source: "display".into(),
                ..DisplayDto::default()
            },
            DisplayDto {
                id: "here".into(),
                label: "HDMI-1 — 1920×1080".into(),
                connected: true,
                wallpaper_id: "w".into(),
                wallpaper_source: "all".into(),
                ..DisplayDto::default()
            },
            DisplayDto {
                id: "bare".into(),
                label: "eDP-1 — 1920×1080 — Primary".into(),
                connected: true,
                ..DisplayDto::default()
            },
        ];
        let rows = rows(&displays, &[wp("w", "Rain", true)]);
        assert_eq!(
            rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["here", "bare", "gone"]
        );
        assert!(
            rows[0].subtitle.contains("Rain") && rows[0].subtitle.contains("all displays"),
            "{}",
            rows[0].subtitle
        );
        assert_eq!(rows[1].subtitle, strings::NO_WALLPAPER_ASSIGNED);
        assert_eq!(rows[2].subtitle, "Rain");
        // Only an override selects a wallpaper in the per-display drop-down.
        assert_eq!(rows[0].wallpaper_index, 0, "inherits: same as all displays");
        assert_eq!(
            rows[2].wallpaper_index, 1,
            "overrides: the wallpaper itself"
        );
        assert_eq!(rows[0].scaling_index, 0);
        assert_eq!(
            rows[2].scaling_index,
            display_scaling_index("center", "display")
        );
    }

    #[test]
    fn per_display_scaling_entries_round_trip_and_start_with_inherit() {
        assert_eq!(DISPLAY_SCALING[0].0, "inherit");
        for (i, (id, _)) in DISPLAY_SCALING.iter().enumerate() {
            let index = u32::try_from(i).unwrap();
            assert_eq!(display_scaling_id(index), *id);
            if i > 0 {
                assert_eq!(display_scaling_index(id, "display"), index);
            }
        }
        assert_eq!(display_scaling_index("fit", "all"), 0, "not overridden");
        assert_eq!(display_scaling_id(99), "inherit");
    }
}
