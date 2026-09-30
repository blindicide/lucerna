//! The GUI's brain: application state, the daemon link, and the operations pages request.
//!
//! Pages never talk to D-Bus. They call the controller and re-render when it publishes new state.
//! Everything asynchronous runs on the glib main context, so no locks are needed.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use futures_util::StreamExt;
use gtk4::glib;
use lucerna_ipc::dto::{DisplayDto, SettingsDto, SettingsPatch, StatusDto, WallpaperDto};
use lucerna_ipc::names::ALL_DISPLAYS;

use crate::link::{DaemonLink, Event, LinkError, find_daemon, spawn_daemon};
use crate::presenter::banner::Link;
use crate::strings;

/// How often the GUI checks that the daemon is still there (signals cover everything else).
const POLL_SECONDS: u32 = 2;
/// How long to wait for a freshly started daemon to claim its bus name.
const STARTUP_WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct AppState {
    pub link: Link,
    pub status: Option<StatusDto>,
    pub wallpapers: Vec<WallpaperDto>,
    pub displays: Vec<DisplayDto>,
    pub settings: Option<SettingsDto>,
    /// A transient error to show in the banner until dismissed.
    pub error: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            link: Link::NotRunning,
            status: None,
            wallpapers: Vec::new(),
            displays: Vec::new(),
            settings: None,
            error: None,
        }
    }
}

type Observer = Box<dyn Fn(&AppState)>;

pub struct Controller {
    address: Option<String>,
    state: RefCell<AppState>,
    link: RefCell<Option<Rc<DaemonLink>>>,
    observers: RefCell<Vec<Observer>>,
    /// Incremented on every (re)connection so a stale event loop can stop itself.
    generation: Cell<u64>,
    auto_start: bool,
}

impl Controller {
    /// `address` connects to a specific bus (tests); `auto_start` launches `lucernad` if absent.
    pub fn new(address: Option<String>, auto_start: bool) -> Rc<Self> {
        Rc::new(Self {
            address,
            state: RefCell::new(AppState::default()),
            link: RefCell::new(None),
            observers: RefCell::new(Vec::new()),
            generation: Cell::new(0),
            auto_start,
        })
    }

    /// Register a view. It is called immediately and after every state change.
    pub fn observe(&self, observer: impl Fn(&AppState) + 'static) {
        observer(&self.state.borrow());
        self.observers.borrow_mut().push(Box::new(observer));
    }

    pub fn snapshot(&self) -> AppState {
        self.state.borrow().clone()
    }

    fn publish(&self) {
        let state = self.state.borrow().clone();
        for observer in self.observers.borrow().iter() {
            observer(&state);
        }
    }

    fn update(&self, change: impl FnOnce(&mut AppState)) {
        change(&mut self.state.borrow_mut());
        self.publish();
    }

    fn link(&self) -> Option<Rc<DaemonLink>> {
        self.link.borrow().clone()
    }

    fn fail(&self, err: &LinkError) {
        match err {
            LinkError::NotRunning => {
                *self.link.borrow_mut() = None;
                self.update(|s| {
                    s.link = Link::NotRunning;
                    s.status = None;
                    // Whatever went wrong just before is moot now; the banner says the service is gone.
                    s.error = None;
                });
            }
            LinkError::Failed(message) => {
                let message = message.clone();
                self.update(|s| s.error = Some(message));
            }
        }
    }

    // ---------------------------------------------------------------------------- connection

    /// Begin: connect (starting the service if needed), then keep in sync.
    pub fn start(self: &Rc<Self>) {
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            this.connect_or_start().await;
        });
        let this = Rc::clone(self);
        glib::timeout_add_seconds_local(POLL_SECONDS, move || {
            let this = Rc::clone(&this);
            glib::spawn_future_local(async move { this.poll().await });
            glib::ControlFlow::Continue
        });
    }

    async fn connect_or_start(self: &Rc<Self>) {
        if self.try_connect().await {
            return;
        }
        if !self.auto_start {
            return;
        }
        self.start_service().await;
    }

    /// Launch `lucernad` and wait for it to claim its bus name (D3).
    pub async fn start_service(self: &Rc<Self>) {
        self.update(|s| {
            s.link = Link::Starting;
            s.error = None;
        });
        if find_daemon().is_none() || spawn_daemon().is_err() {
            self.update(|s| {
                s.link = Link::NotRunning;
                s.error = Some(strings::SERVICE_START_FAILED.to_owned());
            });
            return;
        }
        let steps = STARTUP_WAIT.as_millis() / 100;
        for _ in 0..steps {
            glib::timeout_future(Duration::from_millis(100)).await;
            if self.try_connect().await {
                return;
            }
        }
        self.update(|s| {
            s.link = Link::NotRunning;
            s.error = Some(strings::SERVICE_START_FAILED.to_owned());
        });
    }

    /// One attempt to reach the daemon; on success, load everything and follow its signals.
    async fn try_connect(self: &Rc<Self>) -> bool {
        let Ok(link) = DaemonLink::connect(self.address.as_deref()).await else {
            return false;
        };
        let Ok(status) = link.status().await else {
            return false;
        };
        let link = Rc::new(link);
        *self.link.borrow_mut() = Some(Rc::clone(&link));
        self.update(|s| {
            s.link = Link::Connected;
            s.status = Some(status);
        });
        self.refresh_all().await;
        self.follow_events(link);
        true
    }

    fn follow_events(self: &Rc<Self>, link: Rc<DaemonLink>) {
        self.generation.set(self.generation.get() + 1);
        let generation = self.generation.get();
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            let Ok(mut events) = link.events().await else {
                return;
            };
            while let Some(event) = events.next().await {
                if this.generation.get() != generation {
                    return;
                }
                match event {
                    Event::Status(status) => this.update(|s| s.status = Some(*status)),
                    Event::DisplaysChanged => this.refresh_displays().await,
                    Event::LibraryChanged => {
                        this.refresh_wallpapers().await;
                        this.refresh_displays().await;
                    }
                    Event::SettingsChanged => this.refresh_settings().await,
                }
            }
        });
    }

    async fn poll(self: &Rc<Self>) {
        match self.link() {
            Some(link) => {
                if let Err(err) = link.status().await {
                    self.fail(&err);
                }
            }
            None => {
                if self.snapshot().link == Link::NotRunning {
                    self.try_connect().await;
                }
            }
        }
    }

    // ------------------------------------------------------------------------------ refresh

    pub async fn refresh_all(self: &Rc<Self>) {
        self.refresh_status().await;
        self.refresh_wallpapers().await;
        self.refresh_displays().await;
        self.refresh_settings().await;
    }

    async fn refresh_status(self: &Rc<Self>) {
        if let Some(link) = self.link() {
            match link.status().await {
                Ok(status) => self.update(|s| s.status = Some(status)),
                Err(err) => self.fail(&err),
            }
        }
    }

    async fn refresh_wallpapers(self: &Rc<Self>) {
        if let Some(link) = self.link() {
            match link.wallpapers().await {
                Ok(list) => self.update(|s| s.wallpapers = list),
                Err(err) => self.fail(&err),
            }
        }
    }

    async fn refresh_displays(self: &Rc<Self>) {
        if let Some(link) = self.link() {
            match link.displays().await {
                Ok(list) => self.update(|s| s.displays = list),
                Err(err) => self.fail(&err),
            }
        }
    }

    async fn refresh_settings(self: &Rc<Self>) {
        if let Some(link) = self.link() {
            match link.settings().await {
                Ok(settings) => self.update(|s| s.settings = Some(settings)),
                Err(err) => self.fail(&err),
            }
        }
    }

    // ------------------------------------------------------------------------------ actions

    /// Run one daemon operation; show its error, if any, in the banner.
    fn act<F, Fut>(self: &Rc<Self>, operation: F)
    where
        F: FnOnce(Rc<DaemonLink>) -> Fut + 'static,
        Fut: std::future::Future<Output = Result<(), LinkError>> + 'static,
    {
        let this = Rc::clone(self);
        glib::spawn_future_local(async move {
            let Some(link) = this.link() else {
                this.fail(&LinkError::NotRunning);
                return;
            };
            match operation(link).await {
                Ok(()) => {
                    this.refresh_all().await;
                }
                Err(err) => this.fail(&err),
            }
        });
    }

    pub fn dismiss_error(&self) {
        self.update(|s| s.error = None);
    }

    pub fn add_wallpaper(self: &Rc<Self>, path: PathBuf) {
        self.act(move |link| async move { link.add_wallpaper(&path).await.map(|_| ()) });
    }

    pub fn remove_wallpaper(self: &Rc<Self>, id: String) {
        self.act(move |link| async move { link.remove_wallpaper(&id).await });
    }

    /// Play `id` on every display.
    pub fn play_everywhere(self: &Rc<Self>, id: String) {
        self.assign_wallpaper(ALL_DISPLAYS.to_owned(), Some(id));
    }

    pub fn clear_everywhere(self: &Rc<Self>) {
        self.assign_wallpaper(ALL_DISPLAYS.to_owned(), None);
    }

    /// Assign `id` to `display` (`*` for all displays), or clear the assignment if `None`.
    pub fn assign_wallpaper(self: &Rc<Self>, display: String, id: Option<String>) {
        self.act(move |link| async move {
            match id {
                Some(id) => link.set_wallpaper(&id, &display).await,
                None => link.clear_wallpaper(&display).await,
            }
        });
    }

    pub fn set_scaling_everywhere(self: &Rc<Self>, mode: String) {
        self.set_display_scaling(ALL_DISPLAYS.to_owned(), mode);
    }

    /// `mode` is a scaling mode, or `inherit` for one display.
    pub fn set_display_scaling(self: &Rc<Self>, display: String, mode: String) {
        self.act(move |link| async move { link.set_scaling(&display, &mode).await });
    }

    pub fn apply_settings(self: &Rc<Self>, patch: SettingsPatch) {
        self.act(move |link| async move { link.apply_settings(&patch).await });
    }

    pub fn pause(self: &Rc<Self>) {
        self.act(|link| async move { link.pause().await });
    }

    pub fn resume(self: &Rc<Self>) {
        self.act(|link| async move { link.resume().await });
    }

    pub fn stop_wallpaper(self: &Rc<Self>) {
        self.act(|link| async move { link.stop().await });
    }

    pub fn reload(self: &Rc<Self>) {
        self.act(|link| async move { link.reload().await });
    }

    pub fn quit_service(self: &Rc<Self>) {
        self.act(|link| async move { link.quit().await });
    }

    pub fn start_service_action(self: &Rc<Self>) {
        let this = Rc::clone(self);
        glib::spawn_future_local(async move { this.start_service().await });
    }
}
