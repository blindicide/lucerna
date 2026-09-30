//! The About page.

use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;

use super::page_box;
use crate::controller::{AppState, Controller};
use crate::presenter::about::info;
use crate::strings;

pub struct AboutPage {
    pub root: gtk::Box,
    pub version: gtk::Label,
    pub backend: gtk::Label,
}

impl AboutPage {
    pub fn new(controller: &Rc<Controller>) -> Rc<Self> {
        let details = info(None);
        let root = page_box();
        root.set_valign(gtk::Align::Start);

        let name = gtk::Label::new(Some(details.name));
        name.add_css_class("title-1");
        name.set_xalign(0.0);
        root.append(&name);
        let description = gtk::Label::new(Some(details.description));
        description.set_xalign(0.0);
        root.append(&description);

        let grid = gtk::Grid::new();
        grid.set_column_spacing(18);
        grid.set_row_spacing(6);
        grid.set_margin_top(12);
        let version = gtk::Label::new(Some(&details.version));
        let backend = gtk::Label::new(Some(&details.backend));
        let license = gtk::Label::new(Some(details.license));
        let repository = gtk::LinkButton::with_label(details.repository, details.repository);
        repository.set_halign(gtk::Align::Start);
        for (row, (title, value)) in [
            (
                strings::ABOUT_VERSION,
                version.clone().upcast::<gtk::Widget>(),
            ),
            (strings::ABOUT_LICENSE, license.upcast()),
            (strings::ABOUT_REPOSITORY, repository.upcast()),
            (strings::ABOUT_BACKEND, backend.clone().upcast()),
        ]
        .into_iter()
        .enumerate()
        {
            let row = i32::try_from(row).unwrap_or(0);
            let label = gtk::Label::new(Some(title));
            label.set_xalign(1.0);
            label.add_css_class("dim-label");
            grid.attach(&label, 0, row, 1, 1);
            value.set_halign(gtk::Align::Start);
            grid.attach(&value, 1, row, 1, 1);
        }
        root.append(&grid);

        let page = Rc::new(Self {
            root,
            version,
            backend,
        });
        let observed = Rc::clone(&page);
        controller.observe(move |state| observed.render(state));
        page
    }

    fn render(&self, state: &AppState) {
        let details = info(state.status.as_ref());
        self.version.set_text(&details.version);
        self.backend.set_text(&details.backend);
    }
}
