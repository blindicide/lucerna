//! Pure geometry used by the pause policy (directive §15).

use std::collections::BTreeSet;

use crate::backend::{OutputId, Rect};

/// An output counts as occluded when a single rectangle covers at least this share of its area.
pub const OCCLUSION_THRESHOLD_PERCENT: u64 = 90;

/// Which outputs are hidden behind fullscreen windows?
///
/// An output is occluded when some rectangle covers at least 90% of its area, so a window that
/// spans two monitors via `_NET_WM_FULLSCREEN_MONITORS` pauses both, while a window on the
/// neighbouring monitor pauses only that one.
pub fn occluded_outputs(outputs: &[(OutputId, Rect)], fullscreen: &[Rect]) -> BTreeSet<OutputId> {
    outputs
        .iter()
        .filter(|(_, geometry)| {
            let area = geometry.area();
            area > 0
                && fullscreen.iter().any(|rect| {
                    geometry.intersection_area(rect) * 100 >= area * OCCLUSION_THRESHOLD_PERCENT
                })
        })
        .map(|(id, _)| id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_monitors() -> Vec<(OutputId, Rect)> {
        vec![
            (OutputId::new("a"), Rect::new(0, 0, 1920, 1080)),
            (OutputId::new("b"), Rect::new(1920, 0, 1280, 1024)),
        ]
    }

    fn ids(set: &BTreeSet<OutputId>) -> Vec<&str> {
        set.iter().map(OutputId::as_str).collect()
    }

    #[test]
    fn fullscreen_on_one_monitor_occludes_only_that_monitor() {
        let occluded = occluded_outputs(&two_monitors(), &[Rect::new(0, 0, 1920, 1080)]);
        assert_eq!(ids(&occluded), ["a"]);
        let occluded = occluded_outputs(&two_monitors(), &[Rect::new(1920, 0, 1280, 1024)]);
        assert_eq!(ids(&occluded), ["b"]);
    }

    #[test]
    fn a_window_spanning_both_monitors_occludes_both() {
        let occluded = occluded_outputs(&two_monitors(), &[Rect::new(0, 0, 3200, 1080)]);
        assert_eq!(ids(&occluded), ["a", "b"]);
    }

    #[test]
    fn the_threshold_is_ninety_percent() {
        let outputs = vec![(OutputId::new("a"), Rect::new(0, 0, 1000, 1000))];
        // 90% exactly covers.
        assert_eq!(
            occluded_outputs(&outputs, &[Rect::new(0, 0, 900, 1000)]).len(),
            1
        );
        // 89.9% does not.
        assert!(occluded_outputs(&outputs, &[Rect::new(0, 0, 899, 1000)]).is_empty());
    }

    #[test]
    fn no_windows_or_zero_sized_outputs_occlude_nothing() {
        assert!(occluded_outputs(&two_monitors(), &[]).is_empty());
        let empty = vec![(OutputId::new("z"), Rect::new(0, 0, 0, 0))];
        assert!(occluded_outputs(&empty, &[Rect::new(0, 0, 10, 10)]).is_empty());
    }

    #[test]
    fn several_small_windows_do_not_add_up() {
        // Only a *single* fullscreen rectangle counts; two halves are not fullscreen windows.
        let outputs = vec![(OutputId::new("a"), Rect::new(0, 0, 1000, 1000))];
        let halves = [Rect::new(0, 0, 500, 1000), Rect::new(500, 0, 500, 1000)];
        assert!(occluded_outputs(&outputs, &halves).is_empty());
    }
}
