//! Every user-visible string, kept in one place so localisation can be added later.
//!
//! English only for v1. Functions build the few strings that contain values.

pub const APP_NAME: &str = "Lucerna";
pub const WINDOW_TITLE: &str = "Lucerna";

/// Directive §24 message for a machine without a graphical session.
pub const NO_DISPLAY: &str = "Lucerna could not connect to a graphical display.\n\
DISPLAY is not set or no supported graphical session is available.\n\
Run this program from a desktop session, or use `lucernactl` from a terminal.";

// Navigation pages
pub const PAGE_WALLPAPERS: &str = "Wallpapers";
pub const PAGE_DISPLAYS: &str = "Displays";
pub const PAGE_SETTINGS: &str = "Settings";
pub const PAGE_ABOUT: &str = "About";

// Wallpapers page
pub const ADD_WALLPAPER: &str = "Add…";
pub const REMOVE_WALLPAPER: &str = "Remove from library";
pub const SET_ON_ALL: &str = "Set on all displays";
pub const STOP_WALLPAPER: &str = "Stop wallpaper";
pub const LIBRARY_EMPTY: &str =
    "Your library is empty. Choose “Add…” to pick a video or animated image.";
pub const UNNAMED_WALLPAPER: &str = "Unnamed wallpaper";
pub const MISSING_BADGE: &str = "Missing";
pub const FILE_DIALOG_TITLE: &str = "Add a wallpaper";
pub const FILTER_MEDIA: &str = "Videos and animated images";
pub const FILTER_ALL: &str = "All files";
pub const REMOVE_BODY: &str =
    "The entry is removed from Lucerna's library. The file itself will not be deleted.";
pub const CANCEL: &str = "Cancel";
pub const REMOVE: &str = "Remove";

pub fn missing_tooltip(path: &str) -> String {
    format!(
        "{path}\nThis file cannot be found. Lucerna keeps the entry and resumes automatically when the file returns."
    )
}

pub fn remove_heading(name: &str) -> String {
    format!("Remove “{name}” from the library?")
}

// Displays page
pub const ALL_DISPLAYS: &str = "All displays";
pub const ALL_DISPLAYS_SUBTITLE: &str = "The wallpaper used on every display";
pub const WALLPAPER_LABEL: &str = "Wallpaper";
pub const SCALING_LABEL: &str = "Scaling";
pub const NO_WALLPAPER: &str = "None";
pub const SAME_AS_ALL: &str = "Same as all displays";
pub const NO_WALLPAPER_ASSIGNED: &str = "No wallpaper";
pub const NO_DISPLAYS: &str = "No displays were detected.";
pub const PER_DISPLAY_NOTE: &str =
    "Choosing a different wallpaper for each display arrives in a later version.";

pub fn missing_choice(name: &str) -> String {
    format!("{name} (missing)")
}

pub fn inherits(name: &str) -> String {
    format!("{name} (from all displays)")
}

// Settings page
pub const SETTING_AUTOSTART: &str = "Start Lucerna automatically";
pub const SETTING_AUTOSTART_SUB: &str = "Restore your wallpaper when you log in";
pub const SETTING_PAUSE_FULLSCREEN: &str = "Pause when fullscreen";
pub const SETTING_PAUSE_FULLSCREEN_SUB: &str =
    "Pause a display while a fullscreen window covers it";
pub const SETTING_PAUSE_LOCK: &str = "Pause when the screen is locked";
pub const SETTING_HWDEC: &str = "Hardware decoding";
pub const SETTING_HWDEC_SUB: &str = "Try “Disabled” if a wallpaper crashes or flickers";
pub const SETTING_FPS: &str = "FPS limit";
pub const SETTING_FPS_SUB: &str = "Reduces rendering work; decoding effort is unchanged";
pub const SETTING_AUDIO: &str = "Audio";
pub const SETTING_AUDIO_SUB: &str = "Wallpapers are silent unless you turn this on";
pub const SETTINGS_ADVANCED: &str = "Advanced";
pub const SETTING_STACKING: &str = "Window stacking";
pub const SETTING_STACKING_SUB: &str = "Applies after “Reload”. Only change this to troubleshoot.";
pub const AUDIO_CONFIRM_HEADING: &str = "Turn on wallpaper audio?";
pub const AUDIO_CONFIRM_BODY: &str =
    "Wallpapers with sound will play it through your speakers until you turn this off.";
pub const AUDIO_CONFIRM_ACCEPT: &str = "Turn on audio";

// About page
pub const ABOUT_VERSION: &str = "Version";
pub const ABOUT_LICENSE: &str = "License";
pub const ABOUT_REPOSITORY: &str = "Repository";
pub const ABOUT_BACKEND: &str = "Display backend";
pub const BACKEND_NONE: &str = "none (this session is not supported)";
pub const BACKEND_UNKNOWN: &str = "unknown (the service is not running)";

// Header menu and banner
pub const MENU_PAUSE: &str = "Pause";
pub const MENU_RESUME: &str = "Resume";
pub const MENU_RELOAD: &str = "Reload";
pub const MENU_QUIT_SERVICE: &str = "Quit service";
pub const SERVICE_NOT_RUNNING: &str =
    "The Lucerna service is not running, so no wallpaper is being shown.";
pub const SERVICE_STARTING: &str = "Starting the Lucerna service…";
pub const START_SERVICE: &str = "Start";
pub const RELOAD: &str = "Reload";
pub const RETRY: &str = "Try again";
pub const DISMISS: &str = "Dismiss";
pub const MPV_MISSING: &str = "Lucerna could not find mpv, which plays the wallpapers.\n\
Install it (for example: sudo apt install mpv) and choose Reload.";
pub const SERVICE_START_FAILED: &str = "Lucerna could not start its service.\n\
Make sure `lucernad` is installed next to Lucerna or on your PATH, then try again.";

pub fn config_state(code: &str) -> String {
    match code {
        "defaults-after-corruption" => "The configuration file could not be read and was set aside; defaults are in use.".to_owned(),
        "read-only-newer-schema" => "The configuration was written by a newer version of Lucerna, so this version will not change it.".to_owned(),
        "unreadable" => "The configuration file cannot be read, so it will not be overwritten.".to_owned(),
        other => format!("Configuration state: {other}"),
    }
}
