use adw::prelude::*;
use pepperx_models::{default_cache_root, model_inventory};
use pepperx_platform_gnome::{
    atspi::ModifierCaptureHandle,
    service::{AppCommand, PepperXService, ServiceHandle},
};
use std::io;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::background::BackgroundController;
use crate::history_store::{ArchivedRun, HistoryStore};
use crate::session_runtime::LiveRuntimeHandle;
use crate::settings::AppSettings;
use crate::transcript_log::{state_root, TranscriptEntry};
use crate::window::{settings_summary_text, MainWindow};

pub const APPLICATION_ID: &str = "com.obra.PepperX";

pub fn build_application() -> adw::Application {
    adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build()
}

pub fn run() {
    if std::env::var("PEPPERX_HEADLESS").as_deref() == Ok("1") {
        run_headless();
        return;
    }

    adw::init().expect("failed to initialize GTK/libadwaita");

    let settings = AppSettings::load_or_default();
    let app = build_application();
    let cache_root = default_cache_root();
    let inventory = model_inventory(&cache_root);
    let window = MainWindow::new_with_history_and_settings(
        &app,
        load_history_runs().unwrap_or_else(|error| {
            eprintln!("[Pepper X] failed to load transcript history: {error}");
            Vec::new()
        }),
        settings_summary_text(&settings, &cache_root, &inventory),
    );
    let (command_sender, command_receiver) = mpsc::channel();
    let service_handle = ServiceHandle::start(command_sender, build_live_runtime(&settings))
        .expect("failed to start GNOME IPC service");
    let _modifier_capture = start_modifier_capture(service_handle.service());

    install_command_pump(window.clone(), command_receiver);
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let controller = BackgroundController::new();

        controller.install(app, &window);
        window.present_settings();
    });
    app.run();
}

fn run_headless() {
    let settings = AppSettings::load_or_default();
    let (command_sender, command_receiver) = mpsc::channel::<AppCommand>();
    let service_handle = ServiceHandle::start(command_sender, build_live_runtime(&settings))
        .expect("failed to start GNOME IPC service");
    let _modifier_capture = start_modifier_capture(service_handle.service());
    let _command_receiver = command_receiver;
    let main_loop = gtk::glib::MainLoop::new(None, false);

    main_loop.run();
}

pub fn load_history_runs() -> io::Result<Vec<ArchivedRun>> {
    HistoryStore::open(state_root())?.recent_runs()
}

pub fn load_history_entries() -> io::Result<Vec<TranscriptEntry>> {
    HistoryStore::open(state_root())?.recent_entries()
}

fn build_live_runtime(settings: &AppSettings) -> std::sync::Arc<LiveRuntimeHandle> {
    std::sync::Arc::new(LiveRuntimeHandle::new(
        settings.preferred_microphone.clone(),
    ))
}

fn start_modifier_capture(service: PepperXService) -> Option<ModifierCaptureHandle> {
    match ModifierCaptureHandle::start(APPLICATION_ID, service.clone()) {
        Ok(handle) => {
            service.set_modifier_only_supported(true);
            Some(handle)
        }
        Err(error) => {
            service.set_modifier_only_supported(false);
            eprintln!("[Pepper X] modifier-only capture unavailable: {error}");
            None
        }
    }
}

fn install_command_pump(window: MainWindow, receiver: Receiver<AppCommand>) {
    gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
        while let Ok(command) = receiver.try_recv() {
            match command {
                AppCommand::ShowSettings => window.present_settings(),
                AppCommand::ShowHistory => window.present_history(),
            }
        }

        gtk::glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod app_shell {
    use super::*;
    use crate::cli::{run_with, StartupMode};
    use crate::settings::RecordingTriggerMode;
    use crate::transcript_log::env_lock;
    use crate::transcript_log::TranscriptEntry;
    use crate::transcription::archive_transcription_result;
    use crate::window::history_summary_text;
    use pepperx_asr::TranscriptionResult;
    use pepperx_ipc::SERVICE_NAME;
    use std::time::Duration;

    fn archived_run(entry: TranscriptEntry) -> ArchivedRun {
        ArchivedRun {
            run_id: "run-1".into(),
            archived_at_ms: 42,
            run_dir: std::path::PathBuf::from("/tmp/history/run-1"),
            metadata_path: std::path::PathBuf::from("/tmp/history/run-1/run.json"),
            entry,
            archived_source_wav_path: Some(std::path::PathBuf::from(
                "/tmp/history/run-1/source.wav",
            )),
            prompt_profile: None,
            supporting_context_text: None,
            ocr_text: None,
        }
    }

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

    #[test]
    fn app_shell_uses_distinct_application_and_ipc_bus_names() {
        assert_ne!(APPLICATION_ID, SERVICE_NAME);
    }

    #[test]
    fn app_shell_loads_history_entries_from_cli_written_pepperx_state_root() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-history-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let expected = TranscriptEntry::new(
            state_root.join("loop1.wav"),
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(42),
        );
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let result = run_with(
            StartupMode::TranscribeWav {
                wav_path: state_root.join("loop1.wav"),
            },
            || unreachable!(),
            |wav_path| {
                archive_transcription_result(TranscriptionResult {
                    wav_path: wav_path.to_path_buf(),
                    transcript_text: "hello from pepper x".into(),
                    backend_name: "sherpa-onnx".into(),
                    model_name: "nemo-parakeet-tdt-0.6b-v2-int8".into(),
                    elapsed_ms: 42,
                })
            },
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
        )
        .unwrap();

        let entries = load_history_entries().unwrap();

        assert_eq!(result, Some(expected.clone()));
        assert_eq!(entries, vec![expected]);
        match previous_state_root {
            Some(previous_state_root) => {
                std::env::set_var("PEPPERX_STATE_ROOT", previous_state_root)
            }
            None => std::env::remove_var("PEPPERX_STATE_ROOT"),
        }
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn app_shell_history_summary_shows_latest_transcript_and_not_placeholder_copy() {
        let summary = history_summary_text(&[archived_run(TranscriptEntry::new(
            "/tmp/loop1.wav",
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(84),
        ))]);

        assert!(summary.contains("hello from pepper x"));
        assert!(summary.contains("sherpa-onnx"));
        assert!(summary.contains("nemo-parakeet-tdt-0.6b-v2-int8"));
        assert!(!summary.contains("arrive in a later task"));
    }
}
