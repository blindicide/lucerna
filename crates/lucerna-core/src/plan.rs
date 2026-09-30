//! Reconciliation: turning "what should be running" into the actions that get there.
//!
//! Pure: it compares two maps and returns [`Action`]s in a deterministic order, so the daemon's
//! behaviour on hotplug, reassignment and missing files is table-testable without any process.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::backend::{OutputId, Rect};
use crate::config::WallpaperId;
use crate::types::{FpsLimit, HwDecode, ScalingMode};

/// Everything that can only change by restarting mpv (launch-time flags).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchKey {
    pub media: PathBuf,
    pub hwdec: HwDecode,
    pub fps: FpsLimit,
    pub audio: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesiredRenderer {
    pub wallpaper: WallpaperId,
    pub launch: LaunchKey,
    pub scaling: ScalingMode,
    pub paused: bool,
    pub geometry: Rect,
}

/// Only present outputs that have an effective, available wallpaper appear here.
pub type Desired = BTreeMap<OutputId, DesiredRenderer>;
pub type Actual = BTreeMap<OutputId, DesiredRenderer>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Stop and remove the renderer and surface of an output that no longer needs one.
    Destroy(OutputId),
    /// Restart with new launch-time options (media, hwdec, fps, audio) on the same surface.
    Replace(OutputId),
    /// Create a surface and start a renderer.
    Create(OutputId),
    Resize(OutputId, Rect),
    SetScaling(OutputId, ScalingMode),
    SetPaused(OutputId, bool),
}

/// Compute the actions that turn `actual` into `desired`.
///
/// Order: Destroy, Replace, Create, then the live changes (Resize, SetScaling, SetPaused). Within
/// each group outputs are in id order.
pub fn reconcile(desired: &Desired, actual: &Actual) -> Vec<Action> {
    let mut destroy = Vec::new();
    let mut replace = Vec::new();
    let mut create = Vec::new();
    let mut live = Vec::new();

    for id in actual.keys().filter(|id| !desired.contains_key(*id)) {
        destroy.push(Action::Destroy(id.clone()));
    }
    for (id, want) in desired {
        match actual.get(id) {
            None => create.push(Action::Create(id.clone())),
            Some(have) if have.launch != want.launch => {
                replace.push(Action::Replace(id.clone()));
                // The replacement is created on the current geometry.
                if have.geometry != want.geometry {
                    live.push(Action::Resize(id.clone(), want.geometry));
                }
            }
            Some(have) => {
                if have.geometry != want.geometry {
                    live.push(Action::Resize(id.clone(), want.geometry));
                }
                if have.scaling != want.scaling {
                    live.push(Action::SetScaling(id.clone(), want.scaling));
                }
                if have.paused != want.paused {
                    live.push(Action::SetPaused(id.clone(), want.paused));
                }
            }
        }
    }

    destroy
        .into_iter()
        .chain(replace)
        .chain(create)
        .chain(live)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(id: &str) -> OutputId {
        OutputId::new(id)
    }

    fn renderer(file: &str) -> DesiredRenderer {
        DesiredRenderer {
            wallpaper: WallpaperId::parse(file).unwrap(),
            launch: LaunchKey {
                media: PathBuf::from(format!("/media/{file}.mp4")),
                hwdec: HwDecode::Auto,
                fps: FpsLimit::Native,
                audio: false,
            },
            scaling: ScalingMode::Fill,
            paused: false,
            geometry: Rect::new(0, 0, 1920, 1080),
        }
    }

    fn map(entries: &[(&str, DesiredRenderer)]) -> BTreeMap<OutputId, DesiredRenderer> {
        entries.iter().map(|(id, r)| (out(id), r.clone())).collect()
    }

    #[test]
    fn nothing_to_do_when_actual_matches_desired() {
        let d = map(&[("a", renderer("w1"))]);
        assert!(reconcile(&d, &d.clone()).is_empty());
        assert!(reconcile(&Desired::new(), &Actual::new()).is_empty());
    }

    #[test]
    fn a_new_display_gets_a_renderer_and_a_removed_one_loses_it() {
        let a = renderer("w1");
        let actual = map(&[("a", a.clone())]);
        let desired = map(&[("a", a.clone()), ("b", renderer("w1"))]);
        assert_eq!(reconcile(&desired, &actual), [Action::Create(out("b"))]);
        assert_eq!(reconcile(&actual, &desired), [Action::Destroy(out("b"))]);
    }

    #[test]
    fn a_returning_display_is_created_again_with_its_configured_wallpaper() {
        // Hotplug: b was present, disappears (destroy), then reappears (create).
        let both = map(&[("a", renderer("w1")), ("b", renderer("w2"))]);
        let only_a = map(&[("a", renderer("w1"))]);
        assert_eq!(reconcile(&only_a, &both), [Action::Destroy(out("b"))]);
        assert_eq!(reconcile(&both, &only_a), [Action::Create(out("b"))]);
    }

    #[test]
    fn launch_time_changes_replace_and_live_changes_do_not() {
        let base = renderer("w1");
        let actual = map(&[("a", base.clone())]);

        for change in [
            |r: &mut DesiredRenderer| r.launch.media = PathBuf::from("/media/other.mp4"),
            |r: &mut DesiredRenderer| r.launch.hwdec = HwDecode::Disabled,
            |r: &mut DesiredRenderer| r.launch.fps = FpsLimit::Fps30,
            |r: &mut DesiredRenderer| r.launch.audio = true,
        ] {
            let mut want = base.clone();
            change(&mut want);
            assert_eq!(
                reconcile(&map(&[("a", want)]), &actual),
                [Action::Replace(out("a"))]
            );
        }

        let mut scaled = base.clone();
        scaled.scaling = ScalingMode::Center;
        assert_eq!(
            reconcile(&map(&[("a", scaled)]), &actual),
            [Action::SetScaling(out("a"), ScalingMode::Center)]
        );

        let mut paused = base.clone();
        paused.paused = true;
        assert_eq!(
            reconcile(&map(&[("a", paused)]), &actual),
            [Action::SetPaused(out("a"), true)]
        );

        let mut moved = base.clone();
        moved.geometry = Rect::new(1920, 0, 1280, 1024);
        assert_eq!(
            reconcile(&map(&[("a", moved.clone())]), &actual),
            [Action::Resize(out("a"), moved.geometry)]
        );
    }

    #[test]
    fn a_different_wallpaper_id_with_the_same_file_needs_no_restart() {
        let base = renderer("w1");
        let mut other = base.clone();
        other.wallpaper = WallpaperId::parse("w-same-file").unwrap();
        assert!(reconcile(&map(&[("a", other)]), &map(&[("a", base)])).is_empty());
    }

    #[test]
    fn a_global_to_per_display_change_replaces_only_the_display_that_changed() {
        let actual = map(&[("a", renderer("g")), ("b", renderer("g"))]);
        let desired = map(&[("a", renderer("special")), ("b", renderer("g"))]);
        assert_eq!(reconcile(&desired, &actual), [Action::Replace(out("a"))]);
    }

    #[test]
    fn a_missing_file_yields_no_desired_renderer_so_the_running_one_is_destroyed() {
        let actual = map(&[("a", renderer("w1"))]);
        assert_eq!(
            reconcile(&Desired::new(), &actual),
            [Action::Destroy(out("a"))]
        );
    }

    #[test]
    fn actions_come_in_a_deterministic_group_order() {
        let mut moved = renderer("keep");
        moved.geometry = Rect::new(5, 5, 100, 100);
        let actual = map(&[
            ("a", renderer("old")),
            ("b", renderer("keep")),
            ("c", renderer("replace-me")),
        ]);
        let desired = map(&[
            ("b", moved.clone()),
            ("c", renderer("new-file")),
            ("d", renderer("fresh")),
        ]);
        assert_eq!(
            reconcile(&desired, &actual),
            [
                Action::Destroy(out("a")),
                Action::Replace(out("c")),
                Action::Create(out("d")),
                Action::Resize(out("b"), moved.geometry),
            ]
        );
    }
}
