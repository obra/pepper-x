use adw::prelude::*;
use gtk::glib;
use gtk::{Align, Orientation, PolicyType, SelectionMode};
use pepperx_models::{supported_models, ModelKind};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::history_store::ArchivedRun;
use crate::settings::AppSettings;
use crate::transcript_log::{DiarizationSummary, TranscriptEntry};

#[derive(Debug, Clone, PartialEq)]
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
        let selected_run = self.selected_run()?;
        match self.comparison_runs_for_selected() {
            Some((original_run, rerun)) => {
                Some(details_text_comparison(original_run, rerun))
            }
            None => Some(details_text_single(selected_run)),
        }
    }

    pub(crate) fn select_run(&mut self, run_id: &str) -> bool {
        let Some(index) = self.runs.iter().position(|run| run.run_id == run_id) else {
            return false;
        };
        self.selected_index = index;
        true
    }

    pub(crate) fn rerunnable_run_id(&self) -> Option<&str> {
        let selected_run = self.selected_run()?;
        Some(
            selected_run
                .parent_run_id
                .as_deref()
                .unwrap_or(selected_run.run_id.as_str()),
        )
    }

    pub(crate) fn cleanup_rerunnable_run_id(&self) -> Option<&str> {
        let selected_run = self.selected_run()?;
        Some(selected_run.run_id.as_str())
    }

    pub(crate) fn selected_wav_path(&self) -> Option<PathBuf> {
        let selected_run = self.selected_run()?;
        let wav_path = selected_run.archived_source_wav_path.as_ref()?;
        if wav_path.is_file() {
            Some(wav_path.clone())
        } else {
            None
        }
    }

    pub(crate) fn selected_asr_model(&self) -> Option<&str> {
        self.selected_run()
            .map(|run| run.entry.model_name.as_str())
    }

    pub(crate) fn selected_cleanup_model(&self) -> Option<&str> {
        self.selected_run().and_then(|run| {
            run.entry
                .cleanup
                .as_ref()
                .map(|cleanup| cleanup.model_name.as_str())
        })
    }

    pub(crate) fn selected_diarization(&self) -> Option<&DiarizationSummary> {
        self.selected_run()
            .and_then(|run| run.entry.diarization.as_ref())
    }

    pub(crate) fn selected_recording_duration_secs(&self) -> f64 {
        self.selected_run()
            .and_then(|run| run.runtime_metadata.recording_elapsed_ms)
            .map(|ms| ms as f64 / 1000.0)
            .unwrap_or(0.0)
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

    fn comparison_runs_for_selected(&self) -> Option<(&ArchivedRun, &ArchivedRun)> {
        let selected_run = self.selected_run()?;
        if let Some(parent_run_id) = selected_run.parent_run_id.as_deref() {
            let parent_run = self.runs.iter().find(|run| run.run_id == parent_run_id)?;
            return Some((parent_run, selected_run));
        }

        let rerun = self
            .runs
            .iter()
            .filter(|run| run.parent_run_id.as_deref() == Some(selected_run.run_id.as_str()))
            .max_by(|left, right| {
                left.archived_at_ms
                    .cmp(&right.archived_at_ms)
                    .then_with(|| left.run_id.cmp(&right.run_id))
            })?;

        Some((selected_run, rerun))
    }
}

// ---------------------------------------------------------------------------
// Ghost Pepper detail view: build the right-side scrollable page
// ---------------------------------------------------------------------------

pub(crate) fn build_history_browser(
    runs: &[ArchivedRun],
    rerun_archived_run: Option<Rc<dyn Fn(String, String) -> Option<TranscriptEntry>>>,
    rerun_cleanup: Option<Rc<dyn Fn(String, String, Option<String>) -> Option<TranscriptEntry>>>,
    play_audio: Option<Rc<dyn Fn(PathBuf)>>,
) -> gtk::Paned {
    let model = Rc::new(RefCell::new(HistoryBrowserModel::new(runs.to_vec())));
    let settings = AppSettings::load_or_default();

    // --- Left side: run list ---
    let list_box = gtk::ListBox::builder()
        .selection_mode(SelectionMode::Single)
        .vexpand(true)
        .build();
    for run in &model.borrow().runs {
        list_box.append(&build_history_row(run));
    }

    // --- Right side: detail content container ---
    let details_box = gtk::Box::new(Orientation::Vertical, 18);
    details_box.set_margin_top(18);
    details_box.set_margin_bottom(18);
    details_box.set_margin_start(18);
    details_box.set_margin_end(18);

    // =====================================================================
    // 1. Audio recording section
    // =====================================================================
    let audio_heading = section_label("Audio recording");
    let metadata_line = detail_label("");
    let diarization_container = gtk::Box::new(Orientation::Vertical, 0);
    let play_button = gtk::Button::with_label("Play recording");
    play_button.set_halign(Align::Start);

    details_box.append(&audio_heading);
    details_box.append(&metadata_line);
    details_box.append(&diarization_container);
    details_box.append(&play_button);

    // =====================================================================
    // 2. Transcription section
    // =====================================================================
    let transcription_heading = section_label("Transcription");
    let transcription_subtitle = detail_label("");
    transcription_subtitle.add_css_class("dim-label");
    let original_raw_card = dark_card_label("No archived runs yet.");

    // ASR model picker + Run transcription button
    let asr_model_ids: Vec<String> = supported_models()
        .iter()
        .filter(|m| m.kind == ModelKind::Asr)
        .map(|m| m.id.to_string())
        .collect();
    let asr_string_list = gtk::StringList::new(
        &asr_model_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let asr_dropdown = gtk::DropDown::new(Some(asr_string_list), None::<gtk::Expression>);
    let initial_asr = model
        .borrow()
        .selected_asr_model()
        .and_then(|m| asr_model_ids.iter().position(|id| id == m))
        .unwrap_or(0);
    asr_dropdown.set_selected(initial_asr as u32);

    let asr_row = gtk::Box::new(Orientation::Horizontal, 8);
    asr_row.set_halign(Align::Start);
    let asr_prefix = picker_label("Use transcription model");
    asr_prefix.set_valign(Align::Center);
    let rerun_button = gtk::Button::with_label("Run transcription");
    asr_row.append(&asr_prefix);
    asr_row.append(&asr_dropdown);
    asr_row.append(&rerun_button);

    // Rerun raw transcript result card (hidden until rerun)
    let rerun_raw_card = dark_card_label("");
    rerun_raw_card.0.set_visible(false);

    details_box.append(&transcription_heading);
    details_box.append(&transcription_subtitle);
    details_box.append(&original_raw_card.0);
    details_box.append(&asr_row);
    details_box.append(&rerun_raw_card.0);

    // =====================================================================
    // 3. Cleanup section
    // =====================================================================
    let cleanup_heading = section_label("Cleanup");
    let cleanup_subtitle = detail_label("");
    cleanup_subtitle.add_css_class("dim-label");
    let original_cleaned_card = dark_card_label("No cleanup transcript for this run.");

    // Cleanup prompt label + Reset to Default button
    let prompt_header_box = gtk::Box::new(Orientation::Horizontal, 8);
    prompt_header_box.set_halign(Align::Fill);
    let prompt_header_label = picker_label("Cleanup prompt");
    prompt_header_label.set_hexpand(true);
    let reset_prompt_button = gtk::Button::with_label("Reset to Default");
    reset_prompt_button.set_halign(Align::End);
    prompt_header_box.append(&prompt_header_label);
    prompt_header_box.append(&reset_prompt_button);

    // Editable prompt in dark-styled TextView
    let prompt_text_view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::Word)
        .editable(true)
        .accepts_tab(false)
        .height_request(150)
        .build();
    let initial_prompt = settings.effective_cleanup_custom_prompt().unwrap_or_default();
    prompt_text_view.buffer().set_text(&initial_prompt);

    let prompt_frame = gtk::Frame::new(None);
    prompt_frame.add_css_class("view");
    let prompt_scroll = gtk::ScrolledWindow::new();
    prompt_scroll.set_policy(PolicyType::Automatic, PolicyType::Automatic);
    prompt_scroll.set_child(Some(&prompt_text_view));
    prompt_frame.set_child(Some(&prompt_scroll));

    // "Use captured OCR" checkbox (shown only when entry has OCR data)
    let use_ocr_check = gtk::CheckButton::with_label("Use captured OCR");
    use_ocr_check.set_active(false);
    use_ocr_check.set_visible(model.borrow().selected_run()
        .and_then(|r| r.ocr_text.as_ref())
        .is_some());

    // Cleanup model picker + buttons row
    let cleanup_model_ids: Vec<String> = supported_models()
        .iter()
        .filter(|m| m.kind == ModelKind::Cleanup)
        .map(|m| m.id.to_string())
        .collect();
    let cleanup_string_list = gtk::StringList::new(
        &cleanup_model_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let cleanup_dropdown = gtk::DropDown::new(Some(cleanup_string_list), None::<gtk::Expression>);
    let initial_cleanup = model
        .borrow()
        .selected_cleanup_model()
        .and_then(|m| cleanup_model_ids.iter().position(|id| id == m))
        .unwrap_or(0);
    cleanup_dropdown.set_selected(initial_cleanup as u32);

    let rerun_cleanup_button = gtk::Button::with_label("Run cleanup");
    let cleanup_timing_label = detail_label("");
    cleanup_timing_label.set_visible(false);

    let cleanup_action_row = gtk::Box::new(Orientation::Horizontal, 8);
    cleanup_action_row.set_halign(Align::Start);
    let cleanup_prefix = picker_label("Clean with");
    cleanup_prefix.set_valign(Align::Center);
    cleanup_action_row.append(&cleanup_prefix);
    cleanup_action_row.append(&cleanup_dropdown);
    cleanup_action_row.append(&rerun_cleanup_button);
    cleanup_action_row.append(&cleanup_timing_label);

    // Rerun cleaned transcript result card with diff highlighting (hidden until rerun)
    let rerun_cleaned_card = dark_card_markup("");
    rerun_cleaned_card.0.set_visible(false);

    details_box.append(&cleanup_heading);
    details_box.append(&cleanup_subtitle);
    details_box.append(&original_cleaned_card.0);
    details_box.append(&prompt_header_box);
    details_box.append(&prompt_frame);
    details_box.append(&use_ocr_check);
    details_box.append(&cleanup_action_row);
    details_box.append(&rerun_cleaned_card.0);

    // =====================================================================
    // Populate initial detail content
    // =====================================================================
    {
        let model_ref = model.borrow();
        if let Some(run) = model_ref.selected_run() {
            populate_detail_for_run(
                run,
                &metadata_line,
                &transcription_subtitle,
                &original_raw_card.1,
                &cleanup_subtitle,
                &original_cleaned_card.1,
                &diarization_container,
                model_ref.selected_recording_duration_secs(),
                &use_ocr_check,
            );
        }
    }

    // =====================================================================
    // Callbacks
    // =====================================================================

    // Reset prompt button
    {
        let prompt_text_view = prompt_text_view.clone();
        reset_prompt_button.connect_clicked(move |_| {
            prompt_text_view.buffer().set_text("");
        });
    }

    // Play audio button
    if let Some(play_audio) = play_audio {
        play_button.set_sensitive(model.borrow().selected_wav_path().is_some());
        {
            let model = model.clone();
            play_button.connect_clicked(move |_| {
                let Some(wav_path) = model.borrow().selected_wav_path() else {
                    return;
                };
                play_audio(wav_path);
            });
        }
    } else {
        play_button.set_sensitive(false);
    }

    // Rerun transcription button
    if let Some(rerun_archived_run) = rerun_archived_run {
        rerun_button.set_sensitive(model.borrow().rerunnable_run_id().is_some());
        {
            let model = model.clone();
            let asr_dropdown = asr_dropdown.clone();
            let asr_model_ids = asr_model_ids.clone();
            let rerun_raw_card_frame = rerun_raw_card.0.clone();
            let rerun_raw_card_label = rerun_raw_card.1.clone();
            let rerun_cleaned_card_frame = rerun_cleaned_card.0.clone();
            let rerun_cleaned_card_label = rerun_cleaned_card.1.clone();
            let original_cleaned_label = original_cleaned_card.1.clone();
            rerun_button.connect_clicked(move |_| {
                let run_id = {
                    let model = model.borrow();
                    model.rerunnable_run_id().map(str::to_string)
                };
                let Some(run_id) = run_id else {
                    return;
                };
                let asr_model_id = asr_model_ids
                    .get(asr_dropdown.selected() as usize)
                    .cloned()
                    .unwrap_or_default();
                if let Some(entry) = rerun_archived_run(run_id, asr_model_id) {
                    rerun_raw_card_label.set_label(&entry.transcript_text);
                    rerun_raw_card_frame.set_visible(true);
                    if let Some(cleaned) = entry
                        .cleanup
                        .as_ref()
                        .and_then(|c| c.cleaned_text())
                    {
                        let original_text = original_cleaned_label.label();
                        let diff_markup = word_diff_markup(&original_text, cleaned);
                        rerun_cleaned_card_label.set_markup(&diff_markup);
                        rerun_cleaned_card_frame.set_visible(true);
                    }
                }
            });
        }
    } else {
        rerun_button.set_sensitive(false);
    }

    // Rerun cleanup button
    if let Some(rerun_cleanup) = rerun_cleanup {
        rerun_cleanup_button
            .set_sensitive(model.borrow().cleanup_rerunnable_run_id().is_some());
        {
            let model = model.clone();
            let cleanup_dropdown = cleanup_dropdown.clone();
            let cleanup_model_ids = cleanup_model_ids.clone();
            let prompt_text_view = prompt_text_view.clone();
            let rerun_cleaned_card_frame = rerun_cleaned_card.0.clone();
            let rerun_cleaned_card_label = rerun_cleaned_card.1.clone();
            let original_cleaned_label = original_cleaned_card.1.clone();
            let cleanup_timing_label = cleanup_timing_label.clone();
            rerun_cleanup_button.connect_clicked(move |button| {
                let run_id = {
                    let model = model.borrow();
                    model.cleanup_rerunnable_run_id().map(str::to_string)
                };
                let Some(run_id) = run_id else {
                    return;
                };
                let cleanup_model_id = cleanup_model_ids
                    .get(cleanup_dropdown.selected() as usize)
                    .cloned()
                    .unwrap_or_default();
                let buffer = prompt_text_view.buffer();
                let prompt_text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string();
                let custom_prompt = if prompt_text.chars().any(|c| !c.is_whitespace()) {
                    Some(prompt_text)
                } else {
                    None
                };

                // Run cleanup on a background thread to avoid blocking the UI.
                button.set_sensitive(false);
                button.set_label("Running...");
                let rerun_cleaned_card_label = rerun_cleaned_card_label.clone();
                let original_cleaned_label = original_cleaned_label.clone();
                let cleanup_timing_label = cleanup_timing_label.clone();
                let button = button.clone();
                let rerun_cleaned_card_frame = rerun_cleaned_card_frame.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::Builder::new()
                    .name("pepperx-cleanup-rerun".into())
                    .spawn(move || {
                        use crate::transcription::{
                            experiment_rerun_archived_cleanup, ArchivedCleanupRerunRequest,
                        };
                        let request = ArchivedCleanupRerunRequest {
                            run_id,
                            cleanup_model_id: Some(cleanup_model_id),
                            cleanup_prompt_profile: None,
                            custom_prompt_text: custom_prompt,
                        };
                        let result = experiment_rerun_archived_cleanup(request).ok();
                        let _ = tx.send(result);
                    })
                    .expect("failed to spawn cleanup rerun thread");
                gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    match rx.try_recv() {
                        Ok(result) => {
                            button.set_sensitive(true);
                            button.set_label("Run cleanup");
                            if let Some(entry) = result {
                                if let Some(cleanup) = entry.cleanup.as_ref() {
                                    if let Some(cleaned) = cleanup.cleaned_text() {
                                        let original_text = original_cleaned_label.label();
                                        let diff_markup =
                                            word_diff_markup(&original_text, cleaned);
                                        rerun_cleaned_card_label.set_markup(&diff_markup);
                                        rerun_cleaned_card_frame.set_visible(true);
                                    }
                                    let elapsed_secs = cleanup.elapsed_ms as f64 / 1000.0;
                                    cleanup_timing_label
                                        .set_label(&format!("{elapsed_secs:.1}s"));
                                    cleanup_timing_label.set_visible(true);
                                }
                            }
                            gtk::glib::ControlFlow::Break
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            gtk::glib::ControlFlow::Continue
                        }
                        Err(_) => {
                            button.set_sensitive(true);
                            button.set_label("Run cleanup");
                            gtk::glib::ControlFlow::Break
                        }
                    }
                });
            });
        }
    } else {
        rerun_cleanup_button.set_sensitive(false);
    }

    // =====================================================================
    // Row selection handler
    // =====================================================================
    {
        let model = model.clone();
        let metadata_line = metadata_line.clone();
        let transcription_subtitle = transcription_subtitle.clone();
        let original_raw_label = original_raw_card.1.clone();
        let cleanup_subtitle = cleanup_subtitle.clone();
        let original_cleaned_label = original_cleaned_card.1.clone();
        let rerun_button = rerun_button.clone();
        let rerun_cleanup_button = rerun_cleanup_button.clone();
        let play_button = play_button.clone();
        let asr_dropdown = asr_dropdown.clone();
        let asr_model_ids = asr_model_ids.clone();
        let cleanup_dropdown = cleanup_dropdown.clone();
        let cleanup_model_ids = cleanup_model_ids.clone();
        let diarization_container = diarization_container.clone();
        let use_ocr_check = use_ocr_check.clone();
        let rerun_raw_card_frame = rerun_raw_card.0.clone();
        let rerun_cleaned_card_frame = rerun_cleaned_card.0.clone();
        let cleanup_timing_label = cleanup_timing_label.clone();
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
            if let Some(run) = model.selected_run().cloned() {
                populate_detail_for_run(
                    &run,
                    &metadata_line,
                    &transcription_subtitle,
                    &original_raw_label,
                    &cleanup_subtitle,
                    &original_cleaned_label,
                    &diarization_container,
                    model.selected_recording_duration_secs(),
                    &use_ocr_check,
                );
            }
            // Hide rerun results when switching runs
            rerun_raw_card_frame.set_visible(false);
            rerun_cleaned_card_frame.set_visible(false);
            cleanup_timing_label.set_visible(false);

            rerun_button.set_sensitive(model.rerunnable_run_id().is_some());
            rerun_cleanup_button.set_sensitive(model.cleanup_rerunnable_run_id().is_some());
            play_button.set_sensitive(model.selected_wav_path().is_some());
            if let Some(asr_model) = model.selected_asr_model() {
                if let Some(pos) = asr_model_ids.iter().position(|id| id == asr_model) {
                    asr_dropdown.set_selected(pos as u32);
                }
            }
            if let Some(cleanup_model) = model.selected_cleanup_model() {
                if let Some(pos) = cleanup_model_ids.iter().position(|id| id == cleanup_model) {
                    cleanup_dropdown.set_selected(pos as u32);
                }
            }
        });
    }

    if let Some(first_row) = list_box.row_at_index(0) {
        list_box.select_row(Some(&first_row));
    }

    // =====================================================================
    // Paned: list on left, scrollable detail on right
    // =====================================================================
    let details_scroll = gtk::ScrolledWindow::new();
    details_scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    details_scroll.set_child(Some(&details_box));

    let list_scroll = gtk::ScrolledWindow::new();
    list_scroll.set_min_content_width(280);
    list_scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    list_scroll.set_child(Some(&list_box));

    let browser = gtk::Paned::new(Orientation::Horizontal);
    browser.set_wide_handle(true);
    browser.set_position(300);
    browser.set_start_child(Some(&list_scroll));
    browser.set_end_child(Some(&details_scroll));
    browser
}

// ---------------------------------------------------------------------------
// Populate detail widgets for a selected run
// ---------------------------------------------------------------------------

fn populate_detail_for_run(
    run: &ArchivedRun,
    metadata_line: &gtk::Label,
    transcription_subtitle: &gtk::Label,
    original_raw_label: &gtk::Label,
    cleanup_subtitle: &gtk::Label,
    original_cleaned_label: &gtk::Label,
    diarization_container: &gtk::Box,
    recording_duration_secs: f64,
    use_ocr_check: &gtk::CheckButton,
) {
    let entry = &run.entry;

    // Metadata line: "Apr 1, 2026 at 9:29 PM   2.9s   Parakeet v3 ...   Qwen 3.5 ..."
    let date_str = format_epoch_ms(run.archived_at_ms);
    let duration_str = if let Some(ms) = run.runtime_metadata.recording_elapsed_ms {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        "unknown duration".to_string()
    };
    let cleanup_model_part = entry
        .cleanup
        .as_ref()
        .map(|c| format!("   {}", c.model_name))
        .unwrap_or_default();
    metadata_line.set_label(&format!(
        "{}   {}   {}{}",
        date_str, duration_str, entry.model_name, cleanup_model_part
    ));

    // Transcription subtitle
    let asr_elapsed = entry.elapsed_ms;
    transcription_subtitle.set_label(&format!(
        "Originally transcribed with {} \u{2014} {}ms",
        entry.model_name, asr_elapsed
    ));

    // Original raw transcript
    original_raw_label.set_label(&entry.transcript_text);

    // Cleanup subtitle and cleaned text
    if let Some(cleanup) = entry.cleanup.as_ref() {
        let elapsed_str = format!("{:.1}s", cleanup.elapsed_ms as f64 / 1000.0);
        cleanup_subtitle.set_label(&format!(
            "Originally cleaned with {} \u{2014} {}",
            cleanup.model_name, elapsed_str
        ));
        original_cleaned_label.set_label(
            cleanup
                .cleaned_text()
                .unwrap_or("Cleanup failed for this run."),
        );
    } else {
        cleanup_subtitle.set_label("");
        original_cleaned_label.set_label("No cleanup transcript for this run.");
    }

    // Diarization timeline
    clear_diarization_container(diarization_container);
    if let Some(diarization) = entry.diarization.as_ref() {
        set_diarization_container(diarization_container, diarization, recording_duration_secs);
    }

    // OCR checkbox visibility
    use_ocr_check.set_visible(run.ocr_text.is_some());
    use_ocr_check.set_active(
        run.entry
            .cleanup
            .as_ref()
            .map(|c| c.used_ocr)
            .unwrap_or(false),
    );
}

// ---------------------------------------------------------------------------
// Text-only detail output (used by selected_details_text for tests)
// ---------------------------------------------------------------------------

pub(crate) fn history_run_details_text(run: &ArchivedRun) -> String {
    details_text_single(run)
}

fn details_text_single(run: &ArchivedRun) -> String {
    let entry = &run.entry;
    let mut details = format!("Original Raw Transcript:\n{}", entry.transcript_text);
    if let Some(cleaned_text) = entry
        .cleanup
        .as_ref()
        .and_then(|cleanup| cleanup.cleaned_text())
    {
        details.push_str(&format!("\n\nOriginal Cleaned Transcript:\n{cleaned_text}"));
    }
    let metadata = build_metadata_lines(run);
    details.push_str(&format!("\n\n{}", metadata.join("\n")));
    details
}

fn details_text_comparison(original_run: &ArchivedRun, rerun: &ArchivedRun) -> String {
    let original_cleaned_text = original_run
        .entry
        .cleanup
        .as_ref()
        .and_then(|cleanup| cleanup.cleaned_text())
        .unwrap_or("No cleanup transcript for this run.");
    let rerun_cleaned_text = rerun
        .entry
        .cleanup
        .as_ref()
        .and_then(|cleanup| cleanup.cleaned_text())
        .unwrap_or("No cleanup transcript for this run.");

    let raw_text = format!(
        "Original raw transcript:\n{}\n\nRerun raw transcript:\n{}",
        original_run.entry.transcript_text, rerun.entry.transcript_text
    );
    let cleaned_text = format!(
        "Original cleaned transcript:\n{}\n\nRerun cleaned transcript:\n{}",
        original_cleaned_text, rerun_cleaned_text
    );
    let metadata = comparison_metadata_lines(original_run, rerun);

    let mut details = format!("Original Raw Transcript:\n{raw_text}");
    details.push_str(&format!("\n\nOriginal Cleaned Transcript:\n{cleaned_text}"));
    details.push_str(&format!("\n\n{}", metadata.join("\n")));
    details
}

fn build_metadata_lines(run: &ArchivedRun) -> Vec<String> {
    let entry = &run.entry;
    let mut lines = vec![
        format!("Run ID: {}", run.run_id),
        format!("ASR backend: {}", entry.backend_name),
        format!("ASR model: {}", entry.model_name),
        format!("ASR time: {} ms", entry.elapsed_ms),
    ];

    if let Some(prompt_profile) = run.prompt_profile.as_deref() {
        lines.push(format!("Cleanup prompt profile: {prompt_profile}"));
    }

    if let Some(cleanup) = entry.cleanup.as_ref() {
        lines.push(format!("Cleanup backend: {}", cleanup.backend_name));
        lines.push(format!("Cleanup model: {}", cleanup.model_name));
        if cleanup.succeeded {
            lines.push(format!("Cleanup time: {} ms", cleanup.elapsed_ms));
        } else if let Some(reason) = cleanup.failure_reason.as_deref() {
            lines.push(format!("Cleanup failure: {reason}"));
        }
        lines.push(format!("OCR used: {}", cleanup.used_ocr));
    }

    if let Some(insertion) = entry.insertion.as_ref() {
        lines.push(format!("Insertion backend: {}", insertion.backend_name));
        lines.push(format!(
            "Insertion target: {}",
            insertion.target_application_name
        ));
        if let Some(target_class) = insertion.target_class.as_deref() {
            lines.push(format!("Target class: {target_class}"));
        }
        if !insertion.succeeded {
            lines.push(format!(
                "Insertion failure: {}",
                insertion
                    .failure_reason
                    .as_deref()
                    .unwrap_or("unknown insertion failure")
            ));
        }
    }

    if let Some(supporting_context_text) = run.supporting_context_text.as_deref() {
        lines.push(format!("Supporting context: {}", supporting_context_text));
    }

    if let Some(ocr_text) = run.ocr_text.as_deref() {
        lines.push(format!("OCR text: {ocr_text}"));
    }

    if let Some(diarization) = entry.diarization.as_ref() {
        let speaker_count = diarization.distinct_speakers().len();
        lines.push(format!(
            "Diarization: {} speaker{}, {} segment{}",
            speaker_count,
            if speaker_count == 1 { "" } else { "s" },
            diarization.segments.len(),
            if diarization.segments.len() == 1 {
                ""
            } else {
                "s"
            },
        ));
        if let Some(ref target) = diarization.target_speaker {
            lines.push(format!("Target speaker: {target}"));
        }
        if diarization.filtering_used {
            lines.push("Speaker filtering: enabled".into());
        }
        if let Some(ref reason) = diarization.fallback_reason {
            lines.push(format!("Speaker filtering fallback: {reason}"));
        }
    }

    lines
}

fn comparison_metadata_lines(original_run: &ArchivedRun, rerun: &ArchivedRun) -> Vec<String> {
    let original_cleanup_model = original_run
        .entry
        .cleanup
        .as_ref()
        .map(|cleanup| cleanup.model_name.as_str())
        .unwrap_or("none");
    let rerun_cleanup_model = rerun
        .entry
        .cleanup
        .as_ref()
        .map(|cleanup| cleanup.model_name.as_str())
        .unwrap_or("none");
    let original_cleanup_failure = original_run
        .entry
        .cleanup
        .as_ref()
        .and_then(|cleanup| cleanup.failure_reason.as_deref());
    let rerun_cleanup_failure = rerun
        .entry
        .cleanup
        .as_ref()
        .and_then(|cleanup| cleanup.failure_reason.as_deref());
    let original_insertion_failure = original_run
        .entry
        .insertion
        .as_ref()
        .and_then(|insertion| insertion.failure_reason.as_deref());
    let rerun_insertion_failure = rerun
        .entry
        .insertion
        .as_ref()
        .and_then(|insertion| insertion.failure_reason.as_deref());
    let original_prompt_profile = original_run.prompt_profile.as_deref().unwrap_or("none");
    let rerun_prompt_profile = rerun.prompt_profile.as_deref().unwrap_or("none");

    let mut lines = vec![
        format!("Original run ID: {}", original_run.run_id),
        format!("Rerun run ID: {}", rerun.run_id),
        format!(
            "ASR model: {} -> {}",
            original_run.entry.model_name, rerun.entry.model_name
        ),
        format!(
            "Cleanup model: {} -> {}",
            original_cleanup_model, rerun_cleanup_model
        ),
        format!(
            "Cleanup prompt profile: {} -> {}",
            original_prompt_profile, rerun_prompt_profile
        ),
        format!(
            "Original OCR used: {}",
            original_run
                .entry
                .cleanup
                .as_ref()
                .map(|cleanup| cleanup.used_ocr)
                .unwrap_or(false)
        ),
        format!(
            "Rerun OCR used: {}",
            rerun
                .entry
                .cleanup
                .as_ref()
                .map(|cleanup| cleanup.used_ocr)
                .unwrap_or(false)
        ),
    ];

    if let Some(reason) = original_cleanup_failure {
        lines.push(format!("Original cleanup failure: {reason}"));
    }
    if let Some(reason) = rerun_cleanup_failure {
        lines.push(format!("Rerun cleanup failure: {reason}"));
    }
    if let Some(reason) = original_insertion_failure {
        lines.push(format!("Original insertion failure: {reason}"));
    }
    if let Some(reason) = rerun_insertion_failure {
        lines.push(format!("Rerun insertion failure: {reason}"));
    }
    if let Some(text) = original_run.supporting_context_text.as_deref() {
        lines.push(format!("Original supporting context: {text}"));
    }
    if let Some(text) = rerun.supporting_context_text.as_deref() {
        lines.push(format!("Rerun supporting context: {text}"));
    }
    if let Some(text) = original_run.ocr_text.as_deref() {
        lines.push(format!("Original OCR text: {text}"));
    }
    if let Some(text) = rerun.ocr_text.as_deref() {
        lines.push(format!("Rerun OCR text: {text}"));
    }

    lines
}

// ---------------------------------------------------------------------------
// Dark card widget helpers
// ---------------------------------------------------------------------------

/// Create a dark-styled card (Frame with "view" class) containing a plain Label.
/// Returns (Frame, Label) so the label text can be updated later.
fn dark_card_label(text: &str) -> (gtk::Frame, gtk::Label) {
    let frame = gtk::Frame::new(None);
    frame.add_css_class("view");
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    frame.set_child(Some(&label));
    (frame, label)
}

/// Create a dark-styled card containing a Label that uses Pango markup.
/// Returns (Frame, Label) so the markup can be updated later.
fn dark_card_markup(markup: &str) -> (gtk::Frame, gtk::Label) {
    let frame = gtk::Frame::new(None);
    frame.add_css_class("view");
    let label = gtk::Label::builder()
        .use_markup(true)
        .label(markup)
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    frame.set_child(Some(&label));
    (frame, label)
}

// ---------------------------------------------------------------------------
// Word-level diff with LCS for Pango markup highlighting
// ---------------------------------------------------------------------------

/// Compute longest common subsequence table for two word slices.
fn lcs_table(old_words: &[&str], new_words: &[&str]) -> Vec<Vec<usize>> {
    let m = old_words.len();
    let n = new_words.len();
    let mut table = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if old_words[i - 1] == new_words[j - 1] {
                table[i][j] = table[i - 1][j - 1] + 1;
            } else {
                table[i][j] = table[i - 1][j].max(table[i][j - 1]);
            }
        }
    }
    table
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffOp<'a> {
    Equal(&'a str),
    Removed(&'a str),
    Added(&'a str),
}

/// Produce a word-level diff between old_text and new_text.
fn word_diff<'a>(old_text: &'a str, new_text: &'a str) -> Vec<DiffOp<'a>> {
    let old_words: Vec<&str> = old_text.split_whitespace().collect();
    let new_words: Vec<&str> = new_text.split_whitespace().collect();
    let table = lcs_table(&old_words, &new_words);

    let mut ops = Vec::new();
    let mut i = old_words.len();
    let mut j = new_words.len();

    while i > 0 && j > 0 {
        if old_words[i - 1] == new_words[j - 1] {
            ops.push(DiffOp::Equal(old_words[i - 1]));
            i -= 1;
            j -= 1;
        } else if table[i - 1][j] >= table[i][j - 1] {
            ops.push(DiffOp::Removed(old_words[i - 1]));
            i -= 1;
        } else {
            ops.push(DiffOp::Added(new_words[j - 1]));
            j -= 1;
        }
    }
    while i > 0 {
        ops.push(DiffOp::Removed(old_words[i - 1]));
        i -= 1;
    }
    while j > 0 {
        ops.push(DiffOp::Added(new_words[j - 1]));
        j -= 1;
    }
    ops.reverse();
    ops
}

/// Generate Pango markup showing word-level diff between original and rerun text.
/// Removed words: red strikethrough. Added words: green. Unchanged: plain.
fn word_diff_markup(old_text: &str, new_text: &str) -> String {
    let ops = word_diff(old_text, new_text);
    let mut parts = Vec::new();
    for op in &ops {
        match op {
            DiffOp::Equal(word) => {
                parts.push(glib::markup_escape_text(word).to_string());
            }
            DiffOp::Removed(word) => {
                let escaped = glib::markup_escape_text(word);
                parts.push(format!(
                    "<span strikethrough=\"true\" foreground=\"#e01b24\">{escaped}</span>"
                ));
            }
            DiffOp::Added(word) => {
                let escaped = glib::markup_escape_text(word);
                parts.push(format!(
                    "<span foreground=\"#2ec27e\">{escaped}</span>"
                ));
            }
        }
    }
    parts.join(" ")
}

// ---------------------------------------------------------------------------
// Timestamp formatting (no chrono dependency)
// ---------------------------------------------------------------------------

fn format_epoch_ms(epoch_ms: u64) -> String {
    const MONTHS: &[&str] = &[
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const DAYS_IN_MONTH: &[u64] = &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    fn is_leap_year(year: u64) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    fn days_in_year(year: u64) -> u64 {
        if is_leap_year(year) { 366 } else { 365 }
    }

    fn days_in_month(year: u64, month: usize) -> u64 {
        if month == 1 && is_leap_year(year) {
            29
        } else {
            DAYS_IN_MONTH[month]
        }
    }

    let total_secs = epoch_ms / 1000;
    let mut remaining_days = total_secs / 86400;
    let day_secs = total_secs % 86400;
    let hour = day_secs / 3600;
    let minute = (day_secs % 3600) / 60;

    let mut year = 1970u64;
    loop {
        let dy = days_in_year(year);
        if remaining_days < dy {
            break;
        }
        remaining_days -= dy;
        year += 1;
    }

    let mut month = 0usize;
    loop {
        let dm = days_in_month(year, month);
        if remaining_days < dm {
            break;
        }
        remaining_days -= dm;
        month += 1;
        if month >= 12 {
            break;
        }
    }
    let day = remaining_days + 1;

    let (hour12, ampm) = if hour == 0 {
        (12, "AM")
    } else if hour < 12 {
        (hour, "AM")
    } else if hour == 12 {
        (12, "PM")
    } else {
        (hour - 12, "PM")
    };

    format!(
        "{} {}, {} at {}:{:02} {}",
        MONTHS[month], day, year, hour12, minute, ampm
    )
}

// ---------------------------------------------------------------------------
// Diarization timeline (unchanged from original)
// ---------------------------------------------------------------------------

const SPEAKER_COLORS: &[(f64, f64, f64)] = &[
    (0.204, 0.396, 0.643), // blue
    (0.839, 0.373, 0.176), // orange
    (0.173, 0.627, 0.173), // green
    (0.784, 0.212, 0.212), // red
    (0.580, 0.404, 0.741), // purple
    (0.549, 0.337, 0.294), // brown
    (0.890, 0.467, 0.761), // pink
    (0.498, 0.498, 0.498), // grey
];

pub(crate) fn build_diarization_timeline(
    summary: &DiarizationSummary,
    total_duration_secs: f64,
) -> gtk::Widget {
    let container = gtk::Box::new(Orientation::Vertical, 6);

    let header_text = if summary.filtering_used {
        if let Some(ref fallback_reason) = summary.fallback_reason {
            format!("Speaker Diarization (fallback: {fallback_reason})")
        } else {
            "Speaker Diarization".to_string()
        }
    } else {
        "Speaker Diarization (no filtering)".to_string()
    };
    let header = gtk::Label::builder()
        .label(&header_text)
        .xalign(0.0)
        .css_classes(["title-5"])
        .build();
    container.append(&header);

    if summary.segments.is_empty() || total_duration_secs <= 0.0 {
        let empty_label = gtk::Label::builder()
            .label("No diarization segments recorded.")
            .xalign(0.0)
            .build();
        container.append(&empty_label);
        return container.upcast();
    }

    let distinct_speakers = summary.distinct_speakers();
    let color_map: Vec<(&str, (f64, f64, f64))> = distinct_speakers
        .iter()
        .enumerate()
        .map(|(index, &speaker)| (speaker, SPEAKER_COLORS[index % SPEAKER_COLORS.len()]))
        .collect();

    let timeline_height = 32;
    let drawing_area = gtk::DrawingArea::builder()
        .height_request(timeline_height)
        .hexpand(true)
        .build();

    let segments = summary.segments.clone();
    let target_speaker = summary.target_speaker.clone();
    let color_map_for_draw: Vec<(String, (f64, f64, f64))> = color_map
        .iter()
        .map(|(speaker, color)| (speaker.to_string(), *color))
        .collect();

    drawing_area.set_draw_func(move |_area, cr, width, height| {
        let width = width as f64;
        let height = height as f64;

        cr.set_source_rgb(0.15, 0.15, 0.15);
        let _ = cr.paint();

        for segment in &segments {
            let x_start = (segment.start_secs / total_duration_secs) * width;
            let x_end = (segment.end_secs / total_duration_secs) * width;
            let segment_width = (x_end - x_start).max(1.0);

            let (r, g, b) = color_map_for_draw
                .iter()
                .find(|(speaker, _)| *speaker == segment.speaker)
                .map(|(_, color)| *color)
                .unwrap_or((0.5, 0.5, 0.5));

            cr.set_source_rgb(r, g, b);
            cr.rectangle(x_start, 0.0, segment_width, height);
            let _ = cr.fill();
        }

        if let Some(ref target) = target_speaker {
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.set_line_width(1.5);
            for segment in &segments {
                if segment.speaker == *target {
                    let x_start = (segment.start_secs / total_duration_secs) * width;
                    let x_end = (segment.end_secs / total_duration_secs) * width;
                    let segment_width = (x_end - x_start).max(1.0);
                    cr.rectangle(x_start, 0.0, segment_width, height);
                    let _ = cr.stroke();
                }
            }
        }
    });
    container.append(&drawing_area);

    let legend_box = gtk::Box::new(Orientation::Horizontal, 12);
    legend_box.set_margin_top(2);
    for (speaker, (r, g, b)) in &color_map {
        let swatch = gtk::DrawingArea::builder()
            .width_request(14)
            .height_request(14)
            .build();
        let (r, g, b) = (*r, *g, *b);
        swatch.set_draw_func(move |_area, cr, width, height| {
            cr.set_source_rgb(r, g, b);
            cr.rectangle(0.0, 0.0, width as f64, height as f64);
            let _ = cr.fill();
        });

        let is_target = summary
            .target_speaker
            .as_deref()
            .is_some_and(|target| target == *speaker);
        let label_text = if is_target {
            format!("{speaker} (you)")
        } else {
            speaker.to_string()
        };
        let label = gtk::Label::builder()
            .label(&label_text)
            .xalign(0.0)
            .build();

        let legend_item = gtk::Box::new(Orientation::Horizontal, 4);
        legend_item.append(&swatch);
        legend_item.append(&label);
        legend_box.append(&legend_item);
    }
    container.append(&legend_box);

    container.upcast()
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

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
    format!("{} \u{2022} {} ms", run.entry.model_name, run.entry.elapsed_ms)
}

fn picker_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["caption"])
        .build()
}

fn section_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["title-4"])
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

fn clear_diarization_container(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn set_diarization_container(
    container: &gtk::Box,
    diarization: &DiarizationSummary,
    recording_duration_secs: f64,
) {
    clear_diarization_container(container);
    let segment_max = diarization
        .segments
        .iter()
        .map(|seg| seg.end_secs)
        .fold(0.0_f64, f64::max);
    let total_duration = if recording_duration_secs > 0.0 {
        recording_duration_secs
    } else {
        segment_max
    };
    let timeline = build_diarization_timeline(diarization, total_duration);
    container.append(&timeline);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod history_view_tests {
    use super::*;
    use crate::history_store::{ArchivedRun, RunRuntimeMetadata};
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
            "parakeet-rs",
            "nemotron-speech-streaming-en-0.6b",
            Duration::from_millis(42),
        );
        if let Some(cleaned_transcript) = cleaned_transcript {
            entry.cleanup = Some(CleanupDiagnostics::succeeded(
                "llama.cpp",
                "qwen3.5-2b-q4_k_m.gguf",
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
            runtime_metadata: RunRuntimeMetadata::wav_import(),
            archived_source_wav_path: Some(PathBuf::from(format!(
                "/tmp/history/{run_id}/source.wav"
            ))),
            parent_run_id: None,
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

        assert!(details.contains("Original Raw Transcript:\nhello from pepper x"));
        assert!(details.contains("Original Cleaned Transcript:\nHello from Pepper X."));
        assert!(details.contains("ASR model: nemotron-speech-streaming-en-0.6b"));
        assert!(details.contains("Cleanup model: qwen3.5-2b-q4_k_m.gguf"));
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

    #[test]
    fn history_view_selected_rerun_compares_against_parent() {
        let parent = archived_run(
            "run-parent",
            10,
            "hello from pepper x",
            Some("Hello from Pepper X."),
            "atspi-editable-text",
        );
        let mut rerun = archived_run(
            "run-rerun",
            20,
            "hello from pepper ex",
            Some("Hello from Pepper Ex."),
            "clipboard-paste",
        );
        rerun.parent_run_id = Some(parent.run_id.clone());
        rerun.prompt_profile = Some("literal-dictation".into());
        rerun.entry.model_name = "nemo-parakeet-tdt-0.6b-v3-int8".into();
        rerun.entry.cleanup.as_mut().expect("cleanup").model_name =
            "qwen3.5-0.8b-q4_k_m.gguf".into();

        let model = HistoryBrowserModel::new(vec![parent, rerun]);
        let details = model
            .selected_details_text()
            .expect("selected details text");

        assert!(details.contains("Original raw transcript:\nhello from pepper x"));
        assert!(details.contains("Rerun raw transcript:\nhello from pepper ex"));
        assert!(details.contains(
            "Cleanup model: qwen3.5-2b-q4_k_m.gguf -> qwen3.5-0.8b-q4_k_m.gguf"
        ));
        assert!(details.contains("Cleanup prompt profile: ordinary-dictation -> literal-dictation"));
    }

    #[test]
    fn history_view_selected_parent_compares_against_newest_rerun() {
        let parent = archived_run(
            "run-parent",
            10,
            "hello from pepper x",
            Some("Hello from Pepper X."),
            "atspi-editable-text",
        );
        let mut older_rerun = archived_run(
            "run-rerun-older",
            20,
            "older rerun raw",
            Some("Older rerun cleaned."),
            "clipboard-paste",
        );
        older_rerun.parent_run_id = Some(parent.run_id.clone());
        let mut newer_rerun = archived_run(
            "run-rerun-newer",
            30,
            "newer rerun raw",
            Some("Newer rerun cleaned."),
            "clipboard-paste",
        );
        newer_rerun.parent_run_id = Some(parent.run_id.clone());

        let mut model = HistoryBrowserModel::new(vec![parent, older_rerun, newer_rerun]);
        assert!(model.select_run("run-parent"));
        let details = model
            .selected_details_text()
            .expect("selected details text");

        assert!(details.contains("Original raw transcript:\nhello from pepper x"));
        assert!(details.contains("Rerun raw transcript:\nnewer rerun raw"));
    }

    #[test]
    fn history_view_rerun_action_targets_the_logical_parent_run() {
        let parent = archived_run(
            "run-parent",
            10,
            "hello from pepper x",
            Some("Hello from Pepper X."),
            "atspi-editable-text",
        );
        let mut rerun = archived_run(
            "run-rerun",
            20,
            "hello from pepper ex",
            Some("Hello from Pepper Ex."),
            "clipboard-paste",
        );
        rerun.parent_run_id = Some(parent.run_id.clone());

        let mut model = HistoryBrowserModel::new(vec![parent, rerun]);
        assert_eq!(model.rerunnable_run_id(), Some("run-parent"));

        assert!(model.select_run("run-rerun"));
        assert_eq!(model.rerunnable_run_id(), Some("run-parent"));
    }

    #[test]
    fn history_view_rerun_comparison_keeps_per_run_diagnostics_visible() {
        let mut parent = archived_run(
            "run-parent",
            10,
            "hello from pepper x",
            Some("Hello from Pepper X."),
            "atspi-editable-text",
        );
        parent.supporting_context_text = Some("line before".into());
        parent.ocr_text = Some("ocr parent".into());
        parent.entry.cleanup.as_mut().expect("cleanup").used_ocr = true;

        let mut rerun = archived_run(
            "run-rerun",
            20,
            "hello from pepper ex",
            None,
            "clipboard-paste",
        );
        rerun.parent_run_id = Some(parent.run_id.clone());
        rerun.prompt_profile = Some("literal-dictation".into());
        rerun.supporting_context_text = Some("line after".into());
        rerun.ocr_text = Some("ocr rerun".into());
        rerun.entry.cleanup = Some(CleanupDiagnostics::failed(
            "llama.cpp",
            "qwen3.5-2b-q4_k_m.gguf",
            "cleanup timed out",
        ));
        rerun.entry.insertion = Some(InsertionDiagnostics::failed(
            "uinput-text",
            "xterm",
            "paste backend was unavailable",
        ));

        let model = HistoryBrowserModel::new(vec![parent, rerun]);
        let details = model
            .selected_details_text()
            .expect("selected details text");

        assert!(details.contains("Original OCR used: true"));
        assert!(details.contains("Original supporting context: line before"));
        assert!(details.contains("Original OCR text: ocr parent"));
        assert!(details.contains("Rerun cleanup failure: cleanup timed out"));
        assert!(details.contains("Rerun insertion failure: paste backend was unavailable"));
        assert!(details.contains("Rerun supporting context: line after"));
        assert!(details.contains("Rerun OCR text: ocr rerun"));
    }

    #[test]
    fn history_view_cleanup_rerun_targets_the_selected_run_directly() {
        let parent = archived_run(
            "run-parent",
            10,
            "hello from pepper x",
            Some("Hello from Pepper X."),
            "atspi-editable-text",
        );
        let mut rerun = archived_run(
            "run-rerun",
            20,
            "hello from pepper ex",
            Some("Hello from Pepper Ex."),
            "clipboard-paste",
        );
        rerun.parent_run_id = Some(parent.run_id.clone());

        let mut model = HistoryBrowserModel::new(vec![parent, rerun]);
        assert_eq!(model.cleanup_rerunnable_run_id(), Some("run-rerun"));

        assert!(model.select_run("run-parent"));
        assert_eq!(model.cleanup_rerunnable_run_id(), Some("run-parent"));
    }

    #[test]
    fn history_view_selected_wav_path_returns_none_when_file_does_not_exist() {
        let run = archived_run(
            "run-1",
            20,
            "hello from pepper x",
            Some("Hello from Pepper X."),
            "atspi-editable-text",
        );
        let model = HistoryBrowserModel::new(vec![run]);
        assert!(model.selected_wav_path().is_none());
    }

    #[test]
    fn history_view_selected_wav_path_returns_path_when_file_exists() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "pepper-x-wav-path-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let wav_path = tmp_dir.join("source.wav");
        std::fs::write(&wav_path, b"fake-wav").unwrap();

        let mut run = archived_run(
            "run-1",
            20,
            "hello from pepper x",
            Some("Hello from Pepper X."),
            "atspi-editable-text",
        );
        run.archived_source_wav_path = Some(wav_path.clone());

        let model = HistoryBrowserModel::new(vec![run]);
        assert_eq!(model.selected_wav_path(), Some(wav_path));
        let _ = std::fs::remove_dir_all(tmp_dir);
    }

    #[test]
    fn history_view_selected_wav_path_returns_none_when_no_wav_archived() {
        let mut run = archived_run(
            "run-1",
            20,
            "hello from pepper x",
            Some("Hello from Pepper X."),
            "atspi-editable-text",
        );
        run.archived_source_wav_path = None;

        let model = HistoryBrowserModel::new(vec![run]);
        assert!(model.selected_wav_path().is_none());
    }

    #[test]
    fn word_diff_identical_texts_produces_all_equal() {
        let ops = word_diff("hello world", "hello world");
        assert_eq!(ops, vec![DiffOp::Equal("hello"), DiffOp::Equal("world")]);
    }

    #[test]
    fn word_diff_added_word_highlighted() {
        let ops = word_diff("hello world", "hello brave world");
        assert!(ops.contains(&DiffOp::Added("brave")));
        assert!(ops.contains(&DiffOp::Equal("hello")));
        assert!(ops.contains(&DiffOp::Equal("world")));
    }

    #[test]
    fn word_diff_removed_word_highlighted() {
        let ops = word_diff("hello brave world", "hello world");
        assert!(ops.contains(&DiffOp::Removed("brave")));
    }

    #[test]
    fn word_diff_markup_produces_pango_spans() {
        let markup = word_diff_markup("hello world", "hello brave world");
        assert!(markup.contains("hello"));
        assert!(markup.contains("<span foreground=\"#2ec27e\">brave</span>"));
        assert!(markup.contains("world"));
    }

    #[test]
    fn format_epoch_ms_renders_readable_date() {
        // 2026-01-01 00:00:00 UTC = 1767225600 seconds
        let formatted = format_epoch_ms(1767225600_000);
        assert!(formatted.contains("Jan"));
        assert!(formatted.contains("2026"));
    }
}
