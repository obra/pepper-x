use adw::prelude::*;
use gtk::glib::translate::IntoGlib;
use gtk::Align;
use pepperx_audio::{MicrophoneDevice, SelectedMicrophone};
use pepperx_platform_gnome::evdev_modifier_capture::{
    gdk_keyval_to_evdev, trigger_keys_display_name, SharedTriggerConfig, TriggerConfig,
};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::app_model::SettingsSurfaceState;
use crate::settings::{
    corrections_store_path, load_microphone_ui_state, save_launch_at_login,
    save_preferred_microphone, AppSettings, MicrophoneUiState,
};
use crate::transcript_log::TranscriptEntry;
use pepperx_corrections::CorrectionStore;
use pepperx_models::{supported_models, ModelKind};

const PROMPT_PROFILE_OPTIONS: [&str; 2] = ["ordinary-dictation", "literal-dictation"];

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSwitchControl {
    pub title: String,
    pub subtitle: String,
    pub active: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSelectControl {
    pub title: String,
    pub subtitle: String,
    pub selected: String,
    pub options: Vec<String>,
    pub enabled: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTextAreaControl {
    pub title: String,
    pub subtitle: String,
    pub text: String,
    pub enabled: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsShortcutRecorderControl {
    pub title: String,
    pub subtitle: String,
    pub current_shortcut: String,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsControl {
    Switch(SettingsSwitchControl),
    Select(SettingsSelectControl),
    TextArea(SettingsTextAreaControl),
    ShortcutRecorder(SettingsShortcutRecorderControl),
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFormSection {
    pub title: String,
    pub controls: Vec<SettingsControl>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsContainerKind {
    Form,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsPageScaffold {
    pub container_kind: SettingsContainerKind,
    pub sections: Vec<SettingsFormSection>,
    pub feedback_message: Option<String>,
}

pub struct SettingsView {
    root: gtk::Box,
    asr_model_dropdown: gtk::DropDown,
    asr_model_ids: Vec<String>,
    cleanup_switch: gtk::Switch,
    cleanup_model_dropdown: gtk::DropDown,
    cleanup_model_ids: Vec<String>,
    prompt_profile_dropdown: gtk::DropDown,
    custom_prompt_buffer: gtk::TextBuffer,
    custom_prompt_view: gtk::TextView,
    reset_custom_prompt_button: gtk::Button,
    window_context_switch: gtk::Switch,
    hold_trigger_button: gtk::Button,
    hold_trigger_value: Rc<RefCell<String>>,
    toggle_trigger_button: gtk::Button,
    toggle_trigger_value: Rc<RefCell<String>>,
    preferred_transcriptions_buffer: gtk::TextBuffer,
    replacement_rules_buffer: gtk::TextBuffer,
    clear_corrections_button: gtk::Button,
    launch_at_login_switch: gtk::Switch,
    play_sounds_switch: gtk::Switch,
    ignore_other_speakers_switch: gtk::Switch,
    post_paste_learning_switch: gtk::Switch,
    test_dictation_button: gtk::Button,
    test_dictation_status_label: gtk::Label,
    test_dictation_result_label: gtk::Label,
    model_status_label: gtk::Label,
    feedback_label: gtk::Label,
    updating_settings: Rc<Cell<bool>>,
    history_container: gtk::Box,
    diagnostics_view: crate::diagnostics_view::DiagnosticsView,
    shared_trigger_config: Option<SharedTriggerConfig>,
}

impl SettingsView {
    pub fn new(surface_state: SettingsSurfaceState) -> Self {
        Self::new_with_extras(surface_state, None, None, None, None, String::new(), None)
    }

    pub fn new_with_extras(
        surface_state: SettingsSurfaceState,
        history_widget: Option<gtk::Widget>,
        _rerun_archived_run: Option<Rc<dyn Fn(String, String) -> Option<TranscriptEntry>>>,
        _rerun_cleanup: Option<Rc<dyn Fn(String, String, Option<String>) -> Option<TranscriptEntry>>>,
        play_audio: Option<Rc<dyn Fn(std::path::PathBuf)>>,
        diagnostics_summary: String,
        shared_trigger_config: Option<SharedTriggerConfig>,
    ) -> Self {
        // -- Outer shell: horizontal box with sidebar on the left, stack on the right --
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);

        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_hexpand(true);
        stack.set_vexpand(true);

        let sidebar = gtk::StackSidebar::new();
        sidebar.set_stack(&stack);
        sidebar.set_size_request(160, -1);

        // ===== 1. Recording page =====
        let recording_page = settings_page_box();

        // -- Shortcuts card --
        let shortcuts_label = section_label("Shortcuts");
        recording_page.append(&shortcuts_label);
        let trigger_list = gtk::ListBox::new();
        trigger_list.add_css_class("boxed-list");

        let hold_initial_label =
            trigger_keys_display_name(&surface_state.hold_trigger_keys);
        let hold_trigger_button = gtk::Button::builder()
            .label(&hold_initial_label)
            .hexpand(true)
            .build();
        hold_trigger_button.set_focusable(true);
        let hold_trigger_value = Rc::new(RefCell::new(
            surface_state.hold_trigger_keys.clone(),
        ));
        let hold_trigger_row = adw::ActionRow::builder()
            .title("Hold to Record")
            .subtitle("Hold this key combination to record, release to stop")
            .build();
        hold_trigger_row.add_suffix(&hold_trigger_button);
        trigger_list.append(&list_box_row(&hold_trigger_row));

        let toggle_initial_label =
            trigger_keys_display_name(&surface_state.toggle_trigger_keys);
        let toggle_trigger_button = gtk::Button::builder()
            .label(&toggle_initial_label)
            .hexpand(true)
            .build();
        toggle_trigger_button.set_focusable(true);
        let toggle_trigger_value = Rc::new(RefCell::new(
            surface_state.toggle_trigger_keys.clone(),
        ));
        let toggle_trigger_row = adw::ActionRow::builder()
            .title("Toggle Recording")
            .subtitle("Press once to start recording, press again to stop")
            .build();
        toggle_trigger_row.add_suffix(&toggle_trigger_button);
        trigger_list.append(&list_box_row(&toggle_trigger_row));
        recording_page.append(&trigger_list);

        // -- Input card (microphone picker + sound check + sound effects) --
        let input_label = section_label("Input");
        recording_page.append(&input_label);
        let input_list = gtk::ListBox::new();
        input_list.add_css_class("boxed-list");

        let play_sounds_switch = gtk::Switch::builder().valign(Align::Center).build();
        let play_sounds_row = adw::ActionRow::builder()
            .title("Sound effects")
            .subtitle("Play a sound when recording starts and stops")
            .activatable_widget(&play_sounds_switch)
            .build();
        play_sounds_row.add_suffix(&play_sounds_switch);
        input_list.append(&list_box_row(&play_sounds_row));

        let ignore_other_speakers_switch = gtk::Switch::builder().valign(Align::Center).build();
        let ignore_other_speakers_row = adw::ActionRow::builder()
            .title("Ignore other speakers")
            .subtitle("Filter out other voices during transcription (experimental)")
            .activatable_widget(&ignore_other_speakers_switch)
            .build();
        ignore_other_speakers_row.add_suffix(&ignore_other_speakers_switch);
        input_list.append(&list_box_row(&ignore_other_speakers_row));
        recording_page.append(&input_list);

        let microphone_controls = build_microphone_controls("Microphone", "Check");
        recording_page.append(&microphone_controls);

        // -- Test Dictation card --
        let test_dictation_label = section_label("Test Dictation");
        recording_page.append(&test_dictation_label);
        let test_dictation_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let test_dictation_button = gtk::Button::builder()
            .label("Start Test")
            .css_classes(["suggested-action"])
            .halign(Align::Start)
            .build();
        let test_dictation_status_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .visible(false)
            .build();
        let test_dictation_result_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .visible(false)
            .build();
        test_dictation_box.append(&test_dictation_button);
        test_dictation_box.append(&test_dictation_status_label);
        test_dictation_box.append(&test_dictation_result_label);
        recording_page.append(&test_dictation_box);

        let recording_scroller = scrollable_page(&recording_page);
        stack.add_titled(&recording_scroller, Some("recording"), "Recording");

        // ===== 2. Cleanup page =====
        let cleanup_page = settings_page_box();

        // -- Cleanup card --
        let cleanup_card_label = section_label("Cleanup");
        cleanup_page.append(&cleanup_card_label);
        let cleanup_list = gtk::ListBox::new();
        cleanup_list.add_css_class("boxed-list");

        let cleanup_switch = gtk::Switch::builder().valign(Align::Center).build();
        let cleanup_switch_row = adw::ActionRow::builder()
            .title("Enable cleanup")
            .subtitle("Run the local cleanup model after transcription when it is ready")
            .activatable_widget(&cleanup_switch)
            .build();
        cleanup_switch_row.add_suffix(&cleanup_switch);
        cleanup_list.append(&list_box_row(&cleanup_switch_row));

        let window_context_switch = gtk::Switch::builder().valign(Align::Center).build();
        let window_context_row = adw::ActionRow::builder()
            .title("Window context")
            .subtitle(
                "Capture screen text to help the cleanup model disambiguate names and terms",
            )
            .activatable_widget(&window_context_switch)
            .build();
        window_context_row.add_suffix(&window_context_switch);
        cleanup_list.append(&list_box_row(&window_context_row));
        cleanup_page.append(&cleanup_list);

        // -- Cleanup Prompt card --
        let prompt_card_label = section_label("Cleanup Prompt");
        cleanup_page.append(&prompt_card_label);
        let prompt_list = gtk::ListBox::new();
        prompt_list.add_css_class("boxed-list");

        let prompt_profile_model = gtk::StringList::new(&PROMPT_PROFILE_OPTIONS);
        let prompt_profile_dropdown =
            gtk::DropDown::new(Some(prompt_profile_model.clone()), None::<gtk::Expression>);
        prompt_profile_dropdown.set_hexpand(true);
        let prompt_profile_row = adw::ActionRow::builder()
            .title("Prompt profile")
            .subtitle("Choose the baseline cleanup behavior used when the custom prompt is empty")
            .activatable_widget(&prompt_profile_dropdown)
            .build();
        prompt_profile_row.add_suffix(&prompt_profile_dropdown);
        prompt_list.append(&list_box_row(&prompt_profile_row));
        cleanup_page.append(&prompt_list);

        let custom_prompt_title = gtk::Label::builder()
            .label("Custom cleanup prompt")
            .xalign(0.0)
            .css_classes(["heading"])
            .build();
        let custom_prompt_subtitle = gtk::Label::builder()
            .label("Optional extra instructions added after the selected cleanup profile")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .build();
        let custom_prompt_buffer = gtk::TextBuffer::new(None);
        let custom_prompt_view = gtk::TextView::with_buffer(&custom_prompt_buffer);
        custom_prompt_view.set_wrap_mode(gtk::WrapMode::WordChar);
        custom_prompt_view.set_monospace(true);
        custom_prompt_view.set_vexpand(true);
        custom_prompt_view.set_size_request(-1, 140);
        let custom_prompt_scroller = gtk::ScrolledWindow::builder()
            .min_content_height(140)
            .hexpand(true)
            .child(&custom_prompt_view)
            .build();
        let reset_custom_prompt_button = gtk::Button::builder()
            .label("Reset to Default")
            .css_classes(["destructive-action"])
            .halign(Align::Start)
            .build();
        let custom_prompt_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        custom_prompt_box.append(&custom_prompt_title);
        custom_prompt_box.append(&custom_prompt_subtitle);
        custom_prompt_box.append(&custom_prompt_scroller);
        custom_prompt_box.append(&reset_custom_prompt_button);
        cleanup_page.append(&custom_prompt_box);

        let cleanup_scroller = scrollable_page(&cleanup_page);
        stack.add_titled(&cleanup_scroller, Some("cleanup"), "Cleanup");

        // ===== 3. Corrections page =====
        let corrections_page = settings_page_box();

        // -- Preferred transcriptions editor --
        let preferred_title = gtk::Label::builder()
            .label("Preferred transcriptions")
            .xalign(0.0)
            .css_classes(["heading"])
            .build();
        let preferred_subtitle = gtk::Label::builder()
            .label("One preferred word or phrase per line")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .build();
        let preferred_transcriptions_buffer = gtk::TextBuffer::new(None);
        let preferred_transcriptions_view =
            gtk::TextView::with_buffer(&preferred_transcriptions_buffer);
        preferred_transcriptions_view.set_wrap_mode(gtk::WrapMode::WordChar);
        preferred_transcriptions_view.set_monospace(true);
        preferred_transcriptions_view.set_vexpand(true);
        preferred_transcriptions_view.set_size_request(-1, 100);
        let preferred_scroller = gtk::ScrolledWindow::builder()
            .min_content_height(100)
            .hexpand(true)
            .child(&preferred_transcriptions_view)
            .build();
        let preferred_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        preferred_box.append(&preferred_title);
        preferred_box.append(&preferred_subtitle);
        preferred_box.append(&preferred_scroller);
        corrections_page.append(&preferred_box);

        // -- Commonly misheard editor --
        let misheard_title = gtk::Label::builder()
            .label("Commonly misheard")
            .xalign(0.0)
            .css_classes(["heading"])
            .build();
        let misheard_subtitle = gtk::Label::builder()
            .label("One replacement per line using wrong -> right")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .build();
        let replacement_rules_buffer = gtk::TextBuffer::new(None);
        let replacement_rules_view = gtk::TextView::with_buffer(&replacement_rules_buffer);
        replacement_rules_view.set_wrap_mode(gtk::WrapMode::WordChar);
        replacement_rules_view.set_monospace(true);
        replacement_rules_view.set_vexpand(true);
        replacement_rules_view.set_size_request(-1, 100);
        let misheard_scroller = gtk::ScrolledWindow::builder()
            .min_content_height(100)
            .hexpand(true)
            .child(&replacement_rules_view)
            .build();
        let misheard_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        misheard_box.append(&misheard_title);
        misheard_box.append(&misheard_subtitle);
        misheard_box.append(&misheard_scroller);
        corrections_page.append(&misheard_box);

        // -- Learning card --
        let learning_label = section_label("Learning");
        corrections_page.append(&learning_label);
        let learning_list = gtk::ListBox::new();
        learning_list.add_css_class("boxed-list");

        let post_paste_learning_switch = gtk::Switch::builder().valign(Align::Center).build();
        let post_paste_learning_row = adw::ActionRow::builder()
            .title("Learn from corrections after paste")
            .subtitle("Automatically learn preferred spellings when you correct pasted text")
            .activatable_widget(&post_paste_learning_switch)
            .build();
        post_paste_learning_row.add_suffix(&post_paste_learning_switch);
        learning_list.append(&list_box_row(&post_paste_learning_row));
        corrections_page.append(&learning_list);

        // -- Clear + explanatory footer --
        let clear_corrections_button = gtk::Button::builder()
            .label("Clear All Corrections")
            .css_classes(["destructive-action"])
            .halign(Align::Start)
            .build();
        corrections_page.append(&clear_corrections_button);

        let corrections_explanation = gtk::Label::builder()
            .label("Preferred transcriptions are preserved during cleanup. Commonly misheard replacements are sent as correction hints to the cleanup model.")
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .build();
        corrections_page.append(&corrections_explanation);

        let corrections_scroller = scrollable_page(&corrections_page);
        stack.add_titled(&corrections_scroller, Some("corrections"), "Corrections");

        // ===== 4. Models page =====
        let models_page = settings_page_box();

        let models_card_label = section_label("Models");
        models_page.append(&models_card_label);
        let models_list = gtk::ListBox::new();
        models_list.add_css_class("boxed-list");

        let asr_model_ids: Vec<&str> = supported_models()
            .iter()
            .filter(|m| m.kind == ModelKind::Asr)
            .map(|m| m.id)
            .collect();
        let asr_model_list = gtk::StringList::new(&asr_model_ids);
        let asr_model_dropdown =
            gtk::DropDown::new(Some(asr_model_list), None::<gtk::Expression>);
        asr_model_dropdown.set_hexpand(true);
        let asr_model_index = asr_model_ids
            .iter()
            .position(|id| *id == surface_state.preferred_asr_model)
            .unwrap_or(0);
        asr_model_dropdown.set_selected(asr_model_index as u32);
        let asr_model_row = adw::ActionRow::builder()
            .title("Transcription model")
            .subtitle("Speech-to-text model for voice input")
            .build();
        asr_model_row.add_suffix(&asr_model_dropdown);
        models_list.append(&list_box_row(&asr_model_row));

        let cleanup_model_ids: Vec<&str> = supported_models()
            .iter()
            .filter(|m| m.kind == ModelKind::Cleanup)
            .map(|m| m.id)
            .collect();
        let cleanup_model_list = gtk::StringList::new(&cleanup_model_ids);
        let cleanup_model_dropdown =
            gtk::DropDown::new(Some(cleanup_model_list), None::<gtk::Expression>);
        cleanup_model_dropdown.set_hexpand(true);
        let cleanup_model_index = cleanup_model_ids
            .iter()
            .position(|id| *id == surface_state.preferred_cleanup_model)
            .unwrap_or(0);
        cleanup_model_dropdown.set_selected(cleanup_model_index as u32);
        let cleanup_model_row = adw::ActionRow::builder()
            .title("Cleanup model")
            .subtitle("Local model used to clean up transcripts")
            .build();
        cleanup_model_row.add_suffix(&cleanup_model_dropdown);
        models_list.append(&list_box_row(&cleanup_model_row));
        models_page.append(&models_list);

        // -- Model readiness status --
        let model_status_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .label(&model_readiness_status_text())
            .build();
        models_page.append(&model_status_label);

        // -- Download Missing Models button + progress --
        let download_progress_bar = gtk::ProgressBar::new();
        download_progress_bar.set_visible(false);
        download_progress_bar.set_show_text(true);
        models_page.append(&download_progress_bar);

        let download_models_button = gtk::Button::with_label("Download Missing Models");
        download_models_button.set_halign(gtk::Align::Start);
        download_models_button.add_css_class("suggested-action");
        {
            let model_status_label = model_status_label.clone();
            let download_models_button_ref = download_models_button.clone();
            let download_progress_bar = download_progress_bar.clone();
            download_models_button.connect_clicked(move |_| {
                download_models_button_ref.set_sensitive(false);
                download_progress_bar.set_visible(true);
                download_progress_bar.set_fraction(0.0);
                download_progress_bar.set_text(Some("Preparing download..."));
                model_status_label.set_label("Downloading models\u{2026}");
                let (sender, receiver) =
                    std::sync::mpsc::channel::<pepperx_models::BootstrapProgress>();
                let settings = AppSettings::load_or_default();
                let asr_model_id = settings.preferred_asr_model.clone();
                let cleanup_model_id = settings.preferred_cleanup_model.clone();
                std::thread::spawn(move || {
                    let cache_root = pepperx_models::default_cache_root();
                    // Download the user's selected models, not just the defaults
                    let models_to_download: Vec<&pepperx_models::CatalogModel> = [
                        pepperx_models::catalog_model(&asr_model_id),
                        pepperx_models::catalog_model(&cleanup_model_id),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    for (i, model) in models_to_download.iter().enumerate() {
                        let _ = sender.send(pepperx_models::BootstrapProgress {
                            total_models: models_to_download.len(),
                            completed_models: i,
                            current_model_id: Some(model.id.to_string()),
                            failure_message: None,
                            model_states: Vec::new(),
                        });
                        if let Err(e) = pepperx_models::bootstrap_model(model, &cache_root) {
                            let _ = sender.send(pepperx_models::BootstrapProgress {
                                total_models: models_to_download.len(),
                                completed_models: i,
                                current_model_id: Some(model.id.to_string()),
                                failure_message: Some(e.to_string()),
                                model_states: Vec::new(),
                            });
                            return;
                        }
                    }
                    let _ = sender.send(pepperx_models::BootstrapProgress {
                        total_models: models_to_download.len(),
                        completed_models: models_to_download.len(),
                        current_model_id: None,
                        failure_message: None,
                        model_states: Vec::new(),
                    });
                });
                let status_label = model_status_label.clone();
                let button = download_models_button_ref.clone();
                let progress_bar = download_progress_bar.clone();
                gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
                    let mut last_progress = None;
                    while let Ok(progress) = receiver.try_recv() {
                        last_progress = Some(progress);
                    }
                    if let Some(progress) = last_progress {
                        let fraction = if progress.total_models > 0 {
                            progress.completed_models as f64 / progress.total_models as f64
                        } else {
                            0.0
                        };
                        progress_bar.set_fraction(fraction);
                        if let Some(ref model_id) = progress.current_model_id {
                            let msg = format!(
                                "Downloading {} ({}/{})",
                                model_id,
                                progress.completed_models + 1,
                                progress.total_models,
                            );
                            progress_bar.set_text(Some(&msg));
                            status_label.set_label(&msg);
                        }
                        let done = progress.completed_models == progress.total_models
                            || progress.failure_message.is_some();
                        if done {
                            progress_bar.set_fraction(1.0);
                            if let Some(failure) = progress.failure_message {
                                progress_bar.set_text(Some(&format!("Failed: {failure}")));
                                status_label.set_label(&format!("Download failed: {failure}"));
                            } else {
                                progress_bar.set_text(Some("All models downloaded"));
                                status_label.set_label(&model_readiness_status_text());
                            }
                            button.set_sensitive(true);
                            // Hide progress bar after 3 seconds
                            let bar = progress_bar.clone();
                            gtk::glib::timeout_add_local_once(Duration::from_secs(3), move || {
                                bar.set_visible(false);
                            });
                            return gtk::glib::ControlFlow::Break;
                        }
                    }
                    gtk::glib::ControlFlow::Continue
                });
            });
        }
        models_page.append(&download_models_button);

        let models_scroller = scrollable_page(&models_page);
        stack.add_titled(&models_scroller, Some("models"), "Models");

        // ===== 5. History page =====
        let history_container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        history_container.set_hexpand(true);
        history_container.set_vexpand(true);
        if let Some(history_widget) = history_widget {
            history_container.append(&history_widget);
        } else {
            let placeholder = gtk::Label::builder()
                .label("No history available.")
                .xalign(0.0)
                .css_classes(["dim-label"])
                .build();
            history_container.append(&placeholder);
        }
        stack.add_titled(&history_container, Some("history"), "History");

        // ===== 6. General page =====
        let general_page = settings_page_box();

        let general_card_label = section_label("General");
        general_page.append(&general_card_label);
        let general_list = gtk::ListBox::new();
        general_list.add_css_class("boxed-list");

        let launch_at_login_switch = gtk::Switch::builder().valign(Align::Center).build();
        let launch_at_login_row = adw::ActionRow::builder()
            .title("Launch at login")
            .subtitle("Start Pepper X in the background when your GNOME session begins")
            .activatable_widget(&launch_at_login_switch)
            .build();
        launch_at_login_row.add_suffix(&launch_at_login_switch);
        general_list.append(&list_box_row(&launch_at_login_row));
        general_page.append(&general_list);

        let general_scroller = scrollable_page(&general_page);
        stack.add_titled(&general_scroller, Some("general"), "General");

        // ===== 7. Diagnostics page =====
        let diagnostics_view = crate::diagnostics_view::DiagnosticsView::new(&diagnostics_summary);
        let diagnostics_scroller = scrollable_page(diagnostics_view.widget());
        stack.add_titled(&diagnostics_scroller, Some("diagnostics"), "Diagnostics");

        // -- Feedback label lives outside the stack, at the bottom of the root --
        let feedback_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .visible(false)
            .build();

        let right_side = gtk::Box::new(gtk::Orientation::Vertical, 0);
        right_side.set_hexpand(true);
        right_side.set_vexpand(true);
        right_side.append(&stack);
        right_side.append(&feedback_label);

        root.append(&sidebar);
        root.append(&right_side);

        let updating_settings = Rc::new(Cell::new(false));
        let view = Self {
            root,
            asr_model_dropdown,
            asr_model_ids: asr_model_ids.iter().map(|s| s.to_string()).collect(),
            cleanup_switch,
            cleanup_model_dropdown,
            cleanup_model_ids: cleanup_model_ids.iter().map(|s| s.to_string()).collect(),
            prompt_profile_dropdown,
            custom_prompt_buffer,
            custom_prompt_view,
            reset_custom_prompt_button,
            window_context_switch,
            hold_trigger_button,
            hold_trigger_value,
            toggle_trigger_button,
            toggle_trigger_value,
            preferred_transcriptions_buffer,
            replacement_rules_buffer,
            clear_corrections_button,
            launch_at_login_switch,
            play_sounds_switch,
            ignore_other_speakers_switch,
            post_paste_learning_switch,
            test_dictation_button,
            test_dictation_status_label,
            test_dictation_result_label,
            model_status_label,
            feedback_label,
            updating_settings,
            history_container,
            diagnostics_view,
            shared_trigger_config,
        };
        view.connect_settings_handlers();
        view.set_surface_state(&surface_state);
        view
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn history_container(&self) -> &gtk::Box {
        &self.history_container
    }

    pub fn set_diagnostics_summary(&self, summary: &str) {
        self.diagnostics_view.set_summary(summary);
    }

    pub fn set_surface_state(&self, surface_state: &SettingsSurfaceState) {
        self.updating_settings.set(true);
        self.cleanup_switch
            .set_active(surface_state.cleanup_enabled);
        self.launch_at_login_switch
            .set_active(surface_state.launch_at_login);
        self.play_sounds_switch
            .set_active(surface_state.play_sounds);
        self.ignore_other_speakers_switch
            .set_active(surface_state.ignore_other_speakers);
        self.post_paste_learning_switch
            .set_active(surface_state.enable_post_paste_learning);
        self.window_context_switch
            .set_active(surface_state.enable_window_context);
        self.window_context_switch
            .set_sensitive(surface_state.cleanup_enabled);
        self.custom_prompt_buffer
            .set_text(&surface_state.cleanup_custom_prompt);
        self.custom_prompt_view
            .set_sensitive(surface_state.cleanup_enabled);
        self.custom_prompt_view
            .set_editable(surface_state.cleanup_enabled);
        self.prompt_profile_dropdown
            .set_sensitive(surface_state.cleanup_enabled);
        self.prompt_profile_dropdown
            .set_selected(prompt_profile_index(&surface_state.cleanup_prompt_profile));
        self.hold_trigger_button
            .set_label(&trigger_keys_display_name(&surface_state.hold_trigger_keys));
        *self.hold_trigger_value.borrow_mut() =
            surface_state.hold_trigger_keys.clone();
        self.toggle_trigger_button
            .set_label(&trigger_keys_display_name(&surface_state.toggle_trigger_keys));
        *self.toggle_trigger_value.borrow_mut() =
            surface_state.toggle_trigger_keys.clone();
        self.preferred_transcriptions_buffer
            .set_text(&surface_state.preferred_transcriptions_text);
        self.replacement_rules_buffer
            .set_text(&surface_state.replacement_rules_text);
        let has_corrections = !surface_state.preferred_transcriptions_text.is_empty()
            || !surface_state.replacement_rules_text.is_empty();
        self.clear_corrections_button.set_sensitive(has_corrections);
        self.model_status_label
            .set_label(&model_readiness_status_text());
        if let Some(feedback_message) = surface_state.feedback_message.as_deref() {
            self.feedback_label.set_label(feedback_message);
            self.feedback_label.set_visible(true);
        } else {
            self.feedback_label.set_label("");
            self.feedback_label.set_visible(false);
        }
        self.updating_settings.set(false);
    }

    fn connect_settings_handlers(&self) {
        let asr_model_dropdown = self.asr_model_dropdown.clone();
        let asr_model_ids = self.asr_model_ids.clone();
        let cleanup_switch = self.cleanup_switch.clone();
        let cleanup_model_dropdown = self.cleanup_model_dropdown.clone();
        let cleanup_model_ids = self.cleanup_model_ids.clone();
        let prompt_profile_dropdown = self.prompt_profile_dropdown.clone();
        let custom_prompt_buffer = self.custom_prompt_buffer.clone();
        let custom_prompt_view = self.custom_prompt_view.clone();
        let window_context_switch = self.window_context_switch.clone();
        let launch_at_login_switch = self.launch_at_login_switch.clone();

        self.asr_model_dropdown.connect_selected_notify({
            let updating_settings = self.updating_settings.clone();
            let asr_model_ids = asr_model_ids.clone();
            let feedback_label = self.feedback_label.clone();
            move |dropdown| {
                if updating_settings.get() {
                    return;
                }

                let index = dropdown.selected() as usize;
                if let Some(model_id) = asr_model_ids.get(index) {
                    let model_id = model_id.clone();
                    if let Err(error) = save_settings_change(move |settings| {
                        settings.preferred_asr_model = model_id;
                    }) {
                        feedback_label.set_label(&format!("Failed to save settings: {error}"));
                        feedback_label.set_visible(true);
                    } else {
                        feedback_label.set_label("Saved settings");
                        feedback_label.set_visible(true);
                    }
                }
            }
        });

        self.cleanup_switch.connect_active_notify({
            let updating_settings = self.updating_settings.clone();
            let cleanup_switch = cleanup_switch.clone();
            let prompt_profile_dropdown = prompt_profile_dropdown.clone();
            let custom_prompt_buffer = custom_prompt_buffer.clone();
            let custom_prompt_view = custom_prompt_view.clone();
            let window_context_switch = window_context_switch.clone();
            let launch_at_login_switch = launch_at_login_switch.clone();
            let feedback_label = self.feedback_label.clone();
            move |switch| {
                if updating_settings.get() {
                    return;
                }

                let cleanup_enabled = switch.is_active();
                prompt_profile_dropdown.set_sensitive(cleanup_enabled);
                custom_prompt_view.set_sensitive(cleanup_enabled);
                custom_prompt_view.set_editable(cleanup_enabled);
                window_context_switch.set_sensitive(cleanup_enabled);
                if let Err(error) = save_settings_change(move |settings| {
                    settings.cleanup_enabled = cleanup_enabled;
                }) {
                    restore_settings_controls(
                        &cleanup_switch,
                        &prompt_profile_dropdown,
                        &custom_prompt_buffer,
                        &custom_prompt_view,
                        &launch_at_login_switch,
                        &feedback_label,
                        &updating_settings,
                    );
                    feedback_label.set_label(&format!("Failed to save settings: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved settings");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.window_context_switch.connect_active_notify({
            let updating_settings = self.updating_settings.clone();
            let feedback_label = self.feedback_label.clone();
            move |switch| {
                if updating_settings.get() {
                    return;
                }

                let enable_window_context = switch.is_active();
                if let Err(error) = save_settings_change(move |settings| {
                    settings.enable_window_context = enable_window_context;
                }) {
                    feedback_label.set_label(&format!("Failed to save settings: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved settings");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.cleanup_model_dropdown.connect_selected_notify({
            let updating_settings = self.updating_settings.clone();
            let cleanup_model_ids = cleanup_model_ids.clone();
            let feedback_label = self.feedback_label.clone();
            move |dropdown| {
                if updating_settings.get() {
                    return;
                }

                let index = dropdown.selected() as usize;
                if let Some(model_id) = cleanup_model_ids.get(index) {
                    let model_id = model_id.clone();
                    if let Err(error) = save_settings_change(move |settings| {
                        settings.preferred_cleanup_model = model_id;
                    }) {
                        feedback_label.set_label(&format!("Failed to save settings: {error}"));
                        feedback_label.set_visible(true);
                    } else {
                        feedback_label.set_label("Saved settings");
                        feedback_label.set_visible(true);
                    }
                }
            }
        });

        self.prompt_profile_dropdown.connect_selected_notify({
            let updating_settings = self.updating_settings.clone();
            let cleanup_switch = cleanup_switch.clone();
            let prompt_profile_dropdown = prompt_profile_dropdown.clone();
            let custom_prompt_buffer = custom_prompt_buffer.clone();
            let custom_prompt_view = custom_prompt_view.clone();
            let launch_at_login_switch = launch_at_login_switch.clone();
            let feedback_label = self.feedback_label.clone();
            move |dropdown| {
                if updating_settings.get() {
                    return;
                }

                let selected = prompt_profile_from_index(dropdown.selected());
                if let Err(error) = save_settings_change(move |settings| {
                    settings.cleanup_prompt_profile = selected;
                }) {
                    restore_settings_controls(
                        &cleanup_switch,
                        &prompt_profile_dropdown,
                        &custom_prompt_buffer,
                        &custom_prompt_view,
                        &launch_at_login_switch,
                        &feedback_label,
                        &updating_settings,
                    );
                    feedback_label.set_label(&format!("Failed to save settings: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved settings");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.custom_prompt_buffer.connect_changed({
            let updating_settings = self.updating_settings.clone();
            let cleanup_switch = cleanup_switch.clone();
            let prompt_profile_dropdown = prompt_profile_dropdown.clone();
            let custom_prompt_buffer = custom_prompt_buffer.clone();
            let custom_prompt_view = custom_prompt_view.clone();
            let launch_at_login_switch = launch_at_login_switch.clone();
            let feedback_label = self.feedback_label.clone();
            move |buffer| {
                if updating_settings.get() {
                    return;
                }

                let prompt_text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string();
                if let Err(error) = save_settings_change(move |settings| {
                    settings.cleanup_custom_prompt = prompt_text;
                }) {
                    restore_settings_controls(
                        &cleanup_switch,
                        &prompt_profile_dropdown,
                        &custom_prompt_buffer,
                        &custom_prompt_view,
                        &launch_at_login_switch,
                        &feedback_label,
                        &updating_settings,
                    );
                    feedback_label.set_label(&format!("Failed to save settings: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved settings");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.launch_at_login_switch.connect_active_notify({
            let updating_settings = self.updating_settings.clone();
            let cleanup_switch = cleanup_switch.clone();
            let prompt_profile_dropdown = prompt_profile_dropdown.clone();
            let custom_prompt_buffer = custom_prompt_buffer.clone();
            let custom_prompt_view = custom_prompt_view.clone();
            let launch_at_login_switch = launch_at_login_switch.clone();
            let feedback_label = self.feedback_label.clone();
            move |switch| {
                if updating_settings.get() {
                    return;
                }

                let launch_at_login = switch.is_active();
                if let Err(error) = save_launch_at_login(launch_at_login) {
                    restore_settings_controls(
                        &cleanup_switch,
                        &prompt_profile_dropdown,
                        &custom_prompt_buffer,
                        &custom_prompt_view,
                        &launch_at_login_switch,
                        &feedback_label,
                        &updating_settings,
                    );
                    feedback_label.set_label(&format!("Failed to save settings: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved settings");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.play_sounds_switch.connect_active_notify({
            let updating_settings = self.updating_settings.clone();
            let feedback_label = self.feedback_label.clone();
            move |switch| {
                if updating_settings.get() {
                    return;
                }

                let play_sounds = switch.is_active();
                if let Err(error) = save_settings_change(move |settings| {
                    settings.play_sounds = play_sounds;
                }) {
                    feedback_label.set_label(&format!("Failed to save settings: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved settings");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.ignore_other_speakers_switch.connect_active_notify({
            let updating_settings = self.updating_settings.clone();
            let feedback_label = self.feedback_label.clone();
            move |switch| {
                if updating_settings.get() {
                    return;
                }

                let ignore_other_speakers = switch.is_active();
                if let Err(error) = save_settings_change(move |settings| {
                    settings.ignore_other_speakers = ignore_other_speakers;
                }) {
                    feedback_label.set_label(&format!("Failed to save settings: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved settings");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.post_paste_learning_switch.connect_active_notify({
            let updating_settings = self.updating_settings.clone();
            let feedback_label = self.feedback_label.clone();
            move |switch| {
                if updating_settings.get() {
                    return;
                }

                let enable_post_paste_learning = switch.is_active();
                if let Err(error) = save_settings_change(move |settings| {
                    settings.enable_post_paste_learning = enable_post_paste_learning;
                }) {
                    feedback_label.set_label(&format!("Failed to save settings: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved settings");
                    feedback_label.set_visible(true);
                }
            }
        });

        // Test dictation button handler
        self.test_dictation_button.connect_clicked({
            let button = self.test_dictation_button.clone();
            let status_label = self.test_dictation_status_label.clone();
            let result_label = self.test_dictation_result_label.clone();
            let recording: Rc<Cell<bool>> = Rc::new(Cell::new(false));
            let active_recording: Rc<RefCell<Option<pepperx_audio::recording::ActiveRecording>>> =
                Rc::new(RefCell::new(None));
            move |_| {
                if recording.get() {
                    // Stop recording, begin transcription
                    recording.set(false);
                    button.set_label("Start Test");
                    let artifact = active_recording.borrow_mut().take();
                    if let Some(active) = artifact {
                        status_label.set_label("Transcribing...");
                        status_label.remove_css_class("error");
                        status_label.set_visible(true);
                        result_label.set_visible(false);

                        let status_label = status_label.clone();
                        let result_label = result_label.clone();
                        let (tx, rx) = mpsc::channel::<Result<String, String>>();
                        std::thread::spawn(move || {
                            match active.stop() {
                                Ok(artifact) => {
                                    let wav_path = artifact.wav_path();
                                    match crate::transcription::transcribe_wav_to_log(wav_path) {
                                        Ok(entry) => {
                                            let _ = tx.send(Ok(entry.display_text().to_string()));
                                        }
                                        Err(e) => {
                                            let _ = tx.send(Err(e.to_string()));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(Err(e.to_string()));
                                }
                            }
                        });
                        // Poll for result
                        gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
                            match rx.try_recv() {
                                Ok(Ok(text)) => {
                                    status_label.set_visible(false);
                                    result_label.remove_css_class("error");
                                    result_label.set_label(&text);
                                    result_label.set_visible(true);
                                    gtk::glib::ControlFlow::Break
                                }
                                Ok(Err(error)) => {
                                    status_label.set_visible(false);
                                    result_label.add_css_class("error");
                                    result_label.set_label(&error);
                                    result_label.set_visible(true);
                                    gtk::glib::ControlFlow::Break
                                }
                                Err(mpsc::TryRecvError::Empty) => {
                                    gtk::glib::ControlFlow::Continue
                                }
                                Err(mpsc::TryRecvError::Disconnected) => {
                                    status_label.set_visible(false);
                                    result_label.add_css_class("error");
                                    result_label.set_label("Test transcription failed unexpectedly");
                                    result_label.set_visible(true);
                                    gtk::glib::ControlFlow::Break
                                }
                            }
                        });
                    }
                } else {
                    // Start recording
                    let settings = crate::settings::AppSettings::load_or_default();
                    let mic = settings.preferred_microphone.clone();
                    let unique = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos();
                    let wav_path = crate::transcript_log::state_root()
                        .join("recordings")
                        .join(format!("test-dictation-{unique}.wav"));
                    let request = pepperx_audio::recording::RecordingRequest::new(wav_path, mic);
                    match pepperx_audio::recording::start_recording(request) {
                        Ok(active) => {
                            recording.set(true);
                            *active_recording.borrow_mut() = Some(active);
                            button.set_label("Stop Test");
                            status_label.remove_css_class("error");
                            status_label.set_label("Recording...");
                            status_label.set_visible(true);
                            result_label.set_visible(false);
                        }
                        Err(e) => {
                            status_label.add_css_class("error");
                            status_label.set_label(&format!("Failed to start recording: {e}"));
                            status_label.set_visible(true);
                        }
                    }
                }
            }
        });

        // Hold-to-record shortcut recorder
        install_shortcut_recorder(
            &self.hold_trigger_button,
            self.hold_trigger_value.clone(),
            self.toggle_trigger_value.clone(),
            "Toggle Recording",
            self.feedback_label.clone(),
            self.updating_settings.clone(),
            |settings, value| { settings.hold_trigger_keys = value; },
            self.shared_trigger_config.clone(),
        );

        // Toggle-to-record shortcut recorder
        install_shortcut_recorder(
            &self.toggle_trigger_button,
            self.toggle_trigger_value.clone(),
            self.hold_trigger_value.clone(),
            "Hold to Record",
            self.feedback_label.clone(),
            self.updating_settings.clone(),
            |settings, value| { settings.toggle_trigger_keys = value; },
            self.shared_trigger_config.clone(),
        );

        self.reset_custom_prompt_button.connect_clicked({
            let custom_prompt_buffer = custom_prompt_buffer.clone();
            let updating_settings = self.updating_settings.clone();
            let feedback_label = self.feedback_label.clone();
            move |_| {
                if updating_settings.get() {
                    return;
                }
                custom_prompt_buffer.set_text("");
                if let Err(error) = save_settings_change(move |settings| {
                    settings.cleanup_custom_prompt = String::new();
                }) {
                    feedback_label.set_label(&format!("Failed to save settings: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Custom prompt reset to default");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.preferred_transcriptions_buffer.connect_changed({
            let updating_settings = self.updating_settings.clone();
            let feedback_label = self.feedback_label.clone();
            move |buffer| {
                if updating_settings.get() {
                    return;
                }
                let text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string();
                let entries: Vec<String> = text
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                let store_root = corrections_store_path();
                let mut store = CorrectionStore::load(&store_root)
                    .unwrap_or_else(|_| CorrectionStore::new(&store_root));
                store.set_all_preferred_transcriptions(&entries);
                if let Err(error) = store.rewrite() {
                    feedback_label.set_label(&format!("Failed to save corrections: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved corrections");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.replacement_rules_buffer.connect_changed({
            let updating_settings = self.updating_settings.clone();
            let feedback_label = self.feedback_label.clone();
            move |buffer| {
                if updating_settings.get() {
                    return;
                }
                let text = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), false)
                    .to_string();
                let rules: Vec<(String, String)> = text
                    .lines()
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.splitn(2, "->").collect();
                        if parts.len() == 2 {
                            Some((parts[0].trim().to_string(), parts[1].trim().to_string()))
                        } else {
                            None
                        }
                    })
                    .filter(|(s, r)| !s.is_empty() && !r.is_empty())
                    .collect();
                let store_root = corrections_store_path();
                let mut store = CorrectionStore::load(&store_root)
                    .unwrap_or_else(|_| CorrectionStore::new(&store_root));
                store.set_all_replacement_rules(&rules);
                if let Err(error) = store.rewrite() {
                    feedback_label.set_label(&format!("Failed to save corrections: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    feedback_label.set_label("Saved corrections");
                    feedback_label.set_visible(true);
                }
            }
        });

        self.clear_corrections_button.connect_clicked({
            let preferred_transcriptions_buffer = self.preferred_transcriptions_buffer.clone();
            let replacement_rules_buffer = self.replacement_rules_buffer.clone();
            let clear_corrections_button = self.clear_corrections_button.clone();
            let updating_settings = self.updating_settings.clone();
            let feedback_label = self.feedback_label.clone();
            move |_| {
                let store_root = corrections_store_path();
                let mut store = CorrectionStore::load(&store_root).unwrap_or_else(|_| {
                    CorrectionStore::new(&store_root)
                });
                if let Err(error) = store.clear() {
                    feedback_label.set_label(&format!("Failed to clear corrections: {error}"));
                    feedback_label.set_visible(true);
                } else {
                    updating_settings.set(true);
                    preferred_transcriptions_buffer.set_text("");
                    replacement_rules_buffer.set_text("");
                    updating_settings.set(false);
                    clear_corrections_button.set_sensitive(false);
                    feedback_label.set_label("Cleared all corrections");
                    feedback_label.set_visible(true);
                }
            }
        });
    }
}

#[cfg(test)]
pub fn settings_page_scaffold(surface_state: &SettingsSurfaceState) -> SettingsPageScaffold {
    SettingsPageScaffold {
        container_kind: SettingsContainerKind::Form,
        sections: settings_form_sections(surface_state),
        feedback_message: surface_state.feedback_message.clone(),
    }
}

#[cfg(test)]
pub fn settings_form_sections(surface_state: &SettingsSurfaceState) -> Vec<SettingsFormSection> {
    vec![
        SettingsFormSection {
            title: "Recording".into(),
            controls: vec![
                SettingsControl::ShortcutRecorder(SettingsShortcutRecorderControl {
                    title: "Hold to Record".into(),
                    subtitle: "Hold this key combination to record, release to stop".into(),
                    current_shortcut: trigger_keys_display_name(
                        &surface_state.hold_trigger_keys,
                    ),
                }),
                SettingsControl::ShortcutRecorder(SettingsShortcutRecorderControl {
                    title: "Toggle Recording".into(),
                    subtitle: "Press once to start recording, press again to stop".into(),
                    current_shortcut: trigger_keys_display_name(
                        &surface_state.toggle_trigger_keys,
                    ),
                }),
                SettingsControl::Switch(SettingsSwitchControl {
                    title: "Sound effects".into(),
                    subtitle: "Play a sound when recording starts and stops".into(),
                    active: surface_state.play_sounds,
                }),
                SettingsControl::Switch(SettingsSwitchControl {
                    title: "Ignore other speakers".into(),
                    subtitle:
                        "Filter out other voices during transcription (experimental)"
                            .into(),
                    active: surface_state.ignore_other_speakers,
                }),
            ],
        },
        SettingsFormSection {
            title: "Cleanup".into(),
            controls: vec![
                SettingsControl::Switch(SettingsSwitchControl {
                    title: "Enable cleanup".into(),
                    subtitle: "Run the local cleanup model after transcription when it is ready"
                        .into(),
                    active: surface_state.cleanup_enabled,
                }),
                SettingsControl::Switch(SettingsSwitchControl {
                    title: "Window context".into(),
                    subtitle: "Capture screen text to help the cleanup model disambiguate names and terms"
                        .into(),
                    active: surface_state.enable_window_context,
                }),
                SettingsControl::Select(SettingsSelectControl {
                    title: "Prompt profile".into(),
                    subtitle:
                        "Choose the baseline cleanup behavior used when the custom prompt is empty"
                            .into(),
                    selected: surface_state.cleanup_prompt_profile.clone(),
                    options: PROMPT_PROFILE_OPTIONS
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    enabled: surface_state.cleanup_enabled,
                }),
                SettingsControl::TextArea(SettingsTextAreaControl {
                    title: "Custom cleanup prompt".into(),
                    subtitle:
                        "Optional extra instructions added after the selected cleanup profile"
                            .into(),
                    text: surface_state.cleanup_custom_prompt.clone(),
                    enabled: surface_state.cleanup_enabled,
                }),
            ],
        },
        SettingsFormSection {
            title: "Corrections".into(),
            controls: vec![
                SettingsControl::TextArea(SettingsTextAreaControl {
                    title: "Preferred transcriptions".into(),
                    subtitle: "One preferred word or phrase per line".into(),
                    text: surface_state.preferred_transcriptions_text.clone(),
                    enabled: true,
                }),
                SettingsControl::TextArea(SettingsTextAreaControl {
                    title: "Commonly misheard".into(),
                    subtitle: "One replacement per line using wrong -> right".into(),
                    text: surface_state.replacement_rules_text.clone(),
                    enabled: true,
                }),
                SettingsControl::Switch(SettingsSwitchControl {
                    title: "Learn from corrections after paste".into(),
                    subtitle: "Automatically learn preferred spellings when you correct pasted text"
                        .into(),
                    active: surface_state.enable_post_paste_learning,
                }),
            ],
        },
        SettingsFormSection {
            title: "Models".into(),
            controls: vec![
                SettingsControl::Select(SettingsSelectControl {
                    title: "Transcription model".into(),
                    subtitle: "Speech-to-text model for voice input".into(),
                    selected: surface_state.preferred_asr_model.clone(),
                    options: supported_models()
                        .iter()
                        .filter(|m| m.kind == ModelKind::Asr)
                        .map(|m| m.id.to_string())
                        .collect(),
                    enabled: true,
                }),
                SettingsControl::Select(SettingsSelectControl {
                    title: "Cleanup model".into(),
                    subtitle: "Local model used to clean up transcripts".into(),
                    selected: surface_state.preferred_cleanup_model.clone(),
                    options: supported_models()
                        .iter()
                        .filter(|m| m.kind == ModelKind::Cleanup)
                        .map(|m| m.id.to_string())
                        .collect(),
                    enabled: true,
                }),
            ],
        },
        SettingsFormSection {
            title: "History".into(),
            controls: vec![],
        },
        SettingsFormSection {
            title: "General".into(),
            controls: vec![
                SettingsControl::Switch(SettingsSwitchControl {
                    title: "Launch at login".into(),
                    subtitle: "Start Pepper X in the background when your GNOME session begins"
                        .into(),
                    active: surface_state.launch_at_login,
                }),
            ],
        },
        SettingsFormSection {
            title: "Diagnostics".into(),
            controls: vec![],
        },
    ]
}

fn section_label(title: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .css_classes(["heading"])
        .build()
}

/// Create a standard vertical box used as the inner content of each settings page.
fn settings_page_box() -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_margin_top(24);
    page.set_margin_bottom(24);
    page.set_margin_start(24);
    page.set_margin_end(24);
    page.set_valign(Align::Start);
    page
}

/// Wrap a page content box in a scrolled window so long pages can scroll.
fn scrollable_page(content: &gtk::Box) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .child(content)
        .build()
}

fn list_box_row(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(child));
    row
}

fn prompt_profile_index(selected_profile: &str) -> u32 {
    PROMPT_PROFILE_OPTIONS
        .iter()
        .position(|profile| *profile == selected_profile)
        .map(|index| index as u32)
        .unwrap_or(0)
}

fn prompt_profile_from_index(index: u32) -> String {
    PROMPT_PROFILE_OPTIONS
        .get(index as usize)
        .copied()
        .unwrap_or(PROMPT_PROFILE_OPTIONS[0])
        .to_string()
}

/// Wire up a shortcut recorder button with conflict detection against the other recorder.
fn install_shortcut_recorder(
    button: &gtk::Button,
    own_value: Rc<RefCell<String>>,
    other_value: Rc<RefCell<String>>,
    other_label: &'static str,
    feedback_label: gtk::Label,
    updating_settings: Rc<Cell<bool>>,
    save_fn: fn(&mut AppSettings, String),
    shared_trigger_config: Option<SharedTriggerConfig>,
) {
    let recording = Rc::new(Cell::new(false));
    let pressed_keys: Rc<RefCell<BTreeSet<u32>>> = Rc::new(RefCell::new(BTreeSet::new()));

    // Click handler: enter recording mode.
    button.connect_clicked({
        let recording = recording.clone();
        let pressed_keys = pressed_keys.clone();
        let button = button.clone();
        move |btn| {
            if recording.get() {
                return;
            }
            recording.set(true);
            pressed_keys.borrow_mut().clear();
            btn.set_label("Press keys...");
            button.grab_focus();
        }
    });

    // Track which keys are physically held so we can detect when ALL keys
    // have been released. GDK modifier flags are unreliable for this because
    // the flag for a modifier key is still set in the release event for that
    // same key.
    let held_keys: Rc<RefCell<BTreeSet<u32>>> = Rc::new(RefCell::new(BTreeSet::new()));

    // Key event controller for capture.
    let key_controller = gtk::EventControllerKey::new();

    key_controller.connect_key_pressed({
        let recording = recording.clone();
        let pressed_keys = pressed_keys.clone();
        let held_keys = held_keys.clone();
        let button = button.clone();
        let own_value = own_value.clone();
        move |_controller, keyval, _keycode, _modifier| {
            if !recording.get() {
                return gtk::glib::Propagation::Proceed;
            }

            let raw_keyval = keyval.into_glib();

            // Track every physical key-press so we can detect full release
            held_keys.borrow_mut().insert(raw_keyval);

            // Escape cancels recording
            if raw_keyval == 0xff1b {
                recording.set(false);
                pressed_keys.borrow_mut().clear();
                held_keys.borrow_mut().clear();
                let current = own_value.borrow().clone();
                button.set_label(&trigger_keys_display_name(&current));
                return gtk::glib::Propagation::Stop;
            }

            // Accept keys we can map to evdev
            if gdk_keyval_to_evdev(raw_keyval).is_some() {
                pressed_keys.borrow_mut().insert(raw_keyval);
            }

            gtk::glib::Propagation::Stop
        }
    });

    key_controller.connect_key_released({
        let recording = recording.clone();
        let pressed_keys = pressed_keys.clone();
        let held_keys = held_keys.clone();
        let button = button.clone();
        let own_value = own_value.clone();
        let other_value = other_value.clone();
        let feedback_label = feedback_label.clone();
        let updating_settings = updating_settings.clone();
        let shared_trigger_config = shared_trigger_config.clone();
        move |_controller, keyval, _keycode, _modifier| {
            if !recording.get() {
                return;
            }

            // Remove this key from the held set
            let raw_keyval = keyval.into_glib();
            held_keys.borrow_mut().remove(&raw_keyval);

            // Wait until every key that was pressed has been released
            if !held_keys.borrow().is_empty() {
                return;
            }

            recording.set(false);

            let keys = pressed_keys.borrow().clone();
            pressed_keys.borrow_mut().clear();

            if keys.is_empty() {
                let current = own_value.borrow().clone();
                button.set_label(&trigger_keys_display_name(&current));
                return;
            }

            let evdev_keycodes: Vec<u16> = keys
                .iter()
                .filter_map(|&kv| gdk_keyval_to_evdev(kv))
                .collect();

            if evdev_keycodes.is_empty() {
                let current = own_value.borrow().clone();
                button.set_label(&trigger_keys_display_name(&current));
                return;
            }

            let config = TriggerConfig::from_keycodes(&evdev_keycodes);
            let setting_value = config.to_setting();
            let display = config.display_name();

            // Duplicate detection: check against the other recorder
            let other_setting = other_value.borrow().clone();
            if setting_value == TriggerConfig::from_setting(&other_setting).to_setting() {
                // Conflict — revert and show error
                let current = own_value.borrow().clone();
                button.set_label(&trigger_keys_display_name(&current));
                feedback_label.set_label(&format!(
                    "This shortcut is already used for {other_label}"
                ));
                feedback_label.add_css_class("error");
                feedback_label.set_visible(true);
                return;
            }

            feedback_label.remove_css_class("error");

            button.set_label(&display);
            *own_value.borrow_mut() = setting_value.clone();

            if updating_settings.get() {
                return;
            }

            if let Err(error) = save_settings_change(move |settings| {
                save_fn(settings, setting_value);
            }) {
                feedback_label.set_label(&format!("Failed to save settings: {error}"));
                feedback_label.set_visible(true);
            } else {
                // Push updated config to the capture thread so it takes
                // effect immediately without a restart.
                if let Some(ref shared_config) = shared_trigger_config {
                    let settings = AppSettings::load_or_default();
                    let hold = TriggerConfig::from_setting(&settings.hold_trigger_keys);
                    let toggle = TriggerConfig::from_setting(&settings.toggle_trigger_keys);
                    if let Ok(mut guard) = shared_config.lock() {
                        *guard = (hold, toggle);
                    }
                }
                feedback_label
                    .set_label("Saved settings");
                feedback_label.set_visible(true);
            }
        }
    });

    button.add_controller(key_controller);
}

fn save_settings_change(change: impl FnOnce(&mut AppSettings)) -> std::io::Result<()> {
    let mut settings = AppSettings::load_or_default();
    change(&mut settings);

    settings.save()
}

fn restore_settings_controls(
    cleanup_switch: &gtk::Switch,
    prompt_profile_dropdown: &gtk::DropDown,
    custom_prompt_buffer: &gtk::TextBuffer,
    custom_prompt_view: &gtk::TextView,
    launch_at_login_switch: &gtk::Switch,
    feedback_label: &gtk::Label,
    updating_settings: &Rc<Cell<bool>>,
) {
    let surface_state = SettingsSurfaceState::from_settings(&AppSettings::load_or_default());
    updating_settings.set(true);
    cleanup_switch.set_active(surface_state.cleanup_enabled);
    prompt_profile_dropdown.set_sensitive(surface_state.cleanup_enabled);
    prompt_profile_dropdown
        .set_selected(prompt_profile_index(&surface_state.cleanup_prompt_profile));
    custom_prompt_buffer.set_text(&surface_state.cleanup_custom_prompt);
    custom_prompt_view.set_sensitive(surface_state.cleanup_enabled);
    custom_prompt_view.set_editable(surface_state.cleanup_enabled);
    launch_at_login_switch.set_active(surface_state.launch_at_login);
    if let Some(feedback_message) = surface_state.feedback_message.as_deref() {
        feedback_label.set_label(feedback_message);
        feedback_label.set_visible(true);
    } else {
        feedback_label.set_label("");
        feedback_label.set_visible(false);
    }
    updating_settings.set(false);
}

pub(crate) fn build_microphone_controls(section_title: &str, _meter_label: &str) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);

    let section_label = gtk::Label::builder()
        .label(section_title)
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");

    let picker_label = gtk::Label::builder()
        .label("Microphone")
        .xalign(0.0)
        .css_classes(["caption"])
        .build();
    let picker_model = gtk::StringList::new(&[]);
    let picker = gtk::DropDown::new(Some(picker_model.clone()), None::<gtk::Expression>);
    picker.set_hexpand(true);
    let picker_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    picker_box.append(&picker_label);
    picker_box.append(&picker);
    let picker_list_row = gtk::ListBoxRow::builder()
        .child(&picker_box)
        .build();
    picker_list_row.set_activatable(false);
    list.append(&picker_list_row);

    root.append(&section_label);
    root.append(&list);

    let known_devices = Rc::new(RefCell::new(Vec::<MicrophoneDevice>::new()));
    let selected_microphone = Rc::new(RefCell::new(None::<SelectedMicrophone>));
    let updating_picker = Rc::new(Cell::new(false));

    schedule_microphone_enumeration(
        &picker,
        &picker_model,
        &known_devices,
        &selected_microphone,
        &updating_picker,
    );

    picker.connect_selected_notify({
        let known_devices = known_devices.clone();
        let selected_microphone = selected_microphone.clone();
        let updating_picker = updating_picker.clone();
        move |picker| {
            if updating_picker.get() {
                return;
            }

            let next_selection = (picker.selected() != gtk::INVALID_LIST_POSITION)
                .then(|| picker.selected() as usize)
                .and_then(|index| {
                    known_devices
                        .borrow()
                        .get(index)
                        .map(SelectedMicrophone::from)
                });
            let _ = save_preferred_microphone(next_selection.clone());
            *selected_microphone.borrow_mut() = next_selection;
        }
    });

    root
}

fn schedule_microphone_enumeration(
    picker: &gtk::DropDown,
    picker_model: &gtk::StringList,
    known_devices: &Rc<RefCell<Vec<MicrophoneDevice>>>,
    selected_microphone: &Rc<RefCell<Option<SelectedMicrophone>>>,
    updating_picker: &Rc<Cell<bool>>,
) {
    let (sender, receiver) = mpsc::channel::<MicrophoneUiState>();
    std::thread::spawn(move || {
        let _ = sender.send(load_microphone_ui_state(None));
    });

    let picker = picker.clone();
    let picker_model = picker_model.clone();
    let known_devices = known_devices.clone();
    let selected_microphone = selected_microphone.clone();
    let updating_picker = updating_picker.clone();
    gtk::glib::timeout_add_local_once(Duration::from_millis(50), move || {
        poll_enumeration_result(
            receiver,
            &picker,
            &picker_model,
            &known_devices,
            &selected_microphone,
            &updating_picker,
        );
    });
}

fn poll_enumeration_result(
    receiver: mpsc::Receiver<MicrophoneUiState>,
    picker: &gtk::DropDown,
    picker_model: &gtk::StringList,
    known_devices: &Rc<RefCell<Vec<MicrophoneDevice>>>,
    selected_microphone: &Rc<RefCell<Option<SelectedMicrophone>>>,
    updating_picker: &Rc<Cell<bool>>,
) {
    match receiver.try_recv() {
        Ok(ui_state) => {
            apply_microphone_ui_state(
                &ui_state,
                picker,
                picker_model,
                known_devices,
                selected_microphone,
                updating_picker,
            );
        }
        Err(mpsc::TryRecvError::Empty) => {
            let picker = picker.clone();
            let picker_model = picker_model.clone();
            let known_devices = known_devices.clone();
            let selected_microphone = selected_microphone.clone();
            let updating_picker = updating_picker.clone();
            gtk::glib::timeout_add_local_once(Duration::from_millis(50), move || {
                poll_enumeration_result(
                    receiver,
                    &picker,
                    &picker_model,
                    &known_devices,
                    &selected_microphone,
                    &updating_picker,
                );
            });
        }
        Err(mpsc::TryRecvError::Disconnected) => {
            // Microphone enumeration thread disconnected; no devices to show.
        }
    }
}

fn apply_microphone_ui_state(
    ui_state: &MicrophoneUiState,
    picker: &gtk::DropDown,
    picker_model: &gtk::StringList,
    known_devices: &Rc<RefCell<Vec<MicrophoneDevice>>>,
    selected_microphone: &Rc<RefCell<Option<SelectedMicrophone>>>,
    updating_picker: &Rc<Cell<bool>>,
) {
    updating_picker.set(true);
    picker_model.splice(0, picker_model.n_items(), &[]);
    for device in &ui_state.devices {
        picker_model.append(device.display_name());
    }
    picker.set_sensitive(!ui_state.devices.is_empty());
    if let Some(selected_id) = ui_state
        .selected_microphone
        .as_ref()
        .map(|mic| mic.stable_id())
    {
        let selected_index = ui_state
            .devices
            .iter()
            .position(|device| device.stable_id() == selected_id)
            .map(|index| index as u32)
            .unwrap_or(gtk::INVALID_LIST_POSITION);
        picker.set_selected(selected_index);
    } else {
        picker.set_selected(gtk::INVALID_LIST_POSITION);
    }
    updating_picker.set(false);

    *known_devices.borrow_mut() = ui_state.devices.clone();
    *selected_microphone.borrow_mut() = ui_state.selected_microphone.clone();
}

fn model_readiness_status_text() -> String {
    use pepperx_models::{catalog_model, default_cache_root, model_readiness};

    let settings = AppSettings::load_or_default();
    let cache_root = default_cache_root();

    let selected = [
        &settings.preferred_asr_model,
        &settings.preferred_cleanup_model,
    ];
    let not_ready: Vec<String> = selected
        .iter()
        .filter_map(|id| {
            let model = catalog_model(id)?;
            let readiness = model_readiness(model, &cache_root);
            if readiness.is_ready {
                None
            } else {
                Some(format!("{id}: not downloaded"))
            }
        })
        .collect();

    if not_ready.is_empty() {
        "All selected models ready".into()
    } else {
        not_ready.join("\n")
    }
}
