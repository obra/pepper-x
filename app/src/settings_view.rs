use adw::prelude::*;
use gtk::Align;
use pepperx_audio::{sample_input_level, MicrophoneDevice, SelectedMicrophone};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use crate::settings::{load_microphone_ui_state, save_preferred_microphone, MicrophoneUiState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFormRow {
    pub title: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFormSection {
    pub title: String,
    pub rows: Vec<SettingsFormRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsContainerKind {
    Form,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsPageScaffold {
    pub container_kind: SettingsContainerKind,
    pub sections: Vec<SettingsFormSection>,
}

pub struct SettingsView {
    root: gtk::Box,
    rows: RefCell<Vec<gtk::ListBoxRow>>,
    list: gtk::ListBox,
}

impl SettingsView {
    pub fn new(summary: &str) -> Self {
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
        let microphone_controls = build_microphone_controls("Input", "Level");
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        root.append(&title);
        root.append(&microphone_controls);
        root.append(&list);

        let view = Self {
            root,
            rows: RefCell::new(Vec::new()),
            list,
        };
        view.set_summary(summary);
        view
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_summary(&self, summary: &str) {
        let next_rows = settings_page_scaffold(summary)
            .sections
            .into_iter()
            .flat_map(|section| section.rows)
            .map(build_row)
            .collect::<Vec<_>>();

        let mut existing_rows = self.rows.borrow_mut();
        for row in existing_rows.drain(..) {
            self.list.remove(&row);
        }
        for row in &next_rows {
            self.list.append(row);
        }
        *existing_rows = next_rows;
    }
}

pub fn settings_page_scaffold(summary: &str) -> SettingsPageScaffold {
    SettingsPageScaffold {
        container_kind: SettingsContainerKind::Form,
        sections: settings_form_sections(summary),
    }
}

pub fn settings_form_sections(summary: &str) -> Vec<SettingsFormSection> {
    let rows = summary
        .lines()
        .filter_map(parse_summary_row)
        .collect::<Vec<_>>();
    let mut configuration_rows = Vec::new();
    let mut model_rows = Vec::new();

    for row in rows {
        if row.title.starts_with("ASR model ") || row.title.starts_with("Cleanup model ") {
            model_rows.push(row);
        } else {
            configuration_rows.push(row);
        }
    }

    if configuration_rows.is_empty() && model_rows.is_empty() {
        return vec![SettingsFormSection {
            title: "Configuration".into(),
            rows: vec![SettingsFormRow {
                title: "Status".into(),
                value: "Settings summary unavailable.".into(),
            }],
        }];
    }

    let mut sections = Vec::new();
    if !configuration_rows.is_empty() {
        sections.push(SettingsFormSection {
            title: "Configuration".into(),
            rows: configuration_rows,
        });
    }
    if !model_rows.is_empty() {
        sections.push(SettingsFormSection {
            title: "Model bootstrap".into(),
            rows: model_rows,
        });
    }

    sections
}

fn parse_summary_row(line: &str) -> Option<SettingsFormRow> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (title, value) = trimmed
        .split_once(':')
        .map(|(title, value)| (title.trim(), value.trim()))
        .unwrap_or((trimmed, ""));

    Some(SettingsFormRow {
        title: title.into(),
        value: value.into(),
    })
}

fn build_row(row: SettingsFormRow) -> gtk::ListBoxRow {
    let action_row = adw::ActionRow::builder()
        .title(row.title)
        .subtitle(row.value)
        .activatable(false)
        .build();
    let list_row = gtk::ListBoxRow::new();
    list_row.set_child(Some(&action_row));
    list_row
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
