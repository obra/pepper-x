use adw::prelude::*;
use gtk::gio;
use pepper_x_app::startup_policy::StartupLaunchPolicy;

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

pub fn should_present_initial_window(
    startup_launch_policy: StartupLaunchPolicy,
    skipped_initial_background_activation: bool,
) -> bool {
    match startup_launch_policy {
        StartupLaunchPolicy::Interactive => true,
        StartupLaunchPolicy::Background => skipped_initial_background_activation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_interactive_launch_presents_the_initial_window() {
        assert!(should_present_initial_window(
            StartupLaunchPolicy::Interactive,
            false
        ));
    }

    #[test]
    fn background_autostart_skips_only_the_first_initial_window() {
        assert!(!should_present_initial_window(
            StartupLaunchPolicy::Background,
            false
        ));
        assert!(should_present_initial_window(
            StartupLaunchPolicy::Background,
            true
        ));
    }
}
