//! The displays page.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;

use super::{boxed_list, control_row, dropdown, page_box, set_labels};
use crate::controller::{AppState, Controller};
use crate::presenter::banner::Link;
use crate::presenter::displays::{
    Choice, DisplayRow, SCALING, rows, scaling_id, scaling_index, selected_index, wallpaper_choices,
};
use crate::strings;

pub struct DisplaysPage {
    pub root: gtk::Box,
    pub all_wallpaper: gtk::DropDown,
    pub all_scaling: gtk::DropDown,
    list: gtk::ListBox,
    empty: gtk::Label,
    choices: RefCell<Vec<Choice>>,
    shown: RefCell<Vec<DisplayRow>>,
    /// True while the page itself is changing a control, so it does not send that as a request.
    updating: Cell<bool>,
}

impl DisplaysPage {
    pub fn new(controller: &Rc<Controller>) -> Rc<Self> {
        let root = page_box();

        let heading = gtk::Label::new(Some(strings::ALL_DISPLAYS));
        heading.set_xalign(0.0);
        heading.add_css_class("title-4");
        root.append(&heading);
        let subtitle = gtk::Label::new(Some(strings::ALL_DISPLAYS_SUBTITLE));
        subtitle.set_xalign(0.0);
        subtitle.add_css_class("dim-label");
        root.append(&subtitle);

        let all_wallpaper = dropdown(&[strings::NO_WALLPAPER]);
        let scaling_labels: Vec<&str> = SCALING.iter().map(|(_, label)| *label).collect();
        let all_scaling = dropdown(&scaling_labels);
        let card = boxed_list();
        card.append(&control_row(strings::WALLPAPER_LABEL, "", &all_wallpaper));
        card.append(&control_row(strings::SCALING_LABEL, "", &all_scaling));
        root.append(&card);

        let each = gtk::Label::new(Some(strings::PAGE_DISPLAYS));
        each.set_xalign(0.0);
        each.add_css_class("title-4");
        each.set_margin_top(12);
        root.append(&each);
        let list = boxed_list();
        root.append(&list);
        let empty = gtk::Label::new(Some(strings::NO_DISPLAYS));
        empty.add_css_class("dim-label");
        root.append(&empty);
        let note = gtk::Label::new(Some(strings::PER_DISPLAY_NOTE));
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.add_css_class("dim-label");
        note.add_css_class("caption");
        root.append(&note);

        let page = Rc::new(Self {
            root,
            all_wallpaper,
            all_scaling,
            list,
            empty,
            choices: RefCell::new(Vec::new()),
            shown: RefCell::new(Vec::new()),
            updating: Cell::new(false),
        });
        page.connect(controller);
        let observed = Rc::clone(&page);
        controller.observe(move |state| observed.render(state));
        page
    }

    fn connect(self: &Rc<Self>, controller: &Rc<Controller>) {
        let (page, controller_wallpaper) = (Rc::clone(self), Rc::clone(controller));
        self.all_wallpaper.connect_selected_notify(move |dropdown| {
            if page.updating.get() {
                return;
            }
            let index = usize::try_from(dropdown.selected()).unwrap_or(0);
            match page.choices.borrow().get(index).and_then(|c| c.id.clone()) {
                Some(id) => controller_wallpaper.play_everywhere(id),
                None => controller_wallpaper.clear_everywhere(),
            }
        });

        let (page, controller_scaling) = (Rc::clone(self), Rc::clone(controller));
        self.all_scaling.connect_selected_notify(move |dropdown| {
            if !page.updating.get() {
                controller_scaling
                    .set_scaling_everywhere(scaling_id(dropdown.selected()).to_owned());
            }
        });
    }

    fn render(&self, state: &AppState) {
        let connected = state.link == Link::Connected;
        self.all_wallpaper.set_sensitive(connected);
        self.all_scaling.set_sensitive(connected);

        self.updating.set(true);
        let choices = wallpaper_choices(&state.wallpapers, true);
        if *self.choices.borrow() != choices {
            let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
            set_labels(&self.all_wallpaper, &labels);
            *self.choices.borrow_mut() = choices;
        }
        let assigned = state
            .displays
            .iter()
            .find(|d| d.connected && d.wallpaper_source == "all")
            .map(|d| d.wallpaper_id.clone())
            .or_else(|| state.displays.first().map(|d| d.wallpaper_id.clone()))
            .unwrap_or_default();
        self.all_wallpaper
            .set_selected(selected_index(&self.choices.borrow(), &assigned));
        let scaling = state
            .displays
            .first()
            .map(|d| d.scaling.as_str())
            .unwrap_or("fill");
        self.all_scaling.set_selected(scaling_index(scaling));
        self.updating.set(false);

        let wanted = rows(&state.displays, &state.wallpapers);
        if *self.shown.borrow() != wanted {
            while let Some(child) = self.list.first_child() {
                self.list.remove(&child);
            }
            for row in &wanted {
                let list_row = control_row(
                    &row.title,
                    &row.subtitle,
                    &gtk::Box::new(gtk::Orientation::Horizontal, 0),
                );
                if !row.connected {
                    list_row.add_css_class("dim-label");
                }
                self.list.append(&list_row);
            }
            *self.shown.borrow_mut() = wanted;
        }
        self.empty.set_visible(self.shown.borrow().is_empty());
        self.list.set_visible(!self.shown.borrow().is_empty());
    }
}
