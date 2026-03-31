use crate::settings::{AppSettings, AppSetupState};
use crate::startup_policy::StartupLaunchPolicy;
use pepperx_ipc::Capabilities;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeReadinessSummary {
    pub modifier_capture_supported: bool,
    pub extension_connected: bool,
    pub service_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppModel {
    pub setup_state: SetupState,
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
        Self {
            setup_state: startup_setup_state(
                setup_state,
                settings,
                capabilities.modifier_only_supported,
            ),
            readiness: RuntimeReadinessSummary::from_capabilities(capabilities),
        }
    }

    pub fn setup_title(&self) -> &'static str {
        match self.setup_state {
            SetupState::SetupRequired => "Finish Pepper X setup",
            SetupState::NeedsAttention(_) => "Fix Pepper X setup",
            SetupState::Ready => "Pepper X is ready",
        }
    }

    pub fn setup_description(&self) -> String {
        match &self.setup_state {
            SetupState::SetupRequired => {
                "Pepper X still needs first-run setup before it can stay in the background."
                    .into()
            }
            SetupState::NeedsAttention(issues) => {
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
        match self.setup_state {
            SetupState::Ready => InitialSurface::Settings,
            SetupState::SetupRequired | SetupState::NeedsAttention(_) => InitialSurface::Setup,
        }
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
