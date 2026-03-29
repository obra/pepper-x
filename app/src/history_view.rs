use adw::prelude::*;
use gtk::{Align, Orientation, PolicyType, SelectionMode};
use std::cell::RefCell;
use std::rc::Rc;

use crate::history_store::ArchivedRun;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryBrowserModel {
    runs: Vec<ArchivedRun>,
    selected_index: usize,
}

impl HistoryBrowserModel {
    pub(crate) fn new(mut runs: Vec<ArchivedRun>) -> Self {
        sort_runs_newest_first(&mut runs);
        Self {
            runs,
            selected_index: 0,
        }
    }

    pub(crate) fn visible_run_ids(&self) -> Vec<String> {
        self.runs.iter().map(|run| run.run_id.clone()).collect()
    }

    pub(crate) fn selected_run_id(&self) -> Option<&str> {
        self.runs
            .get(self.selected_index)
            .map(|run| run.run_id.as_str())
    }

    pub(crate) fn selected_details_text(&self) -> Option<String> {
        self.selected_run().map(history_run_details_text)
    }

    pub(crate) fn select_run(&mut self, run_id: &str) -> bool {
        let Some(index) = self.runs.iter().position(|run| run.run_id == run_id) else {
            return false;
        };
        self.selected_index = index;
        true
    }

    fn select_index(&mut self, index: usize) -> bool {
        if index >= self.runs.len() {
            return false;
        }
        self.selected_index = index;
        true
    }

    fn selected_run(&self) -> Option<&ArchivedRun> {
        self.runs.get(self.selected_index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HistoryRunSections {
    raw_text: String,
    cleaned_text: Option<String>,
    metadata_text: String,
}

pub(crate) fn build_history_browser(runs: &[ArchivedRun]) -> gtk::Paned {
    let model = Rc::new(RefCell::new(HistoryBrowserModel::new(runs.to_vec())));

    let list_box = gtk::ListBox::builder()
        .selection_mode(SelectionMode::Single)
        .vexpand(true)
        .build();
    for run in &model.borrow().runs {
        list_box.append(&build_history_row(run));
    }

    let raw_label = detail_label("No archived runs yet.");
    let cleaned_label = detail_label("No cleanup transcript for this run.");
    let metadata_label = detail_label("Select an archived run to inspect its metadata.");
    if let Some(run) = model.borrow().selected_run() {
        set_detail_sections(&raw_label, &cleaned_label, &metadata_label, run);
    }

    let details_box = gtk::Box::new(Orientation::Vertical, 18);
    details_box.set_margin_top(18);
    details_box.set_margin_bottom(18);
    details_box.set_margin_start(18);
    details_box.set_margin_end(18);
    details_box.append(&section_label("Raw Transcript"));
    details_box.append(&raw_label);
    details_box.append(&section_label("Cleaned Transcript"));
    details_box.append(&cleaned_label);
    details_box.append(&section_label("Run Metadata"));
    details_box.append(&metadata_label);

    let details_scroll = gtk::ScrolledWindow::new();
    details_scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    details_scroll.set_child(Some(&details_box));

    let list_scroll = gtk::ScrolledWindow::new();
    list_scroll.set_min_content_width(280);
    list_scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    list_scroll.set_child(Some(&list_box));

    {
        let model = model.clone();
        let raw_label = raw_label.clone();
        let cleaned_label = cleaned_label.clone();
        let metadata_label = metadata_label.clone();
        list_box.connect_row_selected(move |_, row| {
            let Some(row) = row else {
                return;
            };
            let Ok(index) = usize::try_from(row.index()) else {
                return;
            };
            let mut model = model.borrow_mut();
            if !model.select_index(index) {
                return;
            }
            if let Some(run) = model.selected_run() {
                set_detail_sections(&raw_label, &cleaned_label, &metadata_label, run);
            }
        });
    }

    if let Some(first_row) = list_box.row_at_index(0) {
        list_box.select_row(Some(&first_row));
    }

    let browser = gtk::Paned::new(Orientation::Horizontal);
    browser.set_wide_handle(true);
    browser.set_position(300);
    browser.set_start_child(Some(&list_scroll));
    browser.set_end_child(Some(&details_scroll));
    browser
}

pub(crate) fn history_run_details_text(run: &ArchivedRun) -> String {
    let sections = history_run_sections(run);
    let mut details = format!("Raw transcript:\n{}", sections.raw_text);
    if let Some(cleaned_text) = sections.cleaned_text {
        details.push_str(&format!("\n\nCleaned transcript:\n{cleaned_text}"));
    }
    details.push_str(&format!("\n\n{}", sections.metadata_text));
    details
}

fn history_run_sections(run: &ArchivedRun) -> HistoryRunSections {
    let entry = &run.entry;
    let mut metadata_lines = vec![
        format!("Run ID: {}", run.run_id),
        format!("ASR backend: {}", entry.backend_name),
        format!("ASR model: {}", entry.model_name),
        format!("ASR time: {} ms", entry.elapsed_ms),
    ];

    if let Some(prompt_profile) = run.prompt_profile.as_deref() {
        metadata_lines.push(format!("Cleanup prompt profile: {prompt_profile}"));
    }

    if let Some(cleanup) = entry.cleanup.as_ref() {
        metadata_lines.push(format!("Cleanup backend: {}", cleanup.backend_name));
        metadata_lines.push(format!("Cleanup model: {}", cleanup.model_name));
        if cleanup.succeeded {
            metadata_lines.push(format!("Cleanup time: {} ms", cleanup.elapsed_ms));
        } else if let Some(reason) = cleanup.failure_reason.as_deref() {
            metadata_lines.push(format!("Cleanup failure: {reason}"));
        }
        metadata_lines.push(format!("OCR used: {}", cleanup.used_ocr));
    }

    if let Some(insertion) = entry.insertion.as_ref() {
        metadata_lines.push(format!("Insertion backend: {}", insertion.backend_name));
        metadata_lines.push(format!(
            "Insertion target: {}",
            insertion.target_application_name
        ));
        if let Some(target_class) = insertion.target_class.as_deref() {
            metadata_lines.push(format!("Target class: {target_class}"));
        }
        if !insertion.succeeded {
            metadata_lines.push(format!(
                "Insertion failure: {}",
                insertion
                    .failure_reason
                    .as_deref()
                    .unwrap_or("unknown insertion failure")
            ));
        }
    }

    if let Some(supporting_context_text) = run.supporting_context_text.as_deref() {
        metadata_lines.push(format!("Supporting context: {}", supporting_context_text));
    }

    if let Some(ocr_text) = run.ocr_text.as_deref() {
        metadata_lines.push(format!("OCR text: {ocr_text}"));
    }

    HistoryRunSections {
        raw_text: entry.transcript_text.clone(),
        cleaned_text: entry
            .cleanup
            .as_ref()
            .and_then(|cleanup| cleanup.cleaned_text().map(str::to_string)),
        metadata_text: metadata_lines.join("\n"),
    }
}

fn sort_runs_newest_first(runs: &mut [ArchivedRun]) {
    runs.sort_by(|left, right| {
        right
            .archived_at_ms
            .cmp(&left.archived_at_ms)
            .then_with(|| right.run_id.cmp(&left.run_id))
    });
}

fn build_history_row(run: &ArchivedRun) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let action_row = adw::ActionRow::builder()
        .title(history_row_title(run))
        .subtitle(history_row_subtitle(run))
        .activatable(false)
        .build();
    row.set_child(Some(&action_row));
    row
}

fn history_row_title(run: &ArchivedRun) -> String {
    let text = run
        .entry
        .cleanup
        .as_ref()
        .and_then(|cleanup| cleanup.cleaned_text())
        .unwrap_or(&run.entry.transcript_text);
    let preview: String = text.chars().take(48).collect();
    if text.chars().count() > 48 {
        format!("{preview}...")
    } else {
        preview
    }
}

fn history_row_subtitle(run: &ArchivedRun) -> String {
    format!("{} • {} ms", run.entry.model_name, run.entry.elapsed_ms)
}

fn section_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["title-5"])
        .build()
}

fn detail_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .halign(Align::Fill)
        .build()
}

fn set_detail_sections(
    raw_label: &gtk::Label,
    cleaned_label: &gtk::Label,
    metadata_label: &gtk::Label,
    run: &ArchivedRun,
) {
    let sections = history_run_sections(run);
    raw_label.set_label(&sections.raw_text);
    cleaned_label.set_label(
        sections
            .cleaned_text
            .as_deref()
            .unwrap_or("No cleanup transcript for this run."),
    );
    metadata_label.set_label(&sections.metadata_text);
}

#[cfg(test)]
mod history_view_tests {
    use super::*;
    use crate::history_store::ArchivedRun;
    use crate::transcript_log::{CleanupDiagnostics, InsertionDiagnostics, TranscriptEntry};
    use std::path::PathBuf;
    use std::time::Duration;

    fn archived_run(
        run_id: &str,
        archived_at_ms: u64,
        raw_transcript: &str,
        cleaned_transcript: Option<&str>,
        insertion_backend: &str,
    ) -> ArchivedRun {
        let mut entry = TranscriptEntry::new(
            format!("/tmp/{run_id}.wav"),
            raw_transcript,
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(42),
        );
        if let Some(cleaned_transcript) = cleaned_transcript {
            entry.cleanup = Some(CleanupDiagnostics::succeeded(
                "llama.cpp",
                "qwen2.5-3b-instruct-q4_k_m.gguf",
                cleaned_transcript,
                Duration::from_millis(19),
            ));
        }
        entry.insertion = Some(
            InsertionDiagnostics::succeeded(insertion_backend, "Text Editor")
                .with_target_class("text-editor"),
        );

        ArchivedRun {
            run_id: run_id.into(),
            archived_at_ms,
            run_dir: PathBuf::from(format!("/tmp/history/{run_id}")),
            metadata_path: PathBuf::from(format!("/tmp/history/{run_id}/run.json")),
            entry,
            archived_source_wav_path: Some(PathBuf::from(format!(
                "/tmp/history/{run_id}/source.wav"
            ))),
            prompt_profile: Some("ordinary-dictation".into()),
            supporting_context_text: None,
            ocr_text: None,
        }
    }

    #[test]
    fn history_view_lists_runs_newest_first() {
        let model = HistoryBrowserModel::new(vec![
            archived_run(
                "run-older",
                10,
                "older transcript",
                Some("Older transcript."),
                "atspi-editable-text",
            ),
            archived_run(
                "run-newer",
                20,
                "newer transcript",
                Some("Newer transcript."),
                "clipboard-paste",
            ),
        ]);

        assert_eq!(
            model.visible_run_ids(),
            vec!["run-newer".to_string(), "run-older".to_string()]
        );
        assert_eq!(model.selected_run_id(), Some("run-newer"));
    }

    #[test]
    fn history_view_details_show_raw_cleaned_models_insertion_and_timings() {
        let run = archived_run(
            "run-1",
            20,
            "hello from pepper x",
            Some("Hello from Pepper X."),
            "atspi-editable-text",
        );

        let details = history_run_details_text(&run);

        assert!(details.contains("Raw transcript:\nhello from pepper x"));
        assert!(details.contains("Cleaned transcript:\nHello from Pepper X."));
        assert!(details.contains("ASR model: nemo-parakeet-tdt-0.6b-v2-int8"));
        assert!(details.contains("Cleanup model: qwen2.5-3b-instruct-q4_k_m.gguf"));
        assert!(details.contains("Insertion backend: atspi-editable-text"));
        assert!(details.contains("ASR time: 42 ms"));
        assert!(details.contains("Cleanup time: 19 ms"));
    }

    #[test]
    fn history_view_selects_one_run_without_rebuilding_item_order() {
        let mut model = HistoryBrowserModel::new(vec![
            archived_run(
                "run-older",
                10,
                "older transcript",
                Some("Older transcript."),
                "atspi-editable-text",
            ),
            archived_run(
                "run-newer",
                20,
                "newer transcript",
                Some("Newer transcript."),
                "clipboard-paste",
            ),
        ]);
        let visible_run_ids = model.visible_run_ids();

        assert!(model.select_run("run-older"));
        assert_eq!(model.visible_run_ids(), visible_run_ids);
        assert_eq!(model.selected_run_id(), Some("run-older"));
        assert!(model
            .selected_details_text()
            .expect("selected details text")
            .contains("older transcript"));
    }
}
