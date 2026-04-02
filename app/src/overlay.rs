use gtk::prelude::*;
use pepperx_ipc::LiveStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayPresentation {
    pub headline: String,
    pub detail: Option<String>,
    pub indicator_label: String,
    pub visible: bool,
    pub busy: bool,
    pub css_class: &'static str,
}

impl OverlayPresentation {
    pub fn from_live_status(status: &LiveStatus) -> Self {
        match status {
            LiveStatus::Ready => Self {
                headline: "Pepper X is ready".into(),
                detail: None,
                indicator_label: "Ready".into(),
                visible: false,
                busy: false,
                css_class: "status-ready",
            },
            LiveStatus::Recording => Self {
                headline: "Recording...".into(),
                detail: None,
                indicator_label: "Recording".into(),
                visible: true,
                busy: true,
                css_class: "status-recording",
            },
            LiveStatus::Transcribing => Self {
                headline: "Transcribing...".into(),
                detail: None,
                indicator_label: "Transcribing".into(),
                visible: true,
                busy: true,
                css_class: "status-working",
            },
            LiveStatus::CleaningUp => Self {
                headline: "Cleaning up...".into(),
                detail: None,
                indicator_label: "Cleaning up".into(),
                visible: true,
                busy: true,
                css_class: "status-working",
            },
            LiveStatus::ClipboardFallback(message) => Self {
                headline: "Copied to clipboard".into(),
                detail: Some(message.clone()),
                indicator_label: "Clipboard fallback".into(),
                visible: true,
                busy: false,
                css_class: "status-success",
            },
            LiveStatus::Error(message) => Self {
                headline: "Pepper X needs attention".into(),
                detail: Some(message.clone()),
                indicator_label: "Error".into(),
                visible: true,
                busy: false,
                css_class: "status-error",
            },
        }
    }
}

const STATUS_CSS_CLASSES: &[&str] = &[
    "status-ready",
    "status-recording",
    "status-working",
    "status-success",
    "status-error",
];

#[derive(Clone)]
pub struct OverlayView {
    root: gtk::Revealer,
    frame: gtk::Box,
    status_dot: gtk::Label,
    spinner: gtk::Spinner,
    headline: gtk::Label,
    detail: gtk::Label,
}

impl OverlayView {
    pub fn new() -> Self {
        let root = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .reveal_child(false)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .build();
        let frame = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        frame.add_css_class("card");
        frame.add_css_class("toolbar");
        frame.set_margin_top(18);
        frame.set_margin_bottom(12);
        frame.set_margin_start(48);
        frame.set_margin_end(48);

        let status_dot = gtk::Label::builder()
            .label("\u{25CF}") // Unicode filled circle
            .build();
        status_dot.set_visible(false);

        let spinner = gtk::Spinner::new();
        spinner.set_spinning(false);
        spinner.set_visible(false);

        let copy_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        copy_box.set_hexpand(true);
        let headline = gtk::Label::builder()
            .xalign(0.0)
            .build();
        headline.add_css_class("title-3");
        let detail = gtk::Label::builder().xalign(0.0).wrap(true).build();
        detail.add_css_class("caption");
        copy_box.append(&headline);
        copy_box.append(&detail);

        frame.append(&status_dot);
        frame.append(&spinner);
        frame.append(&copy_box);
        root.set_child(Some(&frame));

        Self {
            root,
            frame,
            status_dot,
            spinner,
            headline,
            detail,
        }
    }

    pub fn widget(&self) -> &gtk::Revealer {
        &self.root
    }

    pub fn set_live_status(&self, status: &LiveStatus) {
        let presentation = OverlayPresentation::from_live_status(status);
        self.headline.set_label(&presentation.headline);
        self.detail
            .set_label(presentation.detail.as_deref().unwrap_or(""));
        self.detail.set_visible(presentation.detail.is_some());
        self.spinner.set_visible(presentation.busy);
        self.spinner.set_spinning(presentation.busy);

        // Colored status dot: red for recording, green for success, yellow for error
        let dot_color = match status {
            LiveStatus::Recording => Some("#e01b24"),       // red
            LiveStatus::ClipboardFallback(_) => Some("#2ec27e"), // green
            LiveStatus::Error(_) => Some("#e5a50a"),         // yellow
            _ => None,
        };
        if let Some(color) = dot_color {
            self.status_dot
                .set_markup(&format!("<span foreground=\"{color}\">\u{25CF}</span>"));
            self.status_dot.set_visible(true);
        } else {
            self.status_dot.set_visible(false);
        }

        self.root.set_reveal_child(presentation.visible);
    }
}

impl Default for OverlayView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod overlay_state {
    use super::*;

    #[test]
    fn overlay_runtime_states_map_to_user_facing_copy() {
        let recording = OverlayPresentation::from_live_status(&LiveStatus::recording());
        let transcribing = OverlayPresentation::from_live_status(&LiveStatus::transcribing());
        let cleaning_up = OverlayPresentation::from_live_status(&LiveStatus::cleaning_up());

        assert_eq!(recording.headline, "Recording...");
        assert_eq!(recording.indicator_label, "Recording");
        assert!(recording.visible);

        assert_eq!(transcribing.headline, "Transcribing...");
        assert_eq!(transcribing.indicator_label, "Transcribing");
        assert!(transcribing.visible);

        assert_eq!(cleaning_up.headline, "Cleaning up...");
        assert_eq!(cleaning_up.indicator_label, "Cleaning up");
        assert!(cleaning_up.visible);
    }

    #[test]
    fn overlay_indicator_state_tracks_runtime_errors_and_readiness() {
        let ready = OverlayPresentation::from_live_status(&LiveStatus::ready());
        let failure =
            OverlayPresentation::from_live_status(&LiveStatus::error("Clipboard fallback failed"));

        assert_eq!(ready.indicator_label, "Ready");
        assert!(!ready.visible);

        assert_eq!(failure.headline, "Pepper X needs attention");
        assert_eq!(failure.detail.as_deref(), Some("Clipboard fallback failed"));
        assert_eq!(failure.indicator_label, "Error");
        assert!(failure.visible);
    }

    #[test]
    fn overlay_clipboard_fallback_surfaces_message_without_diagnostics_page() {
        let fallback = OverlayPresentation::from_live_status(&LiveStatus::clipboard_fallback(
            "Copied to clipboard. Press Ctrl+V to paste.",
        ));

        assert_eq!(fallback.headline, "Copied to clipboard");
        assert_eq!(
            fallback.detail.as_deref(),
            Some("Copied to clipboard. Press Ctrl+V to paste.")
        );
        assert_eq!(fallback.indicator_label, "Clipboard fallback");
        assert!(fallback.visible);
    }
}
