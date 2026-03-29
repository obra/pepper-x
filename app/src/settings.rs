use serde::{Deserialize, Serialize};

use pepperx_audio::SelectedMicrophone;

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
    pub preferred_microphone: Option<SelectedMicrophone>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            enable_gnome_extension_integration: true,
            preferred_recording_trigger_mode: RecordingTriggerMode::ModifierOnly,
            preferred_microphone: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_no_preferred_microphone() {
        let settings = AppSettings::default();

        assert_eq!(settings.preferred_microphone, None);
    }

    #[test]
    fn settings_round_trip_the_preferred_microphone_metadata() {
        let settings = AppSettings {
            preferred_microphone: Some(SelectedMicrophone::new(
                "alsa-input-usb-blue-yeti",
                "Blue Yeti",
            )),
            ..AppSettings::default()
        };

        let json = serde_json::to_string(&settings).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.preferred_microphone, settings.preferred_microphone);
    }

    #[test]
    fn settings_serialize_preferred_microphone_with_explicit_shape() {
        let settings = AppSettings {
            preferred_microphone: Some(SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )),
            ..AppSettings::default()
        };

        let json = serde_json::to_value(&settings).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "launch_at_login": false,
                "enable_gnome_extension_integration": true,
                "preferred_recording_trigger_mode": "modifier-only",
                "preferred_microphone": {
                    "stable_id": "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                    "display_name": "Blue Yeti"
                }
            })
        );
    }
}
