use adw::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;

use crate::app_model::{AppModel, SetupChecklist};
use crate::settings::AppSetupState;
use crate::transcript_log::state_root;

const ONBOARDING_PROGRESS_FILE_NAME: &str = "onboarding.json";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OnboardingStep {
    #[default]
    Welcome,
    Setup,
    TryIt,
    Done,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnboardingProgress {
    pub current_step: OnboardingStep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OnboardingStepView {
    title: &'static str,
    body: String,
    progress_label: String,
    primary_label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingWizard {
    progress: OnboardingProgress,
}

impl OnboardingProgress {
    pub fn load() -> io::Result<Self> {
        let progress_path = onboarding_progress_path();
        if !progress_path.is_file() {
            return Ok(Self::default());
        }

        let progress_json = std::fs::read_to_string(&progress_path)?;
        serde_json::from_str(&progress_json).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "failed to parse Pepper X onboarding progress {}: {error}",
                    progress_path.display()
                ),
            )
        })
    }

    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|error| {
            eprintln!("[Pepper X] failed to load onboarding progress: {error}");
            Self::default()
        })
    }

    pub fn save(&self) -> io::Result<()> {
        let progress_path = onboarding_progress_path();
        if let Some(parent) = progress_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let progress_json = serde_json::to_string_pretty(self).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to serialize Pepper X onboarding progress: {error}"),
            )
        })?;
        std::fs::write(progress_path, progress_json)
    }
}

impl OnboardingWizard {
    pub fn resume(
        setup_state: &AppSetupState,
        progress: OnboardingProgress,
        requires_setup: bool,
    ) -> Self {
        let current_step = if setup_state.onboarding_completed {
            if requires_setup {
                OnboardingStep::Setup
            } else {
                OnboardingStep::Done
            }
        } else if progress.current_step == OnboardingStep::Done {
            OnboardingStep::Setup
        } else {
            progress.current_step
        };

        Self {
            progress: OnboardingProgress { current_step },
        }
    }

    pub fn current_step(&self) -> OnboardingStep {
        self.progress.current_step
    }

    pub fn advance(&mut self, checklist: &SetupChecklist) -> Result<(), &'static str> {
        self.progress.current_step = match self.progress.current_step {
            OnboardingStep::Welcome => OnboardingStep::Setup,
            OnboardingStep::Setup if checklist.is_complete() => OnboardingStep::TryIt,
            OnboardingStep::Setup => return Err("setup checklist is incomplete"),
            OnboardingStep::TryIt if checklist.is_complete() => OnboardingStep::Done,
            OnboardingStep::TryIt => return Err("runtime is not ready for try-it"),
            OnboardingStep::Done => OnboardingStep::Done,
        };

        Ok(())
    }

    pub fn finish(&mut self, setup_state: &mut AppSetupState) {
        setup_state.onboarding_completed = true;
        self.progress.current_step = OnboardingStep::Done;
    }

    pub fn save_progress(&self) -> io::Result<()> {
        self.progress.save()
    }

    fn can_continue(&self, checklist: &SetupChecklist) -> bool {
        match self.progress.current_step {
            OnboardingStep::Setup | OnboardingStep::TryIt => checklist.is_complete(),
            OnboardingStep::Welcome | OnboardingStep::Done => true,
        }
    }

    fn step_view(&self, checklist: &SetupChecklist) -> OnboardingStepView {
        match self.progress.current_step {
            OnboardingStep::Welcome => OnboardingStepView {
                title: "Welcome to Pepper X",
                body: "GNOME-first local dictation for Linux.\n\nThis setup will walk you from zero to your first live hold-to-talk run.".into(),
                progress_label: "Step 1 of 4".into(),
                primary_label: "Get Started",
            },
            OnboardingStep::Setup => OnboardingStepView {
                title: "Finish setup",
                body: format!(
                    "Setup checklist: {}/{} complete\n\nRecording trigger available: {}\n\nContinue unlocks once the trigger path is ready.",
                    checklist.completed_items(),
                    checklist.total_items(),
                    if checklist.trigger_ready { "Yes" } else { "No" }
                ),
                progress_label: "Step 2 of 4".into(),
                primary_label: "Continue",
            },
            OnboardingStep::TryIt => OnboardingStepView {
                title: "Try it",
                body: format!(
                    "Hold Pepper X's trigger and speak once the trigger path is ready.\n\nTrigger ready: {}\n\nFor this first GNOME parity slice, you can continue manually after confirming the path is available.",
                    if checklist.trigger_ready { "Yes" } else { "No" }
                ),
                progress_label: "Step 3 of 4".into(),
                primary_label: "Continue",
            },
            OnboardingStep::Done => OnboardingStepView {
                title: "Pepper X is ready",
                body: "Setup is complete. Pepper X can stay in the background and wait for hold-to-talk.".into(),
                progress_label: "Step 4 of 4".into(),
                primary_label: "Start Using Pepper X",
            },
        }
    }
}

pub fn show_onboarding_window(
    app: &adw::Application,
    app_model: &AppModel,
    on_complete: impl Fn() + 'static,
) -> adw::ApplicationWindow {
    let setup_state = AppSetupState::load_or_default();
    let wizard = Rc::new(RefCell::new(OnboardingWizard::resume(
        &setup_state,
        OnboardingProgress::load_or_default(),
        app_model.requested_surface() == crate::app_model::InitialSurface::Setup,
    )));
    let checklist_provider: Rc<dyn Fn() -> SetupChecklist> = {
        let app_model = app_model.clone();
        Rc::new(move || app_model.setup_checklist())
    };

    let progress_label = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["caption-heading"])
        .build();
    let title_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["title-1"])
        .build();
    let body_label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .build();
    let primary_button = gtk::Button::builder().halign(gtk::Align::End).build();

    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&progress_label);
    content.append(&title_label);
    content.append(&body_label);
    content.append(&primary_button);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Pepper X Setup")
        .default_width(480)
        .default_height(420)
        .content(&content)
        .build();

    let refresh_ui: Rc<dyn Fn()> = {
        let wizard = wizard.clone();
        let checklist_provider = checklist_provider.clone();
        let progress_label = progress_label.clone();
        let title_label = title_label.clone();
        let body_label = body_label.clone();
        let primary_button = primary_button.clone();
        Rc::new(move || {
            let checklist = checklist_provider();
            let step_view = wizard.borrow().step_view(&checklist);
            progress_label.set_label(&step_view.progress_label);
            title_label.set_label(step_view.title);
            body_label.set_label(&step_view.body);
            primary_button.set_label(step_view.primary_label);
            primary_button.set_sensitive(wizard.borrow().can_continue(&checklist));
        })
    };
    refresh_ui();

    primary_button.connect_clicked({
        let wizard = wizard.clone();
        let checklist_provider = checklist_provider.clone();
        let refresh_ui = refresh_ui.clone();
        let window = window.clone();
        let on_complete = Rc::new(on_complete);
        move |_| {
            let mut wizard = wizard.borrow_mut();
            if wizard.current_step() == OnboardingStep::Done {
                let mut setup_state = AppSetupState::load_or_default();
                wizard.finish(&mut setup_state);
                let _ = setup_state.save();
                let _ = wizard.save_progress();
                window.close();
                on_complete();
                return;
            }

            if wizard.advance(&checklist_provider()).is_ok() {
                let _ = wizard.save_progress();
                drop(wizard);
                refresh_ui();
            }
        }
    });

    window.present();
    window
}

fn onboarding_progress_path() -> PathBuf {
    state_root().join(ONBOARDING_PROGRESS_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_log::env_lock;
    use std::ffi::OsString;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn ready_checklist() -> SetupChecklist {
        SetupChecklist::new(true)
    }

    fn set_or_remove_env_var(name: &str, value: Option<OsString>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn temp_state_root() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pepper-x-onboarding-test-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn onboarding_welcome_setup_try_it_done_progression() {
        let mut wizard = OnboardingWizard::resume(
            &AppSetupState::default(),
            OnboardingProgress::default(),
            false,
        );

        assert_eq!(wizard.current_step(), OnboardingStep::Welcome);
        wizard
            .advance(&ready_checklist())
            .expect("welcome -> setup");
        assert_eq!(wizard.current_step(), OnboardingStep::Setup);
        wizard.advance(&ready_checklist()).expect("setup -> try-it");
        assert_eq!(wizard.current_step(), OnboardingStep::TryIt);
        wizard.advance(&ready_checklist()).expect("try-it -> done");
        assert_eq!(wizard.current_step(), OnboardingStep::Done);
    }

    #[test]
    fn onboarding_persists_completion_state() {
        let mut setup_state = AppSetupState::default();
        let mut wizard = OnboardingWizard::resume(
            &setup_state,
            OnboardingProgress {
                current_step: OnboardingStep::Done,
            },
            false,
        );

        wizard.finish(&mut setup_state);

        assert!(setup_state.onboarding_completed);
    }

    #[test]
    fn onboarding_reopens_from_partial_or_failed_progress() {
        let setup_state = AppSetupState::default();

        let partial = OnboardingWizard::resume(
            &setup_state,
            OnboardingProgress {
                current_step: OnboardingStep::TryIt,
            },
            false,
        );
        assert_eq!(partial.current_step(), OnboardingStep::TryIt);

        let failed = OnboardingWizard::resume(
            &setup_state,
            OnboardingProgress {
                current_step: OnboardingStep::Setup,
            },
            false,
        );
        assert_eq!(failed.current_step(), OnboardingStep::Setup);
    }

    #[test]
    fn onboarding_completed_users_reopen_setup_when_runtime_requires_attention() {
        let setup_state = AppSetupState {
            onboarding_completed: true,
        };

        let resumed = OnboardingWizard::resume(&setup_state, OnboardingProgress::default(), true);

        assert_eq!(resumed.current_step(), OnboardingStep::Setup);
    }

    #[test]
    fn onboarding_try_it_requires_ready_runtime() {
        let setup_state = AppSetupState::default();
        let mut wizard = OnboardingWizard::resume(
            &setup_state,
            OnboardingProgress {
                current_step: OnboardingStep::TryIt,
            },
            false,
        );

        assert!(wizard.advance(&SetupChecklist::new(false)).is_err());
        assert_eq!(wizard.current_step(), OnboardingStep::TryIt);
    }

    #[test]
    fn onboarding_setup_requires_ready_trigger_path() {
        let setup_state = AppSetupState::default();
        let mut wizard =
            OnboardingWizard::resume(&setup_state, OnboardingProgress::default(), true);

        wizard
            .advance(&ready_checklist())
            .expect("welcome should advance to setup");

        assert!(wizard.advance(&SetupChecklist::new(false)).is_err());
        assert_eq!(wizard.current_step(), OnboardingStep::Setup);
    }

    #[test]
    fn onboarding_progress_round_trips_from_state_root_file() {
        let _guard = env_lock().lock().unwrap();
        let state_root = temp_state_root();
        std::fs::create_dir_all(&state_root).unwrap();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        let expected = OnboardingProgress {
            current_step: OnboardingStep::TryIt,
        };

        expected.save().expect("progress should save");
        let restored = OnboardingProgress::load().expect("progress should load");

        assert_eq!(restored, expected);
        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        let _ = std::fs::remove_dir_all(state_root);
    }
}
