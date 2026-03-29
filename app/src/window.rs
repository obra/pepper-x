use adw::prelude::*;
use gtk::{Align, Orientation};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use pepperx_models::{ModelInventoryEntry, ModelKind};

use crate::history_store::ArchivedRun;
use crate::settings::AppSettings;

const SETTINGS_PAGE_NAME: &str = "settings";
const HISTORY_PAGE_NAME: &str = "history";

#[derive(Clone)]
pub struct MainWindow {
    app: adw::Application,
    settings_summary: Rc<String>,
    history_summary: Rc<String>,
    state: Rc<RefCell<Option<WindowState>>>,
}

struct WindowState {
    window: adw::ApplicationWindow,
    stack: gtk::Stack,
}

impl MainWindow {
    #[cfg(test)]
    pub fn new(app: &adw::Application) -> Self {
        Self::new_with_history_and_settings(app, Vec::new(), default_settings_summary())
    }

    pub fn new_with_history_and_settings(
        app: &adw::Application,
        history_runs: Vec<ArchivedRun>,
        settings_summary: String,
    ) -> Self {
        Self {
            app: app.clone(),
            settings_summary: Rc::new(settings_summary),
            history_summary: Rc::new(history_summary_text(&history_runs)),
            state: Rc::new(RefCell::new(None)),
        }
    }

    #[cfg(test)]
    pub fn application_id(&self) -> Option<String> {
        self.app.application_id().map(|id| id.to_string())
    }

    pub fn present_settings(&self) {
        self.present_page(SETTINGS_PAGE_NAME);
    }

    pub fn present_history(&self) {
        self.present_page(HISTORY_PAGE_NAME);
    }

    fn present_page(&self, page_name: &str) {
        self.ensure_window();

        if let Some(state) = self.state.borrow().as_ref() {
            state.stack.set_visible_child_name(page_name);
            state.window.present();
        }
    }

    fn ensure_window(&self) {
        if self.state.borrow().is_some() {
            return;
        }

        let stack = gtk::Stack::builder()
            .hexpand(true)
            .vexpand(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_titled(
            &build_page("Settings", self.settings_summary.as_str()),
            Some(SETTINGS_PAGE_NAME),
            "Settings",
        );
        stack.add_titled(
            &build_page("History", self.history_summary.as_str()),
            Some(HISTORY_PAGE_NAME),
            "History",
        );

        let header_bar = adw::HeaderBar::builder()
            .title_widget(&adw::WindowTitle::new(
                "Pepper X",
                "GNOME-first local dictation shell",
            ))
            .build();

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

        *self.state.borrow_mut() = Some(WindowState { window, stack });
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

fn build_page(title: &str, description: &str) -> gtk::Box {
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
    container
}

#[cfg(test)]
mod app_shell {
    use super::*;
    use crate::settings::AppSettings;
    use crate::transcript_log::InsertionDiagnostics;
    use crate::transcript_log::TranscriptEntry;
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
}
