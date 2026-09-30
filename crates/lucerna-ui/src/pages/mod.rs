//! The four pages of the main window. Widgets only: every decision lives in `presenter` or in
//! the controller.

pub mod about;
pub mod displays;
pub mod settings;
pub mod wallpapers;

use gtk4 as gtk;
use gtk4::prelude::*;

/// The standard container of a page.
pub fn page_box() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_margin_top(18);
    page.set_margin_bottom(18);
    page.set_margin_start(18);
    page.set_margin_end(18);
    page
}

/// A row with text on the left and a control on the right.
pub fn control_row(
    title: &str,
    subtitle: &str,
    control: &impl IsA<gtk::Widget>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.set_margin_top(8);
    line.set_margin_bottom(8);
    line.set_margin_start(12);
    line.set_margin_end(12);

    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.set_hexpand(true);
    texts.set_valign(gtk::Align::Center);
    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_wrap(true);
    texts.append(&title_label);
    if !subtitle.is_empty() {
        let subtitle_label = gtk::Label::new(Some(subtitle));
        subtitle_label.set_xalign(0.0);
        subtitle_label.set_wrap(true);
        subtitle_label.add_css_class("dim-label");
        subtitle_label.add_css_class("caption");
        texts.append(&subtitle_label);
    }
    line.append(&texts);

    control.set_valign(gtk::Align::Center);
    line.append(control);
    row.set_child(Some(&line));
    row
}

/// A list box styled as a card of rows.
pub fn boxed_list() -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    list
}

/// A `DropDown` showing `labels`.
pub fn dropdown(labels: &[&str]) -> gtk::DropDown {
    gtk::DropDown::new(Some(gtk::StringList::new(labels)), gtk::Expression::NONE)
}

/// Replace a drop-down's entries.
pub fn set_labels(dropdown: &gtk::DropDown, labels: &[&str]) {
    dropdown.set_model(Some(&gtk::StringList::new(labels)));
}
