use adw::prelude::*;

use crate::background::BackgroundController;
use crate::settings::AppSettings;
use crate::window::MainWindow;

pub const APPLICATION_ID: &str = "com.obra.PepperX";

pub fn build_application() -> adw::Application {
    adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build()
}

pub fn run() {
    adw::init().expect("failed to initialize GTK/libadwaita");

    let _settings = AppSettings::default();
    let app = build_application();
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let window = MainWindow::new(app);
        let controller = BackgroundController::new();

        controller.install(app, &window);
        window.present_settings();
    });
    app.run();
}

#[cfg(test)]
mod app_shell {
    use super::*;
    use crate::settings::RecordingTriggerMode;

    #[test]
    fn app_shell_builds_application_with_stable_id() {
        let app = build_application();

        assert_eq!(app.application_id().as_deref(), Some(APPLICATION_ID));
    }

    #[test]
    fn app_shell_creates_main_window_without_runtime_logic() {
        let app = build_application();
        let window = MainWindow::new(&app);

        assert_eq!(window.application_id().as_deref(), Some(APPLICATION_ID));
    }

    #[test]
    fn app_shell_registers_background_actions() {
        let app = build_application();
        let window = MainWindow::new(&app);
        let controller = BackgroundController::new();

        controller.install(&app, &window);

        for action_name in BackgroundController::ACTION_NAMES {
            assert!(app.lookup_action(action_name).is_some());
        }
    }

    #[test]
    fn app_shell_settings_include_integration_toggles() {
        let settings = AppSettings::default();

        assert!(!settings.launch_at_login);
        assert!(settings.enable_gnome_extension_integration);
        assert_eq!(
            settings.preferred_recording_trigger_mode,
            RecordingTriggerMode::ModifierOnly
        );
    }
}
