use adw::prelude::*;
use pepperx_ipc::SharedLiveStatus;
use pepperx_models::{default_cache_root, model_inventory};
use pepperx_platform_gnome::{
    evdev_modifier_capture::{EvdevModifierCaptureHandle, TriggerConfig},
    service::{AppCommand, PepperXService, ServiceHandle},
};
use std::cell::{Cell, RefCell};
use std::io;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::app_model::{initial_surface, AppModel, InitialSurface};
use crate::background::BackgroundController;
use crate::history_store::{ArchivedRun, HistoryStore};
use crate::onboarding::show_onboarding_window;
use crate::session_runtime::LiveRuntimeHandle;
use crate::settings::{AppSettings, AppSetupState};
use crate::startup_policy::startup_launch_policy;
use crate::transcript_log::{state_root, TranscriptEntry};
use crate::transcription::{
    experiment_rerun_archived_cleanup, experiment_rerun_archived_run,
    ArchivedCleanupRerunRequest, ArchivedRunRerunRequest,
};
use crate::status_pill::StatusPill;
use crate::window::{diagnostics_summary_text, MainWindow};

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
    let setup_state = AppSetupState::load_or_default();
    let app = build_application();
    let _background_hold = app.hold();
    let cache_root = default_cache_root();
    let (command_sender, command_receiver) = mpsc::channel();
    let live_status = SharedLiveStatus::new();
    let live_runtime = build_live_runtime(&settings, live_status.clone());
    let live_runtime_for_pump = live_runtime.clone();
    let service_handle = ServiceHandle::start(
        command_sender,
        live_runtime,
        live_status.clone(),
    )
    .expect("failed to start GNOME IPC service");
    let service = service_handle.service();

    // Clean up stale uinput helper socket from a previous run.
    {
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/run/user/1000"));
        let socket = runtime_dir.join("pepper-x").join("uinput-helper.sock");
        let _ = std::fs::remove_file(&socket);
    }

    let modifier_capture = start_modifier_capture(service.clone(), &settings);

    // Pre-warm the cleanup helper: load the model and pre-decode the system
    // prompt in the background so the first recording doesn't pay the ~4s cost.
    {
        let settings = settings.clone();
        std::thread::Builder::new()
            .name("pepperx-cleanup-warmup".into())
            .spawn(move || {
                use pepperx_cleanup::{prefill_cleanup_system_prompt, CleanupRequest};
                use pepperx_models::{
                    catalog_model, chat_template_for_model, default_cache_root, model_readiness,
                };

                if !settings.cleanup_enabled {
                    return;
                }
                let model = match catalog_model(&settings.preferred_cleanup_model) {
                    Some(m) => m,
                    None => return,
                };
                let cache_root = default_cache_root();
                let readiness = model_readiness(model, &cache_root);
                if !readiness.is_ready {
                    return;
                }

                let request = CleanupRequest {
                    transcript_text: String::new(),
                    model_path: readiness.install_path,
                    supporting_context_text: None,
                    ocr_text: None,
                    correction_memory_text: None,
                    prompt_profile: settings.cleanup_prompt_profile.clone(),
                    custom_prompt_text: settings.effective_cleanup_custom_prompt(),
                    chat_template: chat_template_for_model(
                        &settings.preferred_cleanup_model,
                    )
                    .into(),
                };
                prefill_cleanup_system_prompt(&request);
            })
            .ok();
    }
    let shared_trigger_config = modifier_capture
        .as_ref()
        .map(|h| h.shared_config().clone());
    let app_model = Rc::new(AppModel::for_startup(
        &setup_state,
        &settings,
        &service.current_capabilities(),
    ));
    let diagnostics_cache_root = cache_root.clone();
    let diagnostics_app_model = app_model.clone();
    let settings_app_model = app_model.clone();
    let onboarding_window = Rc::new(RefCell::new(None::<adw::ApplicationWindow>));
    let mut window = MainWindow::new_with_providers_and_rerun(
        &app,
        load_history_runs_or_empty,
        move || {
            let settings = AppSettings::load_or_default();
            settings_app_model.settings_surface_state(&settings)
        },
        move || {
            let settings = AppSettings::load_or_default();
            let inventory = model_inventory(&diagnostics_cache_root);
            let history_runs = load_history_runs_or_empty();
            diagnostics_summary_text(
                &settings,
                &diagnostics_cache_root,
                &inventory,
                history_runs.first(),
                &diagnostics_app_model.readiness,
            )
        },
        Some(Rc::new(move |run_id, asr_model_id: String| {
            let settings = AppSettings::load_or_default();
            let request = ArchivedRunRerunRequest {
                run_id,
                asr_model_id: Some(asr_model_id),
                cleanup_enabled: settings.cleanup_enabled,
                cleanup_model_id: settings
                    .cleanup_enabled
                    .then(|| settings.preferred_cleanup_model.clone()),
                cleanup_prompt_profile: settings
                    .cleanup_enabled
                    .then(|| settings.cleanup_prompt_profile.clone()),
            };

            match experiment_rerun_archived_run(request) {
                Ok(entry) => Some(entry),
                Err(error) => {
                    eprintln!("[Pepper X] failed to rerun archived run: {error}");
                    None
                }
            }
        })),
        Some(Rc::new(
            move |run_id, cleanup_model_id: String, custom_prompt: Option<String>| {
                let settings = AppSettings::load_or_default();
                let request = ArchivedCleanupRerunRequest {
                    run_id,
                    cleanup_model_id: Some(cleanup_model_id),
                    cleanup_prompt_profile: Some(settings.cleanup_prompt_profile.clone()),
                    custom_prompt_text: custom_prompt,
                };

                match experiment_rerun_archived_cleanup(request) {
                    Ok(entry) => Some(entry),
                    Err(error) => {
                        eprintln!("[Pepper X] failed to rerun cleanup: {error}");
                        None
                    }
                }
            },
        )),
        Some(Rc::new(|wav_path| {
            std::thread::spawn(move || {
                let result = std::process::Command::new("pw-play")
                    .arg(&wav_path)
                    .status();
                match result {
                    Ok(status) if status.success() => {}
                    Ok(status) => {
                        eprintln!(
                            "[Pepper X] pw-play exited with status {status} for {}",
                            wav_path.display()
                        );
                    }
                    Err(_) => {
                        let result = std::process::Command::new("paplay")
                            .arg(&wav_path)
                            .status();
                        match result {
                            Ok(status) if status.success() => {}
                            Ok(status) => {
                                eprintln!(
                                    "[Pepper X] paplay exited with status {status} for {}",
                                    wav_path.display()
                                );
                            }
                            Err(error) => {
                                eprintln!(
                                    "[Pepper X] failed to play audio {}: {error}",
                                    wav_path.display()
                                );
                            }
                        }
                    }
                }
            });
        })),
    );
    if let Some(config) = shared_trigger_config {
        window.set_shared_trigger_config(config);
    }

    let status_pill = StatusPill::new();

    let startup_launch_policy = startup_launch_policy();
    let skipped_initial_activation = Rc::new(Cell::new(false));
    let app_model = app_model.clone();
    let onboarding_window_handle = onboarding_window.clone();
    install_command_pump(
        app.clone(),
        window.clone(),
        app_model.clone(),
        onboarding_window.clone(),
        live_status.clone(),
        service.clone(),
        command_receiver,
        live_runtime_for_pump,
        status_pill,
    );
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let controller = BackgroundController::new();
        let show_settings = {
            let app = app.clone();
            let window = window.clone();
            let app_model = app_model.clone();
            let onboarding_window = onboarding_window_handle.clone();
            Rc::new(move || {
                present_primary_surface(&app, &window, &app_model, &onboarding_window);
            })
        };
        let show_history = {
            let window = window.clone();
            Rc::new(move || {
                window.present_history();
            })
        };

        controller.install(app, show_settings, show_history);
        match initial_surface(
            startup_launch_policy,
            skipped_initial_activation.replace(true),
            app_model.setup_state(),
        ) {
            Some(InitialSurface::Setup) => {
                present_primary_surface(app, &window, &app_model, &onboarding_window_handle);
            }
            Some(InitialSurface::Settings) => {
                window.present_settings();
            }
            None => {}
        }
    });
    app.run();
}

fn run_headless() {
    let settings = AppSettings::load_or_default();
    let (command_sender, command_receiver) = mpsc::channel::<AppCommand>();
    let live_status = SharedLiveStatus::new();
    let service_handle = ServiceHandle::start(
        command_sender,
        build_live_runtime(&settings, live_status.clone()),
        live_status.clone(),
    )
    .expect("failed to start GNOME IPC service");
    let _modifier_capture = start_modifier_capture(service_handle.service(), &settings);
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

fn load_history_runs_or_empty() -> Vec<ArchivedRun> {
    load_history_runs().unwrap_or_else(|error| {
        eprintln!("[Pepper X] failed to load transcript history: {error}");
        Vec::new()
    })
}

fn build_live_runtime(
    settings: &AppSettings,
    live_status: SharedLiveStatus,
) -> std::sync::Arc<LiveRuntimeHandle> {
    std::sync::Arc::new(LiveRuntimeHandle::new(
        settings.preferred_microphone.clone(),
        live_status,
        settings.play_sounds,
    ))
}

fn start_modifier_capture(
    service: PepperXService,
    settings: &AppSettings,
) -> Option<EvdevModifierCaptureHandle> {
    let hold_config = TriggerConfig::from_setting(&settings.hold_trigger_keys);
    let toggle_config = TriggerConfig::from_setting(&settings.toggle_trigger_keys);
    match EvdevModifierCaptureHandle::start(service.clone(), hold_config, toggle_config) {
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

fn present_primary_surface(
    app: &adw::Application,
    window: &MainWindow,
    app_model: &AppModel,
    onboarding_window: &Rc<RefCell<Option<adw::ApplicationWindow>>>,
) {
    match app_model.requested_surface() {
        InitialSurface::Setup => {
            if let Some(existing_window) = onboarding_window.borrow().as_ref() {
                existing_window.present();
                return;
            }

            let onboarding_window_slot = onboarding_window.clone();
            let onboarding_model = app_model.clone();
            let onboarding = show_onboarding_window(app, app_model, move || {
                onboarding_model.mark_onboarding_completed();
                onboarding_window_slot.borrow_mut().take();
            });
            onboarding.connect_hide({
                let onboarding_window = onboarding_window.clone();
                move |_| {
                    onboarding_window.borrow_mut().take();
                }
            });
            *onboarding_window.borrow_mut() = Some(onboarding);
        }
        InitialSurface::Settings => {
            window.present_settings();
        }
    }
}

fn install_command_pump(
    app: adw::Application,
    window: MainWindow,
    app_model: Rc<AppModel>,
    onboarding_window: Rc<RefCell<Option<adw::ApplicationWindow>>>,
    live_status: SharedLiveStatus,
    service: PepperXService,
    receiver: Receiver<AppCommand>,
    live_runtime: std::sync::Arc<LiveRuntimeHandle>,
    status_pill: StatusPill,
) {
    let last_play_sounds = Cell::new(true);
    gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
        while let Ok(command) = receiver.try_recv() {
            match command {
                AppCommand::ShowSettings => {
                    present_primary_surface(&app, &window, &app_model, &onboarding_window)
                }
                AppCommand::ShowHistory => window.present_history(),
            }
        }

        let capabilities = service.current_capabilities();
        app_model.update_extension_connected(capabilities.extension_connected);
        let snapshot = live_status.snapshot();
        window.set_live_status(&snapshot);
        status_pill.set_live_status(&snapshot);

        // Sync play_sounds setting to runtime (settings toggle saves to disk,
        // we propagate to the live runtime here).
        let settings = AppSettings::load_or_default();
        if settings.play_sounds != last_play_sounds.get() {
            live_runtime.set_play_sounds(settings.play_sounds);
            last_play_sounds.set(settings.play_sounds);
        }

        gtk::glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod app_shell {
    use super::*;
    use crate::app_model::{
        initial_surface, AppModel, InitialSurface, ModelBootstrapSummary, SetupIssue, SetupState,
    };
    use crate::cli::{run_with, StartupMode};
    use crate::history_store::RunRuntimeMetadata;
    use crate::settings::{AppSetupState, RecordingTriggerMode};
    use crate::startup_policy::StartupLaunchPolicy;
    use crate::transcript_log::env_lock;
    use crate::transcript_log::TranscriptEntry;
    use crate::transcription::archive_transcription_result;
    use crate::window::history_summary_text;
    use pepperx_asr::TranscriptionResult;
    use pepperx_ipc::{Capabilities, SERVICE_NAME};
    use std::time::Duration;

    fn archived_run(entry: TranscriptEntry) -> ArchivedRun {
        ArchivedRun {
            run_id: "run-1".into(),
            archived_at_ms: 42,
            run_dir: std::path::PathBuf::from("/tmp/history/run-1"),
            metadata_path: std::path::PathBuf::from("/tmp/history/run-1/run.json"),
            entry,
            runtime_metadata: RunRuntimeMetadata::wav_import(),
            archived_source_wav_path: Some(std::path::PathBuf::from(
                "/tmp/history/run-1/source.wav",
            )),
            parent_run_id: None,
            prompt_profile: None,
            supporting_context_text: None,
            ocr_text: None,
        }
    }

    fn ready_model_bootstrap() -> ModelBootstrapSummary {
        ModelBootstrapSummary::ready()
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
        let show_settings = {
            let window = window.clone();
            Rc::new(move || {
                window.present_settings();
            })
        };
        let show_history = {
            let window = window.clone();
            Rc::new(move || {
                window.present_history();
            })
        };

        controller.install(&app, show_settings, show_history);

        for action_name in BackgroundController::ACTION_NAMES {
            assert!(app.lookup_action(action_name).is_some());
        }
    }

    #[test]
    fn app_shell_settings_include_integration_toggles() {
        let settings = AppSettings::default();
        let setup_state = AppSetupState::default();

        assert!(!setup_state.onboarding_completed);
        assert!(!settings.launch_at_login);
        assert!(settings.enable_gnome_extension_integration);
        assert_eq!(
            settings.preferred_recording_trigger_mode,
            RecordingTriggerMode::ModifierOnly
        );
    }

    #[test]
    fn app_shell_first_run_prefers_setup_surface_over_settings() {
        let settings = AppSettings::default();
        let setup_state = AppSetupState::default();
        let app_model = AppModel::for_startup_with_model_bootstrap(
            &setup_state,
            &settings,
            &Capabilities {
                modifier_only_supported: true,
                extension_connected: false,
                version: "0.1.0".into(),
            },
            ready_model_bootstrap(),
        );

        assert_eq!(
            initial_surface(
                StartupLaunchPolicy::Interactive,
                false,
                app_model.setup_state()
            ),
            Some(InitialSurface::Setup)
        );
        assert_eq!(app_model.setup_title(), "Finish Pepper X setup");
        assert!(app_model
            .setup_description()
            .contains("first-run setup before it can stay in the background"));
    }

    #[test]
    fn app_shell_first_run_blocks_modifier_only_try_it_when_capture_is_unavailable() {
        let settings = AppSettings::default();
        let setup_state = AppSetupState::default();
        let app_model = AppModel::for_startup_with_model_bootstrap(
            &setup_state,
            &settings,
            &Capabilities::shell_default("0.1.0"),
            ready_model_bootstrap(),
        );

        assert_eq!(app_model.setup_state(), SetupState::SetupRequired);
        assert_eq!(app_model.requested_surface(), InitialSurface::Setup);
        assert_eq!(app_model.setup_checklist().completed_items(), 1);
        assert!(!app_model.setup_checklist().trigger_ready);
        assert!(app_model.setup_checklist().asr_ready);
    }

    #[test]
    fn app_shell_completed_setup_keeps_autostart_background_first() {
        let settings = AppSettings::default();
        let setup_state = AppSetupState {
            onboarding_completed: true,
        };
        let app_model = AppModel::for_startup_with_model_bootstrap(
            &setup_state,
            &settings,
            &Capabilities {
                modifier_only_supported: true,
                extension_connected: false,
                version: "0.1.0".into(),
            },
            ready_model_bootstrap(),
        );

        assert_eq!(
            initial_surface(
                StartupLaunchPolicy::Background,
                false,
                app_model.setup_state()
            ),
            None
        );
        assert_eq!(
            initial_surface(
                StartupLaunchPolicy::Background,
                true,
                app_model.setup_state()
            ),
            Some(InitialSurface::Settings)
        );
        assert!(app_model.readiness.modifier_capture_supported);
        assert!(!app_model.readiness.extension_connected.get());
    }

    #[test]
    fn app_shell_modifier_capture_failure_surfaces_recoverable_setup_state() {
        let settings = AppSettings::default();
        let setup_state = AppSetupState {
            onboarding_completed: true,
        };
        let app_model = AppModel::for_startup_with_model_bootstrap(
            &setup_state,
            &settings,
            &Capabilities::shell_default("0.1.0"),
            ready_model_bootstrap(),
        );

        assert_eq!(
            app_model.setup_state(),
            SetupState::NeedsAttention(vec![SetupIssue::ModifierCaptureUnavailable])
        );
        assert_eq!(app_model.setup_title(), "Fix Pepper X setup");
        assert!(app_model
            .setup_description()
            .contains("Modifier-only capture is unavailable"));
    }

    #[test]
    fn app_shell_standard_shortcut_setup_does_not_require_modifier_capture() {
        let settings = AppSettings {
            preferred_recording_trigger_mode: RecordingTriggerMode::StandardShortcut,
            ..AppSettings::default()
        };
        let setup_state = AppSetupState {
            onboarding_completed: true,
        };
        let app_model = AppModel::for_startup_with_model_bootstrap(
            &setup_state,
            &settings,
            &Capabilities::shell_default("0.1.0"),
            ready_model_bootstrap(),
        );

        assert_eq!(app_model.setup_state(), SetupState::Ready);
        assert_eq!(app_model.requested_surface(), InitialSurface::Settings);
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
            "parakeet-rs",
            "nemotron-speech-streaming-en-0.6b",
            Duration::from_millis(42),
        );
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let result = run_with(
            StartupMode::TranscribeWav {
                wav_path: state_root.join("loop1.wav"),
            },
            || unreachable!(),
            |_| unreachable!(),
            |wav_path| {
                archive_transcription_result(TranscriptionResult {
                    wav_path: wav_path.to_path_buf(),
                    transcript_text: "hello from pepper x".into(),
                    backend_name: "parakeet-rs".into(),
                    model_name: "nemotron-speech-streaming-en-0.6b".into(),
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
            "parakeet-rs",
            "nemotron-speech-streaming-en-0.6b",
            Duration::from_millis(84),
        ))]);

        assert!(summary.contains("hello from pepper x"));
        assert!(summary.contains("parakeet-rs"));
        assert!(summary.contains("nemotron-speech-streaming-en-0.6b"));
        assert!(!summary.contains("arrive in a later task"));
    }
}
