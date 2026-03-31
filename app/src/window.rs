use adw::prelude::*;
use gtk::Orientation;
use pepperx_ipc::LiveStatus;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use crate::app_model::{RuntimeReadinessSummary, SettingsSurfaceState};
use crate::diagnostics_view::DiagnosticsView;
use crate::history_view::build_history_browser;
use crate::overlay::OverlayView;
use crate::settings_view::SettingsView;
use pepperx_models::{ModelInventoryEntry, ModelKind};

use crate::history_store::ArchivedRun;
use crate::settings::AppSettings;

const SETTINGS_PAGE_NAME: &str = "settings";
const HISTORY_PAGE_NAME: &str = "history";
const DIAGNOSTICS_PAGE_NAME: &str = "diagnostics";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageScaffoldKind {
    Form,
    Browser,
    CardList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowPage {
    Settings,
    History,
    Diagnostics,
}

impl WindowPage {
    fn page_name(self) -> &'static str {
        match self {
            Self::Settings => SETTINGS_PAGE_NAME,
            Self::History => HISTORY_PAGE_NAME,
            Self::Diagnostics => DIAGNOSTICS_PAGE_NAME,
        }
    }

    fn container_kind(self) -> PageScaffoldKind {
        match self {
            Self::Settings => PageScaffoldKind::Form,
            Self::History => PageScaffoldKind::Browser,
            Self::Diagnostics => PageScaffoldKind::CardList,
        }
    }
}

struct WindowContentProviders {
    history_runs: Rc<dyn Fn() -> Vec<ArchivedRun>>,
    settings_surface_state: Rc<dyn Fn() -> SettingsSurfaceState>,
    diagnostics_summary: Rc<dyn Fn() -> String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowContentSnapshot {
    history_runs: Vec<ArchivedRun>,
    settings_surface_state: SettingsSurfaceState,
    diagnostics_summary: String,
}

#[derive(Clone)]
pub struct MainWindow {
    app: adw::Application,
    content_providers: Rc<WindowContentProviders>,
    rerun_archived_run: Option<Rc<dyn Fn(String)>>,
    live_status: Rc<RefCell<LiveStatus>>,
    state: Rc<RefCell<Option<WindowState>>>,
}

struct WindowState {
    window: adw::ApplicationWindow,
    overlay_view: OverlayView,
    shell_stack: gtk::Stack,
    settings_view: SettingsView,
    diagnostics_view: DiagnosticsView,
    history_container: gtk::Box,
}

impl WindowContentProviders {
    fn snapshot(&self) -> WindowContentSnapshot {
        WindowContentSnapshot {
            history_runs: (self.history_runs)(),
            settings_surface_state: (self.settings_surface_state)(),
            diagnostics_summary: (self.diagnostics_summary)(),
        }
    }
}

impl MainWindow {
    #[cfg(test)]
    pub fn new(app: &adw::Application) -> Self {
        Self::new_with_history_and_settings(
            app,
            Vec::new(),
            default_settings_surface_state(),
            default_diagnostics_summary(),
        )
    }

    pub fn new_with_history_and_settings(
        app: &adw::Application,
        history_runs: Vec<ArchivedRun>,
        settings_surface_state: SettingsSurfaceState,
        diagnostics_summary: String,
    ) -> Self {
        let history_runs = Rc::new(history_runs);
        let settings_surface_state = Rc::new(settings_surface_state);
        let diagnostics_summary = Rc::new(diagnostics_summary);

        Self::new_with_providers(
            app,
            {
                let history_runs = history_runs.clone();
                move || history_runs.as_ref().clone()
            },
            {
                let settings_surface_state = settings_surface_state.clone();
                move || settings_surface_state.as_ref().clone()
            },
            {
                let diagnostics_summary = diagnostics_summary.clone();
                move || diagnostics_summary.as_ref().clone()
            },
        )
    }

    pub(crate) fn new_with_providers<H, S, D>(
        app: &adw::Application,
        history_runs: H,
        settings_surface_state: S,
        diagnostics_summary: D,
    ) -> Self
    where
        H: Fn() -> Vec<ArchivedRun> + 'static,
        S: Fn() -> SettingsSurfaceState + 'static,
        D: Fn() -> String + 'static,
    {
        Self {
            app: app.clone(),
            content_providers: Rc::new(WindowContentProviders {
                history_runs: Rc::new(history_runs),
                settings_surface_state: Rc::new(settings_surface_state),
                diagnostics_summary: Rc::new(diagnostics_summary),
            }),
            rerun_archived_run: None,
            live_status: Rc::new(RefCell::new(LiveStatus::ready())),
            state: Rc::new(RefCell::new(None)),
        }
    }

    pub(crate) fn new_with_providers_and_rerun<H, S, D>(
        app: &adw::Application,
        history_runs: H,
        settings_surface_state: S,
        diagnostics_summary: D,
        rerun_archived_run: Option<Rc<dyn Fn(String)>>,
    ) -> Self
    where
        H: Fn() -> Vec<ArchivedRun> + 'static,
        S: Fn() -> SettingsSurfaceState + 'static,
        D: Fn() -> String + 'static,
    {
        let mut window = Self::new_with_providers(
            app,
            history_runs,
            settings_surface_state,
            diagnostics_summary,
        );
        window.rerun_archived_run = rerun_archived_run;
        window
    }

    #[cfg(test)]
    pub fn application_id(&self) -> Option<String> {
        self.app.application_id().map(|id| id.to_string())
    }

    pub fn present_settings(&self) {
        self.present_page(WindowPage::Settings);
    }

    pub fn present_history(&self) {
        self.present_page(WindowPage::History);
    }

    pub fn set_live_status(&self, status: &LiveStatus) {
        *self.live_status.borrow_mut() = status.clone();

        if let Some(state) = self.state.borrow().as_ref() {
            state.overlay_view.set_live_status(status);
        }
    }

    fn current_content_snapshot(&self) -> WindowContentSnapshot {
        self.content_providers.snapshot()
    }

    fn present_page(&self, page: WindowPage) {
        self.ensure_window();
        self.refresh_content();

        if let Some(state) = self.state.borrow().as_ref() {
            state.shell_stack.set_visible_child_name(page.page_name());
            state.window.present();
        }
    }

    fn ensure_window(&self) {
        if self.state.borrow().is_some() {
            return;
        }

        let snapshot = self.current_content_snapshot();
        let shell_stack = gtk::Stack::builder()
            .hexpand(true)
            .vexpand(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        let overlay_view = OverlayView::new();
        overlay_view.set_live_status(&self.live_status.borrow());
        let settings_view = SettingsView::new(snapshot.settings_surface_state.clone());
        shell_stack.add_titled(settings_view.widget(), Some(SETTINGS_PAGE_NAME), "Settings");
        let history_container = gtk::Box::new(Orientation::Vertical, 0);
        history_container.set_hexpand(true);
        history_container.set_vexpand(true);
        history_container.append(&build_history_browser(
            &snapshot.history_runs,
            self.rerun_archived_run.clone(),
        ));
        shell_stack.add_titled(&history_container, Some(HISTORY_PAGE_NAME), "History");
        let diagnostics_view = DiagnosticsView::new(snapshot.diagnostics_summary.as_str());
        shell_stack.add_titled(
            diagnostics_view.widget(),
            Some(DIAGNOSTICS_PAGE_NAME),
            "Diagnostics",
        );

        let stack_switcher = gtk::StackSwitcher::new();
        stack_switcher.set_stack(Some(&shell_stack));

        let header_bar = adw::HeaderBar::builder()
            .title_widget(&adw::WindowTitle::new(
                "Pepper X",
                "GNOME-first local dictation shell",
            ))
            .build();
        header_bar.pack_start(&stack_switcher);

        let view = adw::ToolbarView::new();
        view.add_top_bar(&header_bar);
        let content = gtk::Box::new(Orientation::Vertical, 0);
        content.append(overlay_view.widget());
        content.append(&shell_stack);
        view.set_content(Some(&content));

        let window = adw::ApplicationWindow::builder()
            .application(&self.app)
            .title("Pepper X")
            .default_width(720)
            .default_height(480)
            .content(&view)
            .build();

        *self.state.borrow_mut() = Some(WindowState {
            window,
            overlay_view,
            shell_stack,
            settings_view,
            diagnostics_view,
            history_container,
        });
    }

    fn refresh_content(&self) {
        let snapshot = self.current_content_snapshot();
        let state = self.state.borrow();
        let Some(state) = state.as_ref() else {
            return;
        };

        state
            .settings_view
            .set_surface_state(&snapshot.settings_surface_state);
        state
            .diagnostics_view
            .set_summary(&snapshot.diagnostics_summary);
        replace_history_browser(
            &state.history_container,
            &snapshot.history_runs,
            self.rerun_archived_run.clone(),
        );
    }
}

pub(crate) fn settings_summary_text(
    settings: &AppSettings,
    cache_root: &Path,
    inventory: &[ModelInventoryEntry],
) -> String {
    let mut lines = vec![
        format!("Model cache: {}", cache_root.display()),
        format!("Default ASR model: {}", settings.preferred_asr_model),
        format!(
            "Default cleanup model: {}",
            settings.preferred_cleanup_model
        ),
        format!(
            "Cleanup prompt profile: {}",
            settings.cleanup_prompt_profile
        ),
    ];

    for entry in inventory {
        let kind_label = match entry.kind {
            ModelKind::Asr => "ASR",
            ModelKind::Cleanup => "Cleanup",
        };
        let status = if entry.readiness.is_ready {
            "ready".to_string()
        } else {
            format!("missing {}", entry.readiness.missing_files.join(", "))
        };
        lines.push(format!("{kind_label} model {}: {status}", entry.id));
    }

    lines.join("\n")
}

pub(crate) fn diagnostics_summary_text(
    settings: &AppSettings,
    cache_root: &Path,
    inventory: &[ModelInventoryEntry],
    latest_run: Option<&ArchivedRun>,
    readiness: &RuntimeReadinessSummary,
) -> String {
    let mut lines = vec![
        format!("Model cache root: {}", cache_root.display()),
        format!("Selected ASR model: {}", settings.preferred_asr_model),
        format!(
            "Selected cleanup model: {}",
            settings.preferred_cleanup_model
        ),
        format!(
            "Active cleanup prompt profile: {}",
            settings.cleanup_prompt_profile
        ),
        format!(
            "Modifier-only capture supported: {}",
            readiness.modifier_capture_supported
        ),
        format!("Extension connected: {}", readiness.extension_connected),
        format!("Service version: {}", readiness.service_version),
    ];

    for entry in inventory {
        let kind_label = match entry.kind {
            ModelKind::Asr => "ASR",
            ModelKind::Cleanup => "Cleanup",
        };
        let status = if entry.readiness.is_ready {
            "ready".to_string()
        } else {
            format!("missing {}", entry.readiness.missing_files.join(", "))
        };
        lines.push(format!(
            "{kind_label} install: {} ({status})",
            entry.readiness.install_path.display()
        ));
    }

    if let Some(run) = latest_run {
        lines.push(String::new());
        lines.push(format!("Latest run ID: {}", run.run_id));
        lines.push(format!("ASR time: {} ms", run.entry.elapsed_ms));

        if let Some(cleanup) = run.entry.cleanup.as_ref() {
            lines.push(format!("Cleanup time: {} ms", cleanup.elapsed_ms));
            lines.push(format!("OCR used: {}", cleanup.used_ocr));
            if let Some(reason) = cleanup.failure_reason.as_deref() {
                lines.push(format!("Cleanup failure: {reason}"));
            }
        }

        if let Some(insertion) = run.entry.insertion.as_ref() {
            lines.push(format!("Insertion backend: {}", insertion.backend_name));
            lines.push(format!(
                "Insertion target: {}",
                insertion.target_application_name
            ));
            if let Some(reason) = insertion.failure_reason.as_deref() {
                lines.push(format!("Insertion failure: {reason}"));
            }
        }
    }

    lines.join("\n")
}

pub(crate) fn history_summary_text(runs: &[ArchivedRun]) -> String {
    if let Some(latest) = runs.first() {
        let entry = &latest.entry;
        let mut summary = format!(
            "Raw transcript:\n{}\n\nSource WAV: {}\nBackend: {}\nModel: {}\nElapsed: {} ms\nArchived entries: {}",
            entry.transcript_text,
            entry.source_wav_path.display(),
            entry.backend_name,
            entry.model_name,
            entry.elapsed_ms,
            runs.len()
        );

        if let Some(prompt_profile) = latest.prompt_profile.as_deref() {
            summary.push_str(&format!("\nCleanup prompt profile: {prompt_profile}"));
        }

        if let Some(cleanup) = entry.cleanup.as_ref() {
            if let Some(cleaned_text) = cleanup.cleaned_text() {
                summary.push_str(&format!(
                    "\n\nCleaned transcript:\n{cleaned_text}\nCleanup backend: {}\nCleanup model: {}",
                    cleanup.backend_name, cleanup.model_name
                ));
            } else {
                summary.push_str(&format!(
                    "\n\nCleanup: failed via {}\nReason: {}",
                    cleanup.backend_name,
                    cleanup
                        .failure_reason
                        .as_deref()
                        .unwrap_or("unknown cleanup failure")
                ));
            }
        }

        if let Some(insertion) = entry.insertion.as_ref() {
            if let Some(target_class) = insertion.target_class.as_deref() {
                summary.push_str(&format!("\nTarget class: {target_class}"));
            }
            let insertion_summary = if insertion.succeeded {
                format!(
                    "\nFriendly insertion: inserted into {} via {}",
                    insertion.target_application_name, insertion.backend_name
                )
            } else {
                format!(
                    "\nFriendly insertion: failed in {} via {}\nReason: {}",
                    insertion.target_application_name,
                    insertion.backend_name,
                    insertion
                        .failure_reason
                        .as_deref()
                        .unwrap_or("unknown failure")
                )
            };
            summary.push_str(&insertion_summary);
        }

        if let Some(learning) = entry.learning.as_ref() {
            let action_label = match learning.action.as_str() {
                "prompt-memory" => "Correction memory updated",
                action => action,
            };
            summary.push_str(&format!(
                "\n{action_label}\n{} -> {}",
                learning.source_text, learning.replacement_text
            ));
        }

        summary
    } else {
        "No dictation runs yet. Run `pepper-x --transcribe-wav <path>` or `pepper-x --transcribe-wav-and-insert-friendly <path>` to archive a transcript."
            .to_string()
    }
}

#[cfg(test)]
fn default_settings_surface_state() -> SettingsSurfaceState {
    SettingsSurfaceState::from_settings(&AppSettings::default())
}

#[cfg(test)]
fn default_diagnostics_summary() -> String {
    String::from("Pepper X runtime diagnostics surface lives here.")
}

fn replace_history_browser(
    container: &gtk::Box,
    runs: &[ArchivedRun],
    rerun_archived_run: Option<Rc<dyn Fn(String)>>,
) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    container.append(&build_history_browser(runs, rerun_archived_run));
}

#[cfg(test)]
mod app_shell {
    use super::*;
    use crate::app_model::SettingsSurfaceState;
    use crate::diagnostics_view::{diagnostics_page_scaffold, DiagnosticsContainerKind};
    use crate::history_store::RunRuntimeMetadata;
    use crate::settings::AppSettings;
    use crate::settings_view::{
        settings_page_scaffold, SettingsContainerKind, SettingsControl, SettingsSelectControl,
        SettingsSwitchControl, SettingsTextAreaControl,
    };
    use crate::transcript_log::{InsertionDiagnostics, LearningDiagnostics, TranscriptEntry};
    use pepperx_ipc::Capabilities;
    use pepperx_models::{ModelInventoryEntry, ModelKind, ModelReadiness};
    use std::path::PathBuf;
    use std::time::Duration;

    fn archived_run(entry: TranscriptEntry) -> ArchivedRun {
        ArchivedRun {
            run_id: "run-1".into(),
            archived_at_ms: 42,
            run_dir: PathBuf::from("/tmp/history/run-1"),
            metadata_path: PathBuf::from("/tmp/history/run-1/run.json"),
            entry,
            runtime_metadata: RunRuntimeMetadata::wav_import(),
            archived_source_wav_path: Some(PathBuf::from("/tmp/history/run-1/source.wav")),
            parent_run_id: None,
            prompt_profile: None,
            supporting_context_text: None,
            ocr_text: None,
        }
    }

    #[test]
    fn model_status_settings_summary_shows_cache_root_and_selected_models() {
        let settings = AppSettings {
            preferred_asr_model: "nemo-parakeet-tdt-0.6b-v2-int8".into(),
            preferred_cleanup_model: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
            cleanup_prompt_profile: "ordinary-dictation".into(),
            ..AppSettings::default()
        };
        let cache_root = PathBuf::from("/tmp/pepper-x-models");
        let summary = settings_summary_text(
            &settings,
            &cache_root,
            &[
                ModelInventoryEntry {
                    id: "nemo-parakeet-tdt-0.6b-v2-int8".into(),
                    kind: ModelKind::Asr,
                    readiness: ModelReadiness {
                        install_path: cache_root.join("asr/nemo-parakeet-tdt-0.6b-v2-int8"),
                        is_ready: true,
                        missing_files: Vec::new(),
                    },
                },
                ModelInventoryEntry {
                    id: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    kind: ModelKind::Cleanup,
                    readiness: ModelReadiness {
                        install_path: cache_root.join("cleanup/qwen2.5-3b-instruct-q4_k_m.gguf"),
                        is_ready: false,
                        missing_files: vec!["qwen2.5-3b-instruct-q4_k_m.gguf".into()],
                    },
                },
            ],
        );

        assert!(summary.contains("Model cache: /tmp/pepper-x-models"));
        assert!(summary.contains("Default ASR model: nemo-parakeet-tdt-0.6b-v2-int8"));
        assert!(summary.contains("Default cleanup model: qwen2.5-3b-instruct-q4_k_m.gguf"));
        assert!(summary.contains("Cleanup prompt profile: ordinary-dictation"));
        assert!(summary.contains("ASR model nemo-parakeet-tdt-0.6b-v2-int8: ready"));
        assert!(summary.contains(
            "Cleanup model qwen2.5-3b-instruct-q4_k_m.gguf: missing qwen2.5-3b-instruct-q4_k_m.gguf"
        ));
    }

    #[test]
    fn diagnostics_summary_shows_model_readiness_cache_paths_and_capabilities() {
        let settings = AppSettings {
            preferred_asr_model: "nemo-parakeet-tdt-0.6b-v2-int8".into(),
            preferred_cleanup_model: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
            cleanup_prompt_profile: "ordinary-dictation".into(),
            ..AppSettings::default()
        };
        let cache_root = PathBuf::from("/tmp/pepper-x-models");
        let summary = diagnostics_summary_text(
            &settings,
            &cache_root,
            &[
                ModelInventoryEntry {
                    id: "nemo-parakeet-tdt-0.6b-v2-int8".into(),
                    kind: ModelKind::Asr,
                    readiness: ModelReadiness {
                        install_path: cache_root.join("asr/nemo-parakeet-tdt-0.6b-v2-int8"),
                        is_ready: true,
                        missing_files: Vec::new(),
                    },
                },
                ModelInventoryEntry {
                    id: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    kind: ModelKind::Cleanup,
                    readiness: ModelReadiness {
                        install_path: cache_root.join("cleanup/qwen2.5-3b-instruct-q4_k_m.gguf"),
                        is_ready: false,
                        missing_files: vec!["qwen2.5-3b-instruct-q4_k_m.gguf".into()],
                    },
                },
            ],
            None,
            &RuntimeReadinessSummary {
                modifier_capture_supported: true,
                extension_connected: false,
                service_version: "0.1.0".into(),
            },
        );

        assert!(summary.contains("Model cache root: /tmp/pepper-x-models"));
        assert!(summary.contains("Selected ASR model: nemo-parakeet-tdt-0.6b-v2-int8"));
        assert!(summary.contains("Selected cleanup model: qwen2.5-3b-instruct-q4_k_m.gguf"));
        assert!(summary.contains("Active cleanup prompt profile: ordinary-dictation"));
        assert!(summary.contains(
            "ASR install: /tmp/pepper-x-models/asr/nemo-parakeet-tdt-0.6b-v2-int8 (ready)"
        ));
        assert!(summary.contains("Cleanup install: /tmp/pepper-x-models/cleanup/qwen2.5-3b-instruct-q4_k_m.gguf (missing qwen2.5-3b-instruct-q4_k_m.gguf)"));
        assert!(summary.contains("Modifier-only capture supported: true"));
        assert!(summary.contains("Extension connected: false"));
    }

    #[test]
    fn diagnostics_summary_shows_latest_run_timings_ocr_usage_and_failure_reasons() {
        let settings = AppSettings::default();
        let cache_root = PathBuf::from("/tmp/pepper-x-models");
        let mut entry = TranscriptEntry::new(
            "/tmp/loop5.wav",
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(21),
        );
        let mut cleanup = crate::transcript_log::CleanupDiagnostics::failed(
            "llama.cpp",
            "qwen2.5-3b-instruct-q4_k_m.gguf",
            "cleanup model timed out",
        );
        cleanup.elapsed_ms = 19;
        cleanup.used_ocr = true;
        entry.cleanup = Some(cleanup);
        entry.insertion = Some(InsertionDiagnostics::failed(
            "clipboard-paste",
            "Text Editor",
            "clipboard restore failed",
        ));

        let summary = diagnostics_summary_text(
            &settings,
            &cache_root,
            &[],
            Some(&archived_run(entry)),
            &RuntimeReadinessSummary::from_capabilities(&Capabilities::shell_default("0.1.0")),
        );

        assert!(summary.contains("Latest run ID: run-1"));
        assert!(summary.contains("ASR time: 21 ms"));
        assert!(summary.contains("Cleanup time: 19 ms"));
        assert!(summary.contains("OCR used: true"));
        assert!(summary.contains("Cleanup failure: cleanup model timed out"));
        assert!(summary.contains("Insertion backend: clipboard-paste"));
        assert!(summary.contains("Insertion failure: clipboard restore failed"));
    }

    #[test]
    fn main_window_content_snapshot_tracks_latest_provider_values() {
        let app = adw::Application::builder()
            .application_id("com.obra.PepperX.Tests")
            .build();
        let settings_surface_state = Rc::new(RefCell::new(SettingsSurfaceState {
            cleanup_enabled: true,
            cleanup_prompt_profile: "ordinary-dictation".into(),
            cleanup_custom_prompt: String::from("settings v1"),
            launch_at_login: false,
            feedback_message: None,
        }));
        let diagnostics_summary = Rc::new(RefCell::new(String::from("diagnostics v1")));
        let history_runs = Rc::new(RefCell::new(vec![archived_run(TranscriptEntry::new(
            "/tmp/run-1.wav",
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(21),
        ))]));
        let window = MainWindow::new_with_providers(
            &app,
            {
                let history_runs = history_runs.clone();
                move || history_runs.borrow().clone()
            },
            {
                let settings_surface_state = settings_surface_state.clone();
                move || settings_surface_state.borrow().clone()
            },
            {
                let diagnostics_summary = diagnostics_summary.clone();
                move || diagnostics_summary.borrow().clone()
            },
        );

        let initial = window.current_content_snapshot();
        assert_eq!(
            initial.settings_surface_state.cleanup_custom_prompt,
            "settings v1"
        );
        assert_eq!(initial.diagnostics_summary, "diagnostics v1");
        assert_eq!(initial.history_runs.len(), 1);

        settings_surface_state.replace(SettingsSurfaceState {
            cleanup_enabled: false,
            cleanup_prompt_profile: "literal-dictation".into(),
            cleanup_custom_prompt: String::from("settings v2"),
            launch_at_login: true,
            feedback_message: Some("Saved settings".into()),
        });
        diagnostics_summary.replace(String::from("diagnostics v2"));
        history_runs.replace(Vec::new());

        let refreshed = window.current_content_snapshot();
        assert_eq!(
            refreshed.settings_surface_state.cleanup_custom_prompt,
            "settings v2"
        );
        assert!(!refreshed.settings_surface_state.cleanup_enabled);
        assert!(refreshed.settings_surface_state.launch_at_login);
        assert_eq!(refreshed.diagnostics_summary, "diagnostics v2");
        assert!(refreshed.history_runs.is_empty());
    }

    #[test]
    fn window_page_routes_cover_all_shell_states() {
        assert_eq!(WindowPage::Settings.page_name(), SETTINGS_PAGE_NAME);
        assert_eq!(WindowPage::History.page_name(), HISTORY_PAGE_NAME);
        assert_eq!(WindowPage::Diagnostics.page_name(), DIAGNOSTICS_PAGE_NAME);
        assert_eq!(
            WindowPage::Settings.container_kind(),
            PageScaffoldKind::Form
        );
        assert_eq!(
            WindowPage::History.container_kind(),
            PageScaffoldKind::Browser
        );
        assert_eq!(
            WindowPage::Diagnostics.container_kind(),
            PageScaffoldKind::CardList
        );
    }

    #[test]
    fn window_settings_page_scaffold_builds_structured_rows() {
        let scaffold = settings_page_scaffold(&SettingsSurfaceState {
            cleanup_enabled: true,
            cleanup_prompt_profile: "ordinary-dictation".into(),
            cleanup_custom_prompt: "Keep Linux app names verbatim.".into(),
            launch_at_login: true,
            feedback_message: Some("Saved settings".into()),
        });

        assert_eq!(scaffold.container_kind, SettingsContainerKind::Form);
        assert_eq!(scaffold.sections.len(), 2);
        assert_eq!(scaffold.sections[0].title, "Cleanup");
        assert_eq!(scaffold.sections[1].title, "General");
        assert_eq!(scaffold.feedback_message.as_deref(), Some("Saved settings"));

        assert!(matches!(
            &scaffold.sections[0].controls[0],
            SettingsControl::Switch(SettingsSwitchControl {
                title,
                active: true,
                ..
            }) if title == "Enable cleanup"
        ));
        assert!(matches!(
            &scaffold.sections[0].controls[1],
            SettingsControl::Select(SettingsSelectControl {
                title,
                selected,
                options,
                ..
            }) if title == "Prompt profile"
                && selected == "ordinary-dictation"
                && options == &vec!["ordinary-dictation".to_string(), "literal-dictation".to_string()]
        ));
        assert!(matches!(
            &scaffold.sections[0].controls[2],
            SettingsControl::TextArea(SettingsTextAreaControl {
                title,
                text,
                enabled: true,
                ..
            }) if title == "Custom cleanup prompt"
                && text == "Keep Linux app names verbatim."
        ));
        assert!(matches!(
            &scaffold.sections[1].controls[0],
            SettingsControl::Switch(SettingsSwitchControl {
                title,
                active: true,
                ..
            }) if title == "Launch at login"
        ));
    }

    #[test]
    fn window_diagnostics_page_scaffold_builds_card_entries() {
        let scaffold = diagnostics_page_scaffold(
            "Modifier-only capture supported: true\nExtension connected: false\nService version: 0.1.0",
        );

        assert_eq!(scaffold.container_kind, DiagnosticsContainerKind::CardList);
        assert_eq!(scaffold.cards.len(), 3);
        assert_eq!(scaffold.cards[0].title, "Modifier-only capture supported");
        assert_eq!(scaffold.cards[0].body, "true");
        assert_eq!(scaffold.cards[2].title, "Service version");
    }

    #[test]
    fn app_shell_history_summary_shows_latest_friendly_insert_success() {
        let mut entry = TranscriptEntry::new(
            "/tmp/loop2.wav",
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(84),
        );
        entry.insertion = Some(
            InsertionDiagnostics::succeeded("atspi-editable-text", "Text Editor")
                .with_target_class("text-editor"),
        );

        let summary = history_summary_text(&[archived_run(entry)]);

        assert!(summary
            .contains("Friendly insertion: inserted into Text Editor via atspi-editable-text"));
        assert!(summary.contains("Target class: text-editor"));
    }

    #[test]
    fn app_shell_history_summary_shows_latest_friendly_insert_failure_reason() {
        let mut entry = TranscriptEntry::new(
            "/tmp/loop2.wav",
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(84),
        );
        entry.insertion = Some(
            InsertionDiagnostics::failed(
                "atspi-editable-text",
                "Calculator",
                "friendly insertion target is not editable",
            )
            .with_target_class("unsupported"),
        );

        let summary = history_summary_text(&[archived_run(entry)]);

        assert!(
            summary.contains("Friendly insertion: failed in Calculator via atspi-editable-text")
        );
        assert!(summary.contains("Target class: unsupported"));
        assert!(summary.contains("Reason: friendly insertion target is not editable"));
    }

    #[test]
    fn cleanup_history_summary_shows_raw_and_cleaned_transcript() {
        let entry: TranscriptEntry = serde_json::from_str(
            r#"{"source_wav_path":"/tmp/loop5.wav","transcript_text":"hello from pepper x","backend_name":"sherpa-onnx","model_name":"nemo-parakeet-tdt-0.6b-v2-int8","elapsed_ms":21,"cleanup":{"backend_name":"llama.cpp","model_name":"qwen2.5-3b-instruct-q4_k_m.gguf","cleaned_text":"Hello from Pepper X.","elapsed_ms":19,"used_ocr":false,"succeeded":true}}"#,
        )
        .expect("deserialize cleanup transcript entry");

        let summary = history_summary_text(&[archived_run(entry)]);

        assert!(summary.contains("Raw transcript:\nhello from pepper x"));
        assert!(summary.contains("Cleaned transcript:\nHello from Pepper X."));
    }

    #[test]
    fn model_status_history_summary_shows_archived_prompt_profile() {
        let mut run = archived_run(TranscriptEntry::new(
            "/tmp/loop5.wav",
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(21),
        ));
        run.prompt_profile = Some("ordinary-dictation".into());

        let summary = history_summary_text(&[run]);

        assert!(summary.contains("Cleanup prompt profile: ordinary-dictation"));
    }

    #[test]
    fn correction_memory_history_summary_shows_latest_learning_action() {
        let mut entry = TranscriptEntry::new(
            "/tmp/loop5.wav",
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(21),
        );
        entry.learning = Some(LearningDiagnostics::prompt_memory(
            "hello from pepper x",
            "Hello from Pepper X.",
        ));

        let summary = history_summary_text(&[archived_run(entry)]);

        assert!(summary.contains("Correction memory updated"));
        assert!(summary.contains("hello from pepper x -> Hello from Pepper X."));
    }
}
