use adw::prelude::*;
use gtk::gio;
use std::rc::Rc;

#[derive(Debug, Default, Clone, Copy)]
pub struct BackgroundController;

impl BackgroundController {
    pub const ACTION_NAMES: [&str; 3] = ["show-settings", "show-history", "quit"];

    pub fn new() -> Self {
        Self
    }

    pub fn install(
        &self,
        app: &adw::Application,
        show_settings_callback: Rc<dyn Fn()>,
        show_history_callback: Rc<dyn Fn()>,
    ) {
        if app.lookup_action(Self::ACTION_NAMES[0]).is_some() {
            return;
        }

        let show_settings = gio::SimpleAction::new(Self::ACTION_NAMES[0], None);
        let show_settings_handler = show_settings_callback.clone();
        show_settings.connect_activate(move |_, _| {
            show_settings_handler();
        });
        app.add_action(&show_settings);

        let show_history = gio::SimpleAction::new(Self::ACTION_NAMES[1], None);
        let show_history_handler = show_history_callback.clone();
        show_history.connect_activate(move |_, _| {
            show_history_handler();
        });
        app.add_action(&show_history);

        let quit = gio::SimpleAction::new(Self::ACTION_NAMES[2], None);
        let quit_app = app.clone();
        quit.connect_activate(move |_, _| {
            quit_app.quit();
        });
        app.add_action(&quit);
    }
}
