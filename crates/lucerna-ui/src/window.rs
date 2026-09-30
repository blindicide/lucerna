//! The main window: header bar, banner, and the four pages behind a sidebar.

use std::cell::Cell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;

use crate::controller::{AppState, Controller};
use crate::pages::about::AboutPage;
use crate::pages::displays::DisplaysPage;
use crate::pages::settings::SettingsPage;
use crate::pages::wallpapers::WallpapersPage;
use crate::presenter::banner::{Banner, BannerAction, BannerKind, banner};
use crate::strings;

/// Names of the stack pages (also what tests look for).
pub const PAGE_NAMES: [&str; 4] = ["wallpapers", "displays", "settings", "about"];

/// The banner across the top.
struct BannerWidget {
    revealer: gtk::Revealer,
    frame: gtk::Box,
    label: gtk::Label,
    button: gtk::Button,
    action: Cell<Option<BannerAction>>,
}

impl BannerWidget {
    fn new(controller: &Rc<Controller>) -> Rc<Self> {
        let frame = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        frame.add_css_class("lucerna-banner");
        frame.set_margin_top(6);
        frame.set_margin_bottom(6);
        frame.set_margin_start(12);
        frame.set_margin_end(12);
        let label = gtk::Label::new(None);
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_selectable(true);
        let button = gtk::Button::new();
        button.set_valign(gtk::Align::Center);
        frame.append(&label);
        frame.append(&button);
        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
        revealer.set_child(Some(&frame));

        let widget = Rc::new(Self {
            revealer,
            frame,
            label,
            button,
            action: Cell::new(None),
        });
        let clicked = Rc::clone(&widget);
        let controller = Rc::clone(controller);
        widget
            .button
            .connect_clicked(move |_| match clicked.action.get() {
                Some(BannerAction::StartService) => controller.start_service_action(),
                Some(BannerAction::Reload) => controller.reload(),
                Some(BannerAction::Dismiss) => controller.dismiss_error(),
                None => {}
            });
        widget
    }

    fn show(&self, banner: Option<Banner>) {
        let Some(banner) = banner else {
            self.revealer.set_reveal_child(false);
            self.action.set(None);
            return;
        };
        self.label.set_text(&banner.text);
        for class in [
            "lucerna-banner-info",
            "lucerna-banner-warning",
            "lucerna-banner-error",
        ] {
            self.frame.remove_css_class(class);
        }
        self.frame.add_css_class(match banner.kind {
            BannerKind::Info => "lucerna-banner-info",
            BannerKind::Warning => "lucerna-banner-warning",
            BannerKind::Error => "lucerna-banner-error",
        });
        match banner.action {
            Some((action, label)) => {
                self.button.set_label(&label);
                self.button.set_visible(true);
                self.action.set(Some(action));
            }
            None => {
                self.button.set_visible(false);
                self.action.set(None);
            }
        }
        self.revealer.set_reveal_child(true);
    }
}

pub struct MainWindow {
    pub window: gtk::ApplicationWindow,
    pub stack: gtk::Stack,
    pub wallpapers: Rc<WallpapersPage>,
    pub displays: Rc<DisplaysPage>,
    pub settings: Rc<SettingsPage>,
    pub about: Rc<AboutPage>,
    banner: Rc<BannerWidget>,
}

impl MainWindow {
    pub fn new(app: &gtk::Application, controller: &Rc<Controller>) -> Self {
        install_css();

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(strings::WINDOW_TITLE)
            .default_width(900)
            .default_height(600)
            .build();

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        let wallpapers = WallpapersPage::new(controller);
        let displays = DisplaysPage::new(controller);
        let settings = SettingsPage::new(controller);
        let about = AboutPage::new(controller);
        stack.add_titled(
            &wallpapers.root,
            Some(PAGE_NAMES[0]),
            strings::PAGE_WALLPAPERS,
        );
        stack.add_titled(&displays.root, Some(PAGE_NAMES[1]), strings::PAGE_DISPLAYS);
        stack.add_titled(&settings.root, Some(PAGE_NAMES[2]), strings::PAGE_SETTINGS);
        stack.add_titled(&about.root, Some(PAGE_NAMES[3]), strings::PAGE_ABOUT);

        let sidebar = gtk::StackSidebar::new();
        sidebar.set_stack(&stack);
        sidebar.set_vexpand(true);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&sidebar);
        body.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        body.append(&stack);

        let banner_widget = BannerWidget::new(controller);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&banner_widget.revealer);
        content.append(&body);
        window.set_child(Some(&content));

        window.set_titlebar(Some(&header_bar(&window, controller)));

        let shown = Rc::clone(&banner_widget);
        controller.observe(move |state: &AppState| {
            shown.show(banner(
                state.link,
                state.status.as_ref(),
                state.error.as_deref(),
            ));
        });

        Self {
            window,
            stack,
            wallpapers,
            displays,
            settings,
            about,
            banner: banner_widget,
        }
    }

    /// The banner's current text, for tests.
    pub fn banner_text(&self) -> String {
        self.banner.label.text().to_string()
    }

    pub fn banner_visible(&self) -> bool {
        self.banner.revealer.reveals_child()
    }
}

type Handler = fn(&Rc<Controller>);

fn header_bar(window: &gtk::ApplicationWindow, controller: &Rc<Controller>) -> gtk::HeaderBar {
    let actions: [(&str, Handler); 4] = [
        ("pause", Controller::pause),
        ("resume", Controller::resume),
        ("reload", Controller::reload),
        ("quit-service", Controller::quit_service),
    ];
    for (name, handler) in actions {
        let action = gio::SimpleAction::new(name, None);
        let controller = Rc::clone(controller);
        action.connect_activate(move |_, _| handler(&controller));
        window.add_action(&action);
    }

    let menu = gio::Menu::new();
    menu.append(Some(strings::MENU_PAUSE), Some("win.pause"));
    menu.append(Some(strings::MENU_RESUME), Some("win.resume"));
    menu.append(Some(strings::MENU_RELOAD), Some("win.reload"));
    menu.append(Some(strings::MENU_QUIT_SERVICE), Some("win.quit-service"));
    let button = gtk::MenuButton::new();
    button.set_icon_name("open-menu-symbolic");
    button.set_menu_model(Some(&menu));

    let header = gtk::HeaderBar::new();
    header.pack_end(&button);
    header
}

/// Minimal styling for the banner and the "Missing" badge. Colours use named theme colours where
/// the theme defines them; nothing here has been checked visually (see docs/MANUAL-ACCEPTANCE.md).
const CSS: &str = "
.lucerna-banner { padding: 8px 12px; border-radius: 6px; }
.lucerna-banner-info { background-color: alpha(currentColor, 0.08); }
.lucerna-banner-warning { background-color: alpha(orange, 0.25); }
.lucerna-banner-error { background-color: alpha(red, 0.20); }
.lucerna-badge-missing { padding: 2px 8px; border-radius: 8px; background-color: alpha(red, 0.25); font-size: smaller; }
";

fn install_css() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    // `add_provider_for_display` is deprecated in GTK 4.10 in favour of theme-level styling, but
    // it remains the supported way for an application to add its own rules.
    #[allow(deprecated)]
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
