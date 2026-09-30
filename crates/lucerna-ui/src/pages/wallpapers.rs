//! The wallpaper library page.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;

use super::page_box;
use crate::controller::{AppState, Controller};
use crate::presenter::banner::Link;
use crate::presenter::wallpapers::{MIME_TYPES, SUFFIXES, WallpaperRow, remove_confirmation, rows};
use crate::strings;

pub struct WallpapersPage {
    pub root: gtk::Box,
    pub list: gtk::ListBox,
    pub add: gtk::Button,
    pub remove: gtk::Button,
    pub play: gtk::Button,
    pub stop: gtk::Button,
    empty: gtk::Label,
    shown: RefCell<Vec<WallpaperRow>>,
}

impl WallpapersPage {
    pub fn new(controller: &Rc<Controller>) -> Rc<Self> {
        let root = page_box();
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let add = gtk::Button::with_label(strings::ADD_WALLPAPER);
        add.add_css_class("suggested-action");
        let play = gtk::Button::with_label(strings::SET_ON_ALL);
        let stop = gtk::Button::with_label(strings::STOP_WALLPAPER);
        let remove = gtk::Button::with_label(strings::REMOVE_WALLPAPER);
        remove.add_css_class("destructive-action");
        remove.set_halign(gtk::Align::End);
        remove.set_hexpand(true);
        for button in [&add, &play, &stop, &remove] {
            toolbar.append(button);
        }
        root.append(&toolbar);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("boxed-list");
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_vexpand(true);
        scroller.set_child(Some(&list));
        root.append(&scroller);

        let empty = gtk::Label::new(Some(strings::LIBRARY_EMPTY));
        empty.set_wrap(true);
        empty.add_css_class("dim-label");
        root.append(&empty);

        let page = Rc::new(Self {
            root,
            list,
            add,
            remove,
            play,
            stop,
            empty,
            shown: RefCell::new(Vec::new()),
        });
        page.connect(controller);
        let observed = Rc::clone(&page);
        controller.observe(move |state| observed.render(state));
        page
    }

    fn selected(&self) -> Option<WallpaperRow> {
        let index = usize::try_from(self.list.selected_row()?.index()).ok()?;
        self.shown.borrow().get(index).cloned()
    }

    fn connect(self: &Rc<Self>, controller: &Rc<Controller>) {
        let page = Rc::clone(self);
        self.list
            .connect_row_selected(move |_, _| page.update_sensitivity());

        let controller_add = Rc::clone(controller);
        self.add.connect_clicked(move |button| {
            let dialog = file_dialog();
            let controller = Rc::clone(&controller_add);
            let parent = button.root().and_then(|r| r.downcast::<gtk::Window>().ok());
            dialog.open(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                if let Some(path) = result.ok().and_then(|file| file.path()) {
                    controller.add_wallpaper(path);
                }
            });
        });

        let (page, controller_play) = (Rc::clone(self), Rc::clone(controller));
        self.play.connect_clicked(move |_| {
            if let Some(row) = page.selected() {
                controller_play.play_everywhere(row.id);
            }
        });

        let controller_stop = Rc::clone(controller);
        self.stop
            .connect_clicked(move |_| controller_stop.stop_wallpaper());

        let (page, controller_remove) = (Rc::clone(self), Rc::clone(controller));
        self.remove.connect_clicked(move |button| {
            let Some(row) = page.selected() else { return };
            let (heading, body) = remove_confirmation(&row);
            let dialog = gtk::AlertDialog::builder()
                .message(heading)
                .detail(body)
                .buttons([strings::CANCEL, strings::REMOVE])
                .cancel_button(0)
                .default_button(0)
                .modal(true)
                .build();
            let controller = Rc::clone(&controller_remove);
            let parent = button.root().and_then(|r| r.downcast::<gtk::Window>().ok());
            dialog.choose(parent.as_ref(), gio::Cancellable::NONE, move |answer| {
                if answer == Ok(1) {
                    controller.remove_wallpaper(row.id.clone());
                }
            });
        });
    }

    fn update_sensitivity(&self) {
        let has_selection = self.list.selected_row().is_some();
        self.remove.set_sensitive(has_selection);
        self.play.set_sensitive(has_selection);
    }

    fn render(&self, state: &AppState) {
        let connected = state.link == Link::Connected;
        for button in [&self.add, &self.stop] {
            button.set_sensitive(connected);
        }

        let wanted = rows(&state.wallpapers);
        if *self.shown.borrow() != wanted {
            let keep = self.selected().map(|r| r.id);
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            for row in &wanted {
                self.list.append(&build_row(row));
            }
            *self.shown.borrow_mut() = wanted;
            if let Some(id) = keep {
                let index = self.shown.borrow().iter().position(|r| r.id == id);
                if let Some(row) = index
                    .and_then(|i| i32::try_from(i).ok())
                    .and_then(|i| self.list.row_at_index(i))
                {
                    self.list.select_row(Some(&row));
                }
            }
        }
        self.empty.set_visible(self.shown.borrow().is_empty());
        self.update_sensitivity();
    }
}

fn build_row(row: &WallpaperRow) -> gtk::ListBoxRow {
    let list_row = gtk::ListBoxRow::new();
    list_row.set_tooltip_text(Some(&row.tooltip));
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.set_margin_top(8);
    line.set_margin_bottom(8);
    line.set_margin_start(12);
    line.set_margin_end(12);

    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    let title = gtk::Label::new(Some(&row.title));
    title.set_xalign(0.0);
    title.add_css_class("heading");
    let subtitle = gtk::Label::new(Some(&row.subtitle));
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    subtitle.add_css_class("dim-label");
    subtitle.add_css_class("caption");
    texts.append(&title);
    texts.append(&subtitle);
    line.append(&texts);

    if row.missing {
        let badge = gtk::Label::new(Some(strings::MISSING_BADGE));
        badge.add_css_class("lucerna-badge-missing");
        badge.set_valign(gtk::Align::Center);
        line.append(&badge);
    }
    list_row.set_child(Some(&line));
    list_row
}

/// The "Add a wallpaper" chooser, filtered to videos and animated images (with an "All files" choice).
fn file_dialog() -> gtk::FileDialog {
    let media = gtk::FileFilter::new();
    media.set_name(Some(strings::FILTER_MEDIA));
    for mime in MIME_TYPES {
        media.add_mime_type(mime);
    }
    for suffix in SUFFIXES {
        media.add_suffix(suffix);
    }
    let all = gtk::FileFilter::new();
    all.set_name(Some(strings::FILTER_ALL));
    all.add_pattern("*");

    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&media);
    filters.append(&all);
    gtk::FileDialog::builder()
        .title(strings::FILE_DIALOG_TITLE)
        .filters(&filters)
        .default_filter(&media)
        .modal(true)
        .build()
}
