use adw::prelude::*;
use gtk::{Align, Orientation};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use crate::app_model::{AppModel, RuntimeReadinessSummary};
use crate::history_view::build_history_browser;
use pepperx_models::{ModelInventoryEntry, ModelKind};

use crate::history_store::ArchivedRun;
use crate::settings::AppSettings;

const SETUP_PAGE_NAME: &str = "setup";
const SETTINGS_PAGE_NAME: &str = "settings";
const HISTORY_PAGE_NAME: &str = "history";
const DIAGNOSTICS_PAGE_NAME: &str = "diagnostics";

struct WindowContentProviders {
    history_runs: Rc<dyn Fn() -> Vec<ArchivedRun>>,
    settings_summary: Rc<dyn Fn() -> String>,
    diagnostics_summary: Rc<dyn Fn() -> String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowContentSnapshot {
    history_runs: Vec<ArchivedRun>,
    settings_summary: String,
    diagnostics_summary: String,
}

#[derive(Clone)]
pub struct MainWindow {
    app: adw::Application,
    content_providers: Rc<WindowContentProviders>,
    rerun_archived_run: Option<Rc<dyn Fn(String)>>,
    state: Rc<RefCell<Option<WindowState>>>,
}

struct WindowState {
    window: adw::ApplicationWindow,
    stack: gtk::Stack,
    setup_title_label: gtk::Label,
    setup_label: gtk::Label,
    settings_label: gtk::Label,
    diagnostics_label: gtk::Label,
    history_container: gtk::Box,
}

impl WindowContentProviders {
    fn snapshot(&self) -> WindowContentSnapshot {
        WindowContentSnapshot {
            history_runs: (self.history_runs)(),
            settings_summary: (self.settings_summary)(),
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
            default_settings_summary(),
            default_diagnostics_summary(),
        )
    }

    pub fn new_with_history_and_settings(
        app: &adw::Application,
        history_runs: Vec<ArchivedRun>,
        settings_summary: String,
        diagnostics_summary: String,
    ) -> Self {
        let history_runs = Rc::new(history_runs);
        let settings_summary = Rc::new(settings_summary);
        let diagnostics_summary = Rc::new(diagnostics_summary);

        Self::new_with_providers(
            app,
            {
                let history_runs = history_runs.clone();
                move || history_runs.as_ref().clone()
            },
            {
                let settings_summary = settings_summary.clone();
                move || settings_summary.as_ref().clone()
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
        settings_summary: S,
        diagnostics_summary: D,
    ) -> Self
    where
        H: Fn() -> Vec<ArchivedRun> + 'static,
        S: Fn() -> String + 'static,
        D: Fn() -> String + 'static,
    {
        Self {
            app: app.clone(),
            content_providers: Rc::new(WindowContentProviders {
                history_runs: Rc::new(history_runs),
                settings_summary: Rc::new(settings_summary),
                diagnostics_summary: Rc::new(diagnostics_summary),
            }),
            rerun_archived_run: None,
            state: Rc::new(RefCell::new(None)),
        }
    }

    pub(crate) fn new_with_providers_and_rerun<H, S, D>(
        app: &adw::Application,
        history_runs: H,
        settings_summary: S,
        diagnostics_summary: D,
        rerun_archived_run: Option<Rc<dyn Fn(String)>>,
    ) -> Self
    where
        H: Fn() -> Vec<ArchivedRun> + 'static,
        S: Fn() -> String + 'static,
        D: Fn() -> String + 'static,
    {
        let mut window =
            Self::new_with_providers(app, history_runs, settings_summary, diagnostics_summary);
        window.rerun_archived_run = rerun_archived_run;
        window
    }

    #[cfg(test)]
    pub fn application_id(&self) -> Option<String> {
        self.app.application_id().map(|id| id.to_string())
    }

    pub fn present_settings(&self) {
        self.present_page(SETTINGS_PAGE_NAME);
    }

    pub fn present_setup(&self, app_model: &AppModel) {
        self.ensure_window();
        self.refresh_content();

        if let Some(state) = self.state.borrow().as_ref() {
            state.setup_title_label.set_label(app_model.setup_title());
            state.setup_label.set_label(&app_model.setup_description());
            state.stack.set_visible_child_name(SETUP_PAGE_NAME);
            state.window.present();
        }
    }

    pub fn present_history(&self) {
        self.present_page(HISTORY_PAGE_NAME);
    }

    fn current_content_snapshot(&self) -> WindowContentSnapshot {
        self.content_providers.snapshot()
    }

    fn present_page(&self, page_name: &str) {
        self.ensure_window();
        self.refresh_content();

        if let Some(state) = self.state.borrow().as_ref() {
            state.stack.set_visible_child_name(page_name);
            state.window.present();
        }
    }

    fn ensure_window(&self) {
        if self.state.borrow().is_some() {
            return;
        }

        let snapshot = self.current_content_snapshot();
        let stack = gtk::Stack::builder()
            .hexpand(true)
            .vexpand(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        let (setup_page, setup_title_label, setup_label) =
            build_page("Finish Pepper X setup", "Pepper X setup will appear here.");
        stack.add_titled(&setup_page, Some(SETUP_PAGE_NAME), "Setup");
        let (settings_page, _, settings_label) =
            build_page("Settings", snapshot.settings_summary.as_str());
        stack.add_titled(&settings_page, Some(SETTINGS_PAGE_NAME), "Settings");
        let history_container = gtk::Box::new(Orientation::Vertical, 0);
        history_container.set_hexpand(true);
        history_container.set_vexpand(true);
        history_container.append(&build_history_browser(
            &snapshot.history_runs,
            self.rerun_archived_run.clone(),
        ));
        stack.add_titled(&history_container, Some(HISTORY_PAGE_NAME), "History");
        let (diagnostics_page, _, diagnostics_label) =
            build_page("Diagnostics", snapshot.diagnostics_summary.as_str());
        stack.add_titled(
            &diagnostics_page,
            Some(DIAGNOSTICS_PAGE_NAME),
            "Diagnostics",
        );

        let stack_switcher = gtk::StackSwitcher::new();
        stack_switcher.set_stack(Some(&stack));

        let header_bar = adw::HeaderBar::builder()
            .title_widget(&adw::WindowTitle::new(
                "Pepper X",
                "GNOME-first local dictation shell",
            ))
            .build();
        header_bar.pack_start(&stack_switcher);

        let view = adw::ToolbarView::new();
        view.add_top_bar(&header_bar);
        view.set_content(Some(&stack));

        let window = adw::ApplicationWindow::builder()
            .application(&self.app)
            .title("Pepper X")
            .default_width(720)
            .default_height(480)
            .content(&view)
            .build();

        *self.state.borrow_mut() = Some(WindowState {
            window,
            stack,
            setup_title_label,
            setup_label,
            settings_label,
            diagnostics_label,
            history_container,
        });
    }

    fn refresh_content(&self) {
        let snapshot = self.current_content_snapshot();
        let state = self.state.borrow();
        let Some(state) = state.as_ref() else {
            return;
        };

        state.settings_label.set_label(&snapshot.settings_summary);
        state
            .diagnostics_label
            .set_label(&snapshot.diagnostics_summary);
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
fn default_settings_summary() -> String {
    String::from("Pepper X shell settings and GNOME integration controls live here.")
}

#[cfg(test)]
fn default_diagnostics_summary() -> String {
    String::from("Pepper X runtime diagnostics surface lives here.")
}

fn build_page(title: &str, description: &str) -> (gtk::Box, gtk::Label, gtk::Label) {
    let container = gtk::Box::new(Orientation::Vertical, 12);
    container.set_margin_top(24);
    container.set_margin_bottom(24);
    container.set_margin_start(24);
    container.set_margin_end(24);
    container.set_valign(Align::Start);

    let title_label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["title-2"])
        .build();
    let description_label = gtk::Label::builder()
        .label(description)
        .wrap(true)
        .xalign(0.0)
        .build();

    container.append(&title_label);
    container.append(&description_label);
    (container, title_label, description_label)
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
    use crate::settings::AppSettings;
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
        let settings_summary = Rc::new(RefCell::new(String::from("settings v1")));
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
                let settings_summary = settings_summary.clone();
                move || settings_summary.borrow().clone()
            },
            {
                let diagnostics_summary = diagnostics_summary.clone();
                move || diagnostics_summary.borrow().clone()
            },
        );

        let initial = window.current_content_snapshot();
        assert_eq!(initial.settings_summary, "settings v1");
        assert_eq!(initial.diagnostics_summary, "diagnostics v1");
        assert_eq!(initial.history_runs.len(), 1);

        settings_summary.replace(String::from("settings v2"));
        diagnostics_summary.replace(String::from("diagnostics v2"));
        history_runs.replace(Vec::new());

        let refreshed = window.current_content_snapshot();
        assert_eq!(refreshed.settings_summary, "settings v2");
        assert_eq!(refreshed.diagnostics_summary, "diagnostics v2");
        assert!(refreshed.history_runs.is_empty());
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
