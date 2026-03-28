use adw::prelude::*;
use gtk::{Align, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

const SETTINGS_PAGE_NAME: &str = "settings";
const HISTORY_PAGE_NAME: &str = "history";

#[derive(Clone)]
pub struct MainWindow {
    app: adw::Application,
    state: Rc<RefCell<Option<WindowState>>>,
}

struct WindowState {
    window: adw::ApplicationWindow,
    stack: gtk::Stack,
}

impl MainWindow {
    pub fn new(app: &adw::Application) -> Self {
        Self {
            app: app.clone(),
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
            &build_page(
                "Settings",
                "Pepper X shell settings and GNOME integration controls live here.",
            ),
            Some(SETTINGS_PAGE_NAME),
            "Settings",
        );
        stack.add_titled(
            &build_page(
                "History",
                "History, reruns, and diagnostics arrive in a later task.",
            ),
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
