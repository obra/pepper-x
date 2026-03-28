use adw::prelude::*;
use gtk::gio;

use crate::window::MainWindow;

#[derive(Debug, Default, Clone, Copy)]
pub struct BackgroundController;

impl BackgroundController {
    pub const ACTION_NAMES: [&str; 3] = ["show-settings", "show-history", "quit"];

    pub fn new() -> Self {
        Self
    }

    pub fn install(&self, app: &adw::Application, window: &MainWindow) {
        if app.lookup_action(Self::ACTION_NAMES[0]).is_some() {
            return;
        }

        let show_settings = gio::SimpleAction::new(Self::ACTION_NAMES[0], None);
        let settings_window = window.clone();
        show_settings.connect_activate(move |_, _| {
            settings_window.present_settings();
        });
        app.add_action(&show_settings);

        let show_history = gio::SimpleAction::new(Self::ACTION_NAMES[1], None);
        let history_window = window.clone();
        show_history.connect_activate(move |_, _| {
            history_window.present_history();
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
