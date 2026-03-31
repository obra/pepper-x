use crate::settings::{AppSettings, AppSetupState, RecordingTriggerMode};
use crate::startup_policy::StartupLaunchPolicy;
use pepperx_ipc::Capabilities;
use pepperx_models::{default_bootstrap_readiness, default_cache_root, BootstrapProgress};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeReadinessSummary {
    pub modifier_capture_supported: bool,
    pub extension_connected: bool,
    pub service_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupChecklist {
    pub trigger_ready: bool,
    pub asr_ready: bool,
    pub cleanup_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelBootstrapSummary {
    pub asr_ready: bool,
    pub cleanup_ready: bool,
    pub progress_label: String,
    pub failure_message: Option<String>,
    pub retry_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSurfaceState {
    pub cleanup_enabled: bool,
    pub cleanup_prompt_profile: String,
    pub cleanup_custom_prompt: String,
    pub launch_at_login: bool,
    pub feedback_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppModel {
    setup_state: Rc<RefCell<SetupState>>,
    trigger_ready: bool,
    model_bootstrap: Rc<RefCell<ModelBootstrapSummary>>,
    pub readiness: RuntimeReadinessSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitialSurface {
    Setup,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupIssue {
    ModifierCaptureUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupState {
    SetupRequired,
    NeedsAttention(Vec<SetupIssue>),
    Ready,
}

impl AppModel {
    pub fn for_startup(
        setup_state: &AppSetupState,
        settings: &AppSettings,
        capabilities: &Capabilities,
    ) -> Self {
        let cache_root = default_cache_root();
        let model_bootstrap = ModelBootstrapSummary::from_default_readiness(
            &default_bootstrap_readiness(&cache_root),
        );
        Self::for_startup_with_model_bootstrap(setup_state, settings, capabilities, model_bootstrap)
    }

    pub fn for_startup_with_model_bootstrap(
        setup_state: &AppSetupState,
        settings: &AppSettings,
        capabilities: &Capabilities,
        model_bootstrap: ModelBootstrapSummary,
    ) -> Self {
        Self {
            trigger_ready: trigger_path_ready(settings, capabilities.modifier_only_supported),
            model_bootstrap: Rc::new(RefCell::new(model_bootstrap)),
            setup_state: Rc::new(RefCell::new(startup_setup_state(
                setup_state,
                settings,
                capabilities.modifier_only_supported,
            ))),
            readiness: RuntimeReadinessSummary::from_capabilities(capabilities),
        }
    }

    pub fn setup_state(&self) -> SetupState {
        self.setup_state.borrow().clone()
    }

    pub fn setup_title(&self) -> &'static str {
        match self.setup_state() {
            SetupState::SetupRequired => "Finish Pepper X setup",
            SetupState::NeedsAttention(_) => "Fix Pepper X setup",
            SetupState::Ready => "Pepper X is ready",
        }
    }

    pub fn setup_description(&self) -> String {
        match self.setup_state() {
            SetupState::SetupRequired => {
                "Pepper X still needs first-run setup before it can stay in the background.".into()
            }
            SetupState::NeedsAttention(ref issues) => {
                let mut details = Vec::new();
                if issues.contains(&SetupIssue::ModifierCaptureUnavailable) {
                    details.push(
                        "Modifier-only capture is unavailable. Keep Pepper X open while you fix GNOME integration or choose a different trigger mode.".to_string(),
                    );
                }

                if details.is_empty() {
                    "Pepper X setup needs attention before live dictation is ready.".into()
                } else {
                    details.join("\n")
                }
            }
            SetupState::Ready => "Pepper X is ready for live dictation.".into(),
        }
    }

    pub fn requested_surface(&self) -> InitialSurface {
        match self.setup_state() {
            SetupState::Ready => InitialSurface::Settings,
            SetupState::SetupRequired | SetupState::NeedsAttention(_) => InitialSurface::Setup,
        }
    }

    pub fn setup_checklist(&self) -> SetupChecklist {
        let model_bootstrap = self.model_bootstrap_summary();
        SetupChecklist::with_model_readiness(
            self.trigger_ready,
            model_bootstrap.asr_ready,
            model_bootstrap.cleanup_ready,
        )
    }

    pub fn mark_onboarding_completed(&self) {
        *self.setup_state.borrow_mut() = SetupState::Ready;
    }

    pub fn model_bootstrap_summary(&self) -> ModelBootstrapSummary {
        self.model_bootstrap.borrow().clone()
    }

    pub fn set_model_bootstrap_summary(&self, summary: ModelBootstrapSummary) {
        *self.model_bootstrap.borrow_mut() = summary;
    }

    pub fn settings_surface_state(&self, settings: &AppSettings) -> SettingsSurfaceState {
        SettingsSurfaceState::from_settings(settings)
    }
}

impl RuntimeReadinessSummary {
    pub fn from_capabilities(capabilities: &Capabilities) -> Self {
        Self {
            modifier_capture_supported: capabilities.modifier_only_supported,
            extension_connected: capabilities.extension_connected,
            service_version: capabilities.version.clone(),
        }
    }
}

impl SetupChecklist {
    pub fn new(trigger_ready: bool) -> Self {
        Self {
            trigger_ready,
            asr_ready: true,
            cleanup_ready: true,
        }
    }

    pub fn with_model_readiness(trigger_ready: bool, asr_ready: bool, cleanup_ready: bool) -> Self {
        Self {
            trigger_ready,
            asr_ready,
            cleanup_ready,
        }
    }

    pub fn completed_items(&self) -> usize {
        usize::from(self.trigger_ready) + usize::from(self.asr_ready)
    }

    pub fn total_items(&self) -> usize {
        2
    }

    pub fn is_complete(&self) -> bool {
        self.trigger_ready && self.asr_ready
    }
}

impl ModelBootstrapSummary {
    pub fn ready() -> Self {
        Self {
            asr_ready: true,
            cleanup_ready: true,
            progress_label: "Default models ready".into(),
            failure_message: None,
            retry_available: false,
        }
    }

    pub fn from_default_readiness(readiness: &pepperx_models::DefaultBootstrapReadiness) -> Self {
        let progress_label = if !readiness.asr.is_ready {
            "Default ASR model download pending".into()
        } else if !readiness.cleanup.is_ready {
            "Cleanup model download pending".into()
        } else {
            "Default models ready".into()
        };

        Self {
            asr_ready: readiness.asr.is_ready,
            cleanup_ready: readiness.cleanup.is_ready,
            progress_label,
            failure_message: None,
            retry_available: false,
        }
    }

    pub fn from_progress(progress: &BootstrapProgress) -> Self {
        let asr_ready = progress.model_states.iter().any(|state| {
            state.kind == pepperx_models::ModelKind::Asr
                && state.phase == pepperx_models::BootstrapModelPhase::Ready
        });
        let cleanup_ready = progress.model_states.iter().any(|state| {
            state.kind == pepperx_models::ModelKind::Cleanup
                && state.phase == pepperx_models::BootstrapModelPhase::Ready
        });
        let progress_label = if let Some(model_id) = progress.current_model_id.as_deref() {
            format!("Downloading {model_id}")
        } else if let Some(failure_message) = progress.failure_message.as_deref() {
            if asr_ready {
                format!("Cleanup bootstrap failed: {failure_message}")
            } else {
                "Default ASR download failed".into()
            }
        } else if asr_ready && cleanup_ready {
            "Default models ready".into()
        } else if asr_ready {
            "Cleanup model bootstrapping in background".into()
        } else {
            "Downloading default ASR model".into()
        };

        Self {
            asr_ready,
            cleanup_ready,
            progress_label,
            failure_message: progress.failure_message.clone(),
            retry_available: progress.failure_message.is_some(),
        }
    }
}

impl SettingsSurfaceState {
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            cleanup_enabled: settings.cleanup_enabled,
            cleanup_prompt_profile: settings.cleanup_prompt_profile.clone(),
            cleanup_custom_prompt: settings.cleanup_custom_prompt.clone(),
            launch_at_login: settings.launch_at_login,
            feedback_message: None,
        }
    }
}

pub fn startup_setup_state(
    setup_state: &AppSetupState,
    settings: &AppSettings,
    modifier_capture_supported: bool,
) -> SetupState {
    if !setup_state.onboarding_completed {
        return SetupState::SetupRequired;
    }

    let mut issues = Vec::new();
    if settings.enable_gnome_extension_integration
        && settings.preferred_recording_trigger_mode
            == crate::settings::RecordingTriggerMode::ModifierOnly
        && !modifier_capture_supported
    {
        issues.push(SetupIssue::ModifierCaptureUnavailable);
    }

    if issues.is_empty() {
        SetupState::Ready
    } else {
        SetupState::NeedsAttention(issues)
    }
}

fn trigger_path_ready(settings: &AppSettings, modifier_capture_supported: bool) -> bool {
    !matches!(
        (
            settings.enable_gnome_extension_integration,
            &settings.preferred_recording_trigger_mode,
            modifier_capture_supported,
        ),
        (true, RecordingTriggerMode::ModifierOnly, false)
    )
}

pub fn initial_surface(
    startup_launch_policy: StartupLaunchPolicy,
    skipped_initial_background_activation: bool,
    setup_state: SetupState,
) -> Option<InitialSurface> {
    match setup_state {
        SetupState::SetupRequired | SetupState::NeedsAttention(_) => Some(InitialSurface::Setup),
        SetupState::Ready => match startup_launch_policy {
            StartupLaunchPolicy::Interactive => Some(InitialSurface::Settings),
            StartupLaunchPolicy::Background if skipped_initial_background_activation => {
                Some(InitialSurface::Settings)
            }
            StartupLaunchPolicy::Background => None,
        },
    }
}
