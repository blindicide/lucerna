//! Every user-visible string, kept in one place so localisation can be added later.

pub const WINDOW_TITLE: &str = "Lucerna";

/// Directive §24 message for a machine without a graphical session.
pub const NO_DISPLAY: &str = "Lucerna could not connect to a graphical display.\n\
DISPLAY is not set or no supported graphical session is available.\n\
Run this program from a desktop session, or use `lucernactl` from a terminal.";
