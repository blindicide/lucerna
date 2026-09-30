//! The settings page.

use std::cell::Cell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;
use lucerna_ipc::dto::SettingsPatch;

use super::{boxed_list, control_row, dropdown, page_box};
use crate::controller::{AppState, Controller};
use crate::presenter::banner::Link;
use crate::presenter::settings::{
    FPS_LIMIT, HARDWARE_DECODE, STACKING, index_of, labels, value_at,
};
use crate::strings;

pub struct SettingsPage {
    pub root: gtk::ScrolledWindow,
    pub autostart: gtk::Switch,
    pub pause_fullscreen: gtk::Switch,
    pub pause_lock: gtk::Switch,
    pub audio: gtk::Switch,
    pub hardware_decode: gtk::DropDown,
    pub fps_limit: gtk::DropDown,
    pub stacking: gtk::DropDown,
    updating: Cell<bool>,
}

impl SettingsPage {
    pub fn new(controller: &Rc<Controller>) -> Rc<Self> {
        let autostart = gtk::Switch::new();
        let pause_fullscreen = gtk::Switch::new();
        let pause_lock = gtk::Switch::new();
        let audio = gtk::Switch::new();
        let hardware_decode = dropdown(&labels(HARDWARE_DECODE));
        let fps_limit = dropdown(&labels(FPS_LIMIT));
        let stacking = dropdown(&labels(STACKING));

        let page_content = page_box();
        let card = boxed_list();
        card.append(&control_row(
            strings::SETTING_AUTOSTART,
            strings::SETTING_AUTOSTART_SUB,
            &autostart,
        ));
        card.append(&control_row(
            strings::SETTING_PAUSE_FULLSCREEN,
            strings::SETTING_PAUSE_FULLSCREEN_SUB,
            &pause_fullscreen,
        ));
        card.append(&control_row(strings::SETTING_PAUSE_LOCK, "", &pause_lock));
        card.append(&control_row(
            strings::SETTING_HWDEC,
            strings::SETTING_HWDEC_SUB,
            &hardware_decode,
        ));
        card.append(&control_row(
            strings::SETTING_FPS,
            strings::SETTING_FPS_SUB,
            &fps_limit,
        ));
        card.append(&control_row(
            strings::SETTING_AUDIO,
            strings::SETTING_AUDIO_SUB,
            &audio,
        ));
        page_content.append(&card);

        let advanced = gtk::Expander::new(Some(strings::SETTINGS_ADVANCED));
        let advanced_list = boxed_list();
        advanced_list.set_margin_top(6);
        advanced_list.append(&control_row(
            strings::SETTING_STACKING,
            strings::SETTING_STACKING_SUB,
            &stacking,
        ));
        advanced.set_child(Some(&advanced_list));
        page_content.append(&advanced);

        let root = gtk::ScrolledWindow::new();
        root.set_child(Some(&page_content));

        let page = Rc::new(Self {
            root,
            autostart,
            pause_fullscreen,
            pause_lock,
            audio,
            hardware_decode,
            fps_limit,
            stacking,
            updating: Cell::new(false),
        });
        page.connect(controller);
        let observed = Rc::clone(&page);
        controller.observe(move |state| observed.render(state));
        page
    }

    fn connect(self: &Rc<Self>, controller: &Rc<Controller>) {
        self.on_switch(&self.autostart, controller, |v| SettingsPatch {
            autostart: Some(v),
            ..Default::default()
        });
        self.on_switch(&self.pause_fullscreen, controller, |v| SettingsPatch {
            pause_on_fullscreen: Some(v),
            ..Default::default()
        });
        self.on_switch(&self.pause_lock, controller, |v| SettingsPatch {
            pause_on_lock: Some(v),
            ..Default::default()
        });

        self.on_choice(&self.hardware_decode, controller, HARDWARE_DECODE, |v| {
            SettingsPatch {
                hardware_decode: Some(v),
                ..Default::default()
            }
        });
        self.on_choice(&self.fps_limit, controller, FPS_LIMIT, |v| SettingsPatch {
            fps_limit: Some(v),
            ..Default::default()
        });
        self.on_choice(&self.stacking, controller, STACKING, |v| SettingsPatch {
            stacking: Some(v),
            ..Default::default()
        });

        // Audio must never be enabled silently (§16): turning it on asks first, and the switch
        // only moves once the daemon confirms the change.
        let (page, controller_audio) = (Rc::clone(self), Rc::clone(controller));
        self.audio.connect_active_notify(move |switch| {
            if page.updating.get() {
                return;
            }
            if !switch.is_active() {
                controller_audio.apply_settings(SettingsPatch {
                    audio: Some(false),
                    ..Default::default()
                });
                return;
            }
            page.updating.set(true);
            switch.set_active(false);
            page.updating.set(false);
            let dialog = gtk::AlertDialog::builder()
                .message(strings::AUDIO_CONFIRM_HEADING)
                .detail(strings::AUDIO_CONFIRM_BODY)
                .buttons([strings::CANCEL, strings::AUDIO_CONFIRM_ACCEPT])
                .cancel_button(0)
                .default_button(0)
                .modal(true)
                .build();
            let controller = Rc::clone(&controller_audio);
            let parent = switch.root().and_then(|r| r.downcast::<gtk::Window>().ok());
            dialog.choose(parent.as_ref(), gio::Cancellable::NONE, move |answer| {
                if answer == Ok(1) {
                    controller.apply_settings(SettingsPatch {
                        audio: Some(true),
                        ..Default::default()
                    });
                }
            });
        });
    }

    fn on_switch(
        self: &Rc<Self>,
        switch: &gtk::Switch,
        controller: &Rc<Controller>,
        patch: impl Fn(bool) -> SettingsPatch + 'static,
    ) {
        let (page, controller) = (Rc::clone(self), Rc::clone(controller));
        switch.connect_active_notify(move |switch| {
            if !page.updating.get() {
                controller.apply_settings(patch(switch.is_active()));
            }
        });
    }

    fn on_choice(
        self: &Rc<Self>,
        control: &gtk::DropDown,
        controller: &Rc<Controller>,
        options: &'static [(&'static str, &'static str)],
        patch: impl Fn(String) -> SettingsPatch + 'static,
    ) {
        let (page, controller) = (Rc::clone(self), Rc::clone(controller));
        control.connect_selected_notify(move |control| {
            if !page.updating.get() {
                controller.apply_settings(patch(value_at(options, control.selected()).to_owned()));
            }
        });
    }

    fn render(&self, state: &AppState) {
        let connected = state.link == Link::Connected && state.settings.is_some();
        self.root.set_sensitive(connected);
        let Some(settings) = &state.settings else {
            return;
        };
        self.updating.set(true);
        self.autostart.set_active(settings.autostart);
        self.pause_fullscreen
            .set_active(settings.pause_on_fullscreen);
        self.pause_lock.set_active(settings.pause_on_lock);
        self.audio.set_active(settings.audio);
        self.hardware_decode
            .set_selected(index_of(HARDWARE_DECODE, &settings.hardware_decode));
        self.fps_limit
            .set_selected(index_of(FPS_LIMIT, &settings.fps_limit));
        self.stacking
            .set_selected(index_of(STACKING, &settings.stacking));
        self.updating.set(false);
    }
}
