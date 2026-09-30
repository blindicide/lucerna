//! The client proxy: async (for the GUI) and blocking (for the CLI), both generated from one trait.

use zbus::proxy;

use crate::dict::Dict;

#[proxy(
    interface = "org.lucerna.Lucerna1",
    default_service = "org.lucerna.Lucerna1",
    default_path = "/org/lucerna/Lucerna1"
)]
pub trait Lucerna {
    fn get_status(&self) -> zbus::Result<Dict>;
    fn get_displays(&self) -> zbus::Result<Vec<Dict>>;
    fn get_assignments(&self) -> zbus::Result<Vec<Dict>>;
    fn list_wallpapers(&self) -> zbus::Result<Vec<Dict>>;
    fn add_wallpaper(&self, path: &str, name: &str) -> zbus::Result<String>;
    fn remove_wallpaper(&self, wallpaper_id: &str) -> zbus::Result<()>;
    fn set_wallpaper(&self, wallpaper_id: &str, display_id: &str) -> zbus::Result<()>;
    fn clear_assignment(&self, display_id: &str) -> zbus::Result<()>;
    fn set_scaling(&self, display_id: &str, mode: &str) -> zbus::Result<()>;
    fn get_settings(&self) -> zbus::Result<Dict>;
    fn set_settings(&self, changes: Dict) -> zbus::Result<()>;
    fn pause(&self) -> zbus::Result<()>;
    fn resume(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;
    fn start(&self) -> zbus::Result<()>;
    fn reload(&self) -> zbus::Result<()>;
    fn quit(&self) -> zbus::Result<()>;
    fn get_diagnostics(&self, redact: bool) -> zbus::Result<String>;

    #[zbus(signal)]
    fn status_changed(&self, status: Dict) -> zbus::Result<()>;
    #[zbus(signal)]
    fn displays_changed(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn library_changed(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn settings_changed(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn renderer_failed(&self, display_id: &str, code: &str, message: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn version(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn api_version(&self) -> zbus::Result<u32>;
}
