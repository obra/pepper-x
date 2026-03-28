use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RecordingTriggerMode {
    ModifierOnly,
    StandardShortcut,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSettings {
    pub launch_at_login: bool,
    pub enable_gnome_extension_integration: bool,
    pub preferred_recording_trigger_mode: RecordingTriggerMode,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            enable_gnome_extension_integration: true,
            preferred_recording_trigger_mode: RecordingTriggerMode::ModifierOnly,
        }
    }
}
