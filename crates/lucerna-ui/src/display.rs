//! Decide whether a graphical session appears to exist, without touching GTK.

use std::ffi::OsString;

/// True when `DISPLAY` or `WAYLAND_DISPLAY` is set to a non-empty value.
///
/// `env` looks up one variable; injecting it keeps this testable without
/// changing the process environment.
pub fn graphical_session_available(env: impl Fn(&str) -> Option<OsString>) -> bool {
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .into_iter()
        .any(|key| env(key).is_some_and(|v| !v.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn nothing_set_means_no_session() {
        assert!(!graphical_session_available(lookup(&[])));
    }

    #[test]
    fn empty_values_do_not_count() {
        assert!(!graphical_session_available(lookup(&[
            ("DISPLAY", ""),
            ("WAYLAND_DISPLAY", "")
        ])));
    }

    #[test]
    fn display_or_wayland_counts() {
        assert!(graphical_session_available(lookup(&[("DISPLAY", ":0")])));
        assert!(graphical_session_available(lookup(&[(
            "WAYLAND_DISPLAY",
            "wayland-0"
        )])));
    }
}
