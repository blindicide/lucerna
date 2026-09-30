//! The settings page: drop-down contents and the mapping between indices and D-Bus values.

pub const HARDWARE_DECODE: &[(&str, &str)] = &[
    ("auto", "Automatic (recommended)"),
    ("disabled", "Disabled"),
];

pub const FPS_LIMIT: &[(&str, &str)] = &[
    ("native", "Native (no limit)"),
    ("60", "60 fps"),
    ("30", "30 fps"),
    ("15", "15 fps"),
];

pub const STACKING: &[(&str, &str)] = &[
    ("auto", "Automatic"),
    ("override-redirect", "Bottom of the window stack"),
    (
        "desktop-window",
        "Desktop window (managed by the window manager)",
    ),
];

pub fn index_of(options: &[(&str, &str)], value: &str) -> u32 {
    options
        .iter()
        .position(|(id, _)| *id == value)
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or(0)
}

pub fn value_at(options: &'static [(&'static str, &'static str)], index: u32) -> &'static str {
    usize::try_from(index)
        .ok()
        .and_then(|i| options.get(i))
        .map_or(options[0].0, |(id, _)| id)
}

pub fn labels(options: &'static [(&'static str, &'static str)]) -> Vec<&'static str> {
    options.iter().map(|(_, label)| *label).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_round_trip_through_indices() {
        for options in [HARDWARE_DECODE, FPS_LIMIT, STACKING] {
            for (i, (id, _)) in options.iter().enumerate() {
                let index = u32::try_from(i).unwrap();
                assert_eq!(index_of(options, id), index);
                assert_eq!(value_at(options, index), *id);
            }
            assert_eq!(index_of(options, "unknown"), 0);
            assert_eq!(value_at(options, 99), options[0].0);
        }
    }

    #[test]
    fn the_values_are_the_ones_the_daemon_accepts() {
        let ids = |o: &'static [(&'static str, &'static str)]| {
            o.iter().map(|(id, _)| *id).collect::<Vec<_>>()
        };
        assert_eq!(ids(HARDWARE_DECODE), ["auto", "disabled"]);
        assert_eq!(ids(FPS_LIMIT), ["native", "60", "30", "15"]);
        assert_eq!(
            ids(STACKING),
            ["auto", "override-redirect", "desktop-window"]
        );
    }

    #[test]
    fn labels_are_human_readable() {
        assert!(labels(FPS_LIMIT).iter().all(|l| !l.is_empty()));
        assert_eq!(labels(HARDWARE_DECODE)[0], "Automatic (recommended)");
    }
}
