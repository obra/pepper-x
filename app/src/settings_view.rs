use adw::prelude::*;
use gtk::Align;
use std::cell::RefCell;

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
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        root.append(&title);
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
