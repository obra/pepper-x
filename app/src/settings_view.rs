use adw::prelude::*;
use gtk::Align;
use pepperx_audio::{sample_input_level, MicrophoneDevice, SelectedMicrophone};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::app_model::SettingsSurfaceState;
use crate::settings::{
    load_microphone_ui_state, save_launch_at_login, save_preferred_microphone, AppSettings,
    MicrophoneUiState,
};

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
pub enum SettingsControl {
    Switch(SettingsSwitchControl),
    Select(SettingsSelectControl),
    TextArea(SettingsTextAreaControl),
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
    cleanup_switch: gtk::Switch,
    prompt_profile_dropdown: gtk::DropDown,
    custom_prompt_buffer: gtk::TextBuffer,
    custom_prompt_view: gtk::TextView,
    launch_at_login_switch: gtk::Switch,
    feedback_label: gtk::Label,
    updating_settings: Rc<Cell<bool>>,
}

impl SettingsView {
    pub fn new(surface_state: SettingsSurfaceState) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_margin_top(24);
        root.set_margin_bottom(24);
        root.set_margin_start(24);
        root.set_margin_end(24);
        root.set_valign(Align::Start);

        let title = gtk::Label::builder()
            .label("Settings")
            .xalign(0.0)
            .css_classes(["title-2"])
            .build();
        root.append(&title);

        let microphone_controls = build_microphone_controls("Input", "Level");
        root.append(&microphone_controls);

        let cleanup_section_label = section_label("Cleanup");
        root.append(&cleanup_section_label);
        let cleanup_list = gtk::ListBox::new();
        cleanup_list.add_css_class("boxed-list");
        root.append(&cleanup_list);

        let cleanup_switch = gtk::Switch::builder().valign(Align::Center).build();
        let cleanup_switch_row = adw::ActionRow::builder()
            .title("Enable cleanup")
            .subtitle("Run the local cleanup model after transcription when it is ready")
            .activatable_widget(&cleanup_switch)
            .build();
        cleanup_switch_row.add_suffix(&cleanup_switch);
        cleanup_list.append(&list_box_row(&cleanup_switch_row));

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
        cleanup_list.append(&list_box_row(&prompt_profile_row));

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
        let custom_prompt_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        custom_prompt_box.append(&custom_prompt_title);
        custom_prompt_box.append(&custom_prompt_subtitle);
        custom_prompt_box.append(&custom_prompt_scroller);
        root.append(&custom_prompt_box);

        let general_section_label = section_label("General");
        root.append(&general_section_label);
        let general_list = gtk::ListBox::new();
        general_list.add_css_class("boxed-list");
        root.append(&general_list);

        let launch_at_login_switch = gtk::Switch::builder().valign(Align::Center).build();
        let launch_at_login_row = adw::ActionRow::builder()
            .title("Launch at login")
            .subtitle("Start Pepper X in the background when your GNOME session begins")
            .activatable_widget(&launch_at_login_switch)
            .build();
        launch_at_login_row.add_suffix(&launch_at_login_switch);
        general_list.append(&list_box_row(&launch_at_login_row));

        let feedback_label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .css_classes(["caption"])
            .visible(false)
            .build();
        root.append(&feedback_label);

        let updating_settings = Rc::new(Cell::new(false));
        let view = Self {
            root,
            cleanup_switch,
            prompt_profile_dropdown,
            custom_prompt_buffer,
            custom_prompt_view,
            launch_at_login_switch,
            feedback_label,
            updating_settings,
        };
        view.connect_settings_handlers();
        view.set_surface_state(&surface_state);
        view
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_surface_state(&self, surface_state: &SettingsSurfaceState) {
        self.updating_settings.set(true);
        self.cleanup_switch
            .set_active(surface_state.cleanup_enabled);
        self.launch_at_login_switch
            .set_active(surface_state.launch_at_login);
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
        let cleanup_switch = self.cleanup_switch.clone();
        let prompt_profile_dropdown = self.prompt_profile_dropdown.clone();
        let custom_prompt_buffer = self.custom_prompt_buffer.clone();
        let custom_prompt_view = self.custom_prompt_view.clone();
        let launch_at_login_switch = self.launch_at_login_switch.clone();

        self.cleanup_switch.connect_active_notify({
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

                let cleanup_enabled = switch.is_active();
                prompt_profile_dropdown.set_sensitive(cleanup_enabled);
                custom_prompt_view.set_sensitive(cleanup_enabled);
                custom_prompt_view.set_editable(cleanup_enabled);
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
            title: "Cleanup".into(),
            controls: vec![
                SettingsControl::Switch(SettingsSwitchControl {
                    title: "Enable cleanup".into(),
                    subtitle: "Run the local cleanup model after transcription when it is ready"
                        .into(),
                    active: surface_state.cleanup_enabled,
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
            title: "General".into(),
            controls: vec![SettingsControl::Switch(SettingsSwitchControl {
                title: "Launch at login".into(),
                subtitle: "Start Pepper X in the background when your GNOME session begins".into(),
                active: surface_state.launch_at_login,
            })],
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

pub(crate) fn build_microphone_controls(section_title: &str, meter_label: &str) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);

    let section_label = gtk::Label::builder()
        .label(section_title)
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");

    let picker_row = adw::ActionRow::builder()
        .title("Microphone")
        .subtitle("Choose the input Pepper X should use")
        .build();
    let picker_model = gtk::StringList::new(&[]);
    let picker = gtk::DropDown::new(Some(picker_model.clone()), None::<gtk::Expression>);
    picker.set_hexpand(true);
    picker_row.add_suffix(&picker);
    let picker_list_row = gtk::ListBoxRow::new();
    picker_list_row.set_child(Some(&picker_row));
    list.append(&picker_list_row);

    let level_row = adw::ActionRow::builder()
        .title("Sound check")
        .subtitle("Live input feedback without starting dictation")
        .build();
    let level_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    level_box.set_valign(Align::Center);
    let level_bar = gtk::ProgressBar::new();
    level_bar.set_hexpand(true);
    let level_caption = gtk::Label::builder()
        .label(meter_label)
        .xalign(1.0)
        .css_classes(["caption"])
        .build();
    level_box.append(&level_bar);
    level_box.append(&level_caption);
    level_row.add_suffix(&level_box);
    let level_list_row = gtk::ListBoxRow::new();
    level_list_row.set_child(Some(&level_row));
    list.append(&level_list_row);

    let status_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["caption"])
        .build();

    root.append(&section_label);
    root.append(&list);
    root.append(&status_label);

    let known_devices = Rc::new(RefCell::new(Vec::<MicrophoneDevice>::new()));
    let selected_microphone = Rc::new(RefCell::new(None::<SelectedMicrophone>));
    let updating_picker = Rc::new(Cell::new(false));

    refresh_microphone_controls(
        &picker,
        &picker_model,
        &level_bar,
        &status_label,
        &known_devices,
        &selected_microphone,
        &updating_picker,
        None,
    );

    picker.connect_selected_notify({
        let picker_model = picker_model.clone();
        let known_devices = known_devices.clone();
        let selected_microphone = selected_microphone.clone();
        let status_label = status_label.clone();
        let level_bar = level_bar.clone();
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
            level_bar.set_fraction(0.0);
            status_label.set_label("Sound check updating...");
            refresh_microphone_controls(
                picker,
                &picker_model,
                &level_bar,
                &status_label,
                &known_devices,
                &selected_microphone,
                &updating_picker,
                None,
            );
        }
    });

    start_level_polling(&level_bar, &status_label, &selected_microphone);

    root
}

fn refresh_microphone_controls(
    picker: &gtk::DropDown,
    picker_model: &gtk::StringList,
    level_bar: &gtk::ProgressBar,
    status_label: &gtk::Label,
    known_devices: &Rc<RefCell<Vec<MicrophoneDevice>>>,
    selected_microphone: &Rc<RefCell<Option<SelectedMicrophone>>>,
    updating_picker: &Rc<Cell<bool>>,
    level_probe: Option<Result<pepperx_audio::InputLevelSample, pepperx_audio::InputLevelError>>,
) {
    let ui_state = load_microphone_ui_state(level_probe);
    let status_copy = microphone_status_copy(&ui_state);
    let devices = ui_state.devices.clone();
    let selected = ui_state.selected_microphone.clone();

    updating_picker.set(true);
    picker_model.splice(0, picker_model.n_items(), &[]);
    for device in &devices {
        picker_model.append(device.display_name());
    }
    picker.set_sensitive(!devices.is_empty());
    if let Some(selected_microphone_id) = selected.as_ref().map(|microphone| microphone.stable_id())
    {
        let selected_index = devices
            .iter()
            .position(|device| device.stable_id() == selected_microphone_id)
            .map(|index| index as u32)
            .unwrap_or(gtk::INVALID_LIST_POSITION);
        picker.set_selected(selected_index);
    } else {
        picker.set_selected(gtk::INVALID_LIST_POSITION);
    }
    updating_picker.set(false);

    *known_devices.borrow_mut() = devices;
    *selected_microphone.borrow_mut() = selected;
    level_bar.set_fraction(ui_state.level_fraction);
    status_label.set_label(&status_copy);
}

fn start_level_polling(
    level_bar: &gtk::ProgressBar,
    status_label: &gtk::Label,
    selected_microphone: &Rc<RefCell<Option<SelectedMicrophone>>>,
) {
    let level_bar = level_bar.downgrade();
    let status_label = status_label.downgrade();
    let selected_microphone = selected_microphone.clone();
    let (sender, receiver) =
        mpsc::channel::<Result<pepperx_audio::InputLevelSample, pepperx_audio::InputLevelError>>();
    let in_flight = Rc::new(Cell::new(false));

    gtk::glib::timeout_add_local(Duration::from_millis(750), move || {
        let Some(level_bar) = level_bar.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        let Some(status_label) = status_label.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };

        while let Ok(level_probe) = receiver.try_recv() {
            in_flight.set(false);
            let ui_state = load_microphone_ui_state(Some(level_probe));
            level_bar.set_fraction(ui_state.level_fraction);
            status_label.set_label(&microphone_status_copy(&ui_state));
        }

        if !in_flight.get() && selected_microphone.borrow().is_some() {
            in_flight.set(true);
            let sender = sender.clone();
            let selected_microphone = selected_microphone.borrow().clone();
            std::thread::spawn(move || {
                let _ = sender.send(sample_input_level(selected_microphone));
            });
        }

        gtk::glib::ControlFlow::Continue
    });
}

fn microphone_status_copy(ui_state: &MicrophoneUiState) -> String {
    match ui_state.recovery_message.as_deref() {
        Some(recovery_message) => format!("{}\n{}", ui_state.status_label, recovery_message),
        None => ui_state.status_label.clone(),
    }
}
