//! Pause policy (directive §15): pure decisions from inputs, no I/O.

use std::collections::BTreeSet;

use crate::backend::OutputId;
use crate::config::GeneralSettings;

/// Why a renderer is (or should be) paused. Paused iff any reason is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseReasons {
    pub user: bool,
    pub lock: bool,
    pub fullscreen: bool,
}

impl PauseReasons {
    pub fn any(self) -> bool {
        self.user || self.lock || self.fullscreen
    }

    /// The reasons as stable strings for D-Bus (`user`, `lock`, `fullscreen`).
    pub fn names(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.user {
            out.push("user");
        }
        if self.lock {
            out.push("lock");
        }
        if self.fullscreen {
            out.push("fullscreen");
        }
        out
    }
}

pub struct PolicyInputs<'a> {
    /// `Pause` was called and `Resume` has not been.
    pub user_paused: bool,
    pub session_locked: bool,
    /// Outputs currently covered by a fullscreen window.
    pub occluded: &'a BTreeSet<OutputId>,
    pub settings: &'a GeneralSettings,
}

/// Decide the pause reasons for one output.
///
/// `lock` counts only if `pause_on_lock` is on; `fullscreen` only if `pause_on_fullscreen` is on
/// and this very output is occluded. Merely maximised windows never cause a pause: they are not
/// even inputs here.
pub fn evaluate(output: &OutputId, inputs: &PolicyInputs<'_>) -> PauseReasons {
    PauseReasons {
        user: inputs.user_paused,
        lock: inputs.session_locked && inputs.settings.pause_on_lock,
        fullscreen: inputs.settings.pause_on_fullscreen && inputs.occluded.contains(output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(id: &str) -> OutputId {
        OutputId::new(id)
    }

    fn reasons(user: bool, lock: bool, occluded: bool, s: &GeneralSettings) -> PauseReasons {
        let set: BTreeSet<OutputId> = if occluded {
            [out("a")].into()
        } else {
            BTreeSet::new()
        };
        evaluate(
            &out("a"),
            &PolicyInputs {
                user_paused: user,
                session_locked: lock,
                occluded: &set,
                settings: s,
            },
        )
    }

    #[test]
    fn every_combination_of_inputs_and_settings() {
        for pause_on_fullscreen in [false, true] {
            for pause_on_lock in [false, true] {
                let settings = GeneralSettings {
                    pause_on_fullscreen,
                    pause_on_lock,
                    ..Default::default()
                };
                for user in [false, true] {
                    for lock in [false, true] {
                        for occluded in [false, true] {
                            let got = reasons(user, lock, occluded, &settings);
                            let want = PauseReasons {
                                user,
                                lock: lock && pause_on_lock,
                                fullscreen: occluded && pause_on_fullscreen,
                            };
                            assert_eq!(
                                got, want,
                                "user={user} lock={lock} occluded={occluded} {settings:?}"
                            );
                            assert_eq!(got.any(), want.user || want.lock || want.fullscreen);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn defaults_pause_on_fullscreen_and_lock() {
        let s = GeneralSettings::default();
        assert!(s.pause_on_fullscreen && s.pause_on_lock);
        assert!(reasons(false, false, true, &s).fullscreen);
        assert!(reasons(false, true, false, &s).lock);
        assert!(
            !reasons(false, false, false, &s).any(),
            "nothing to pause for"
        );
    }

    #[test]
    fn only_the_occluded_output_pauses() {
        let occluded: BTreeSet<OutputId> = [out("a")].into();
        let s = GeneralSettings::default();
        let inputs = PolicyInputs {
            user_paused: false,
            session_locked: false,
            occluded: &occluded,
            settings: &s,
        };
        assert!(evaluate(&out("a"), &inputs).fullscreen);
        assert!(!evaluate(&out("b"), &inputs).any());
    }

    #[test]
    fn reason_names_are_stable() {
        assert_eq!(
            PauseReasons {
                user: true,
                lock: true,
                fullscreen: true
            }
            .names(),
            ["user", "lock", "fullscreen"]
        );
        assert!(PauseReasons::default().names().is_empty());
    }
}
