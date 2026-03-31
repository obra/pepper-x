use adw::prelude::*;
use gtk::Align;
use std::cell::RefCell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsCard {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticsContainerKind {
    CardList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsPageScaffold {
    pub container_kind: DiagnosticsContainerKind,
    pub cards: Vec<DiagnosticsCard>,
}

pub struct DiagnosticsView {
    root: gtk::Box,
    rows: RefCell<Vec<gtk::ListBoxRow>>,
    list: gtk::ListBox,
}

impl DiagnosticsView {
    pub fn new(summary: &str) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_margin_top(24);
        root.set_margin_bottom(24);
        root.set_margin_start(24);
        root.set_margin_end(24);
        root.set_valign(Align::Start);

        let title = gtk::Label::builder()
            .label("Diagnostics")
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
        let next_rows = diagnostics_page_scaffold(summary)
            .cards
            .into_iter()
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

pub fn diagnostics_page_scaffold(summary: &str) -> DiagnosticsPageScaffold {
    DiagnosticsPageScaffold {
        container_kind: DiagnosticsContainerKind::CardList,
        cards: diagnostics_cards(summary),
    }
}

pub fn diagnostics_cards(summary: &str) -> Vec<DiagnosticsCard> {
    let cards = summary.lines().filter_map(parse_card).collect::<Vec<_>>();

    if cards.is_empty() {
        vec![DiagnosticsCard {
            title: "Status".into(),
            body: "Diagnostics summary unavailable.".into(),
        }]
    } else {
        cards
    }
}

fn parse_card(line: &str) -> Option<DiagnosticsCard> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (title, body) = trimmed
        .split_once(':')
        .map(|(title, body)| (title.trim(), body.trim()))
        .unwrap_or((trimmed, ""));

    if title == "Modifier-only capture supported" && body == "false" {
        return Some(DiagnosticsCard {
            title: "Modifier-only trigger unavailable".into(),
            body: "Pepper X could not confirm GNOME modifier capture.\nActions: Retry, Open GNOME integration docs, Recheck".into(),
        });
    }

    if title == "Extension connected" && body == "false" {
        return Some(DiagnosticsCard {
            title: "GNOME extension disconnected".into(),
            body: "Pepper X can still run, but the shell indicator is not connected.\nActions: Recheck".into(),
        });
    }

    Some(DiagnosticsCard {
        title: title.into(),
        body: body.into(),
    })
}

fn build_row(card: DiagnosticsCard) -> gtk::ListBoxRow {
    let action_row = adw::ActionRow::builder()
        .title(card.title)
        .subtitle(card.body)
        .activatable(false)
        .build();
    let list_row = gtk::ListBoxRow::new();
    list_row.set_child(Some(&action_row));
    list_row
}
