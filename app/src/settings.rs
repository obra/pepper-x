use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

use pepperx_audio::SelectedMicrophone;
use pepperx_models::{default_model, ModelKind};

use crate::transcript_log::state_root;

const SETTINGS_FILE_NAME: &str = "settings.json";
const SETUP_STATE_FILE_NAME: &str = "setup.json";
pub const DEFAULT_CLEANUP_PROMPT_PROFILE: &str = "ordinary-dictation";
pub const LAUNCH_AT_LOGIN_DESKTOP_FILE_NAME: &str = "pepper-x-autostart.desktop";
pub const LAUNCH_AT_LOGIN_DESKTOP_FILE_PATH: &str = "/etc/xdg/autostart/pepper-x-autostart.desktop";

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
    pub preferred_asr_model: String,
    pub preferred_cleanup_model: String,
    pub cleanup_prompt_profile: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSetupState {
    pub onboarding_completed: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            enable_gnome_extension_integration: true,
            preferred_recording_trigger_mode: RecordingTriggerMode::ModifierOnly,
            preferred_microphone: None,
            preferred_asr_model: default_model(ModelKind::Asr).id.into(),
            preferred_cleanup_model: default_model(ModelKind::Cleanup).id.into(),
            cleanup_prompt_profile: DEFAULT_CLEANUP_PROMPT_PROFILE.into(),
        }
    }
}

impl AppSettings {
    pub fn load() -> io::Result<Self> {
        let settings_path = settings_path();
        if !settings_path.is_file() {
            return Ok(Self::default());
        }

        let settings_json = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&settings_json).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "failed to parse Pepper X settings {}: {error}",
                    settings_path.display()
                ),
            )
        })
    }

    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|error| {
            eprintln!("[Pepper X] failed to load settings: {error}");
            Self::default()
        })
    }

    pub fn save(&self) -> io::Result<()> {
        let settings_path = settings_path();
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let settings_json = serde_json::to_string_pretty(self).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to serialize Pepper X settings: {error}"),
            )
        })?;
        std::fs::write(settings_path, settings_json)
    }
}

impl AppSetupState {
    pub fn load() -> io::Result<Self> {
        let setup_state_path = setup_state_path();
        if !setup_state_path.is_file() {
            return Ok(Self::default());
        }

        let setup_state_json = std::fs::read_to_string(&setup_state_path)?;
        serde_json::from_str(&setup_state_json).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "failed to parse Pepper X setup state {}: {error}",
                    setup_state_path.display()
                ),
            )
        })
    }

    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|error| {
            eprintln!("[Pepper X] failed to load setup state: {error}");
            Self::default()
        })
    }

    pub fn save(&self) -> io::Result<()> {
        let setup_state_path = setup_state_path();
        if let Some(parent) = setup_state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let setup_state_json = serde_json::to_string_pretty(self).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to serialize Pepper X setup state: {error}"),
            )
        })?;
        std::fs::write(setup_state_path, setup_state_json)
    }
}

fn settings_path() -> PathBuf {
    state_root().join(SETTINGS_FILE_NAME)
}

fn setup_state_path() -> PathBuf {
    state_root().join(SETUP_STATE_FILE_NAME)
}

pub fn corrections_store_path() -> PathBuf {
    state_root().join("corrections")
}

pub fn launch_at_login_desktop_file_path() -> &'static std::path::Path {
    std::path::Path::new(LAUNCH_AT_LOGIN_DESKTOP_FILE_PATH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_log::env_lock;
    use std::ffi::OsString;
    use std::time::{SystemTime, UNIX_EPOCH};

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
            "pepper-x-settings-test-{}-{unique}",
            std::process::id()
        ))
    }

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
    fn model_status_settings_round_trip_default_models_and_cleanup_prompt_profile() {
        let settings = AppSettings {
            preferred_asr_model: "nemo-parakeet-tdt-0.6b-v2-int8".into(),
            preferred_cleanup_model: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
            cleanup_prompt_profile: "ordinary-dictation".into(),
            ..AppSettings::default()
        };

        let json = serde_json::to_value(&settings).unwrap();
        let restored: AppSettings = serde_json::from_value(json.clone()).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "launch_at_login": false,
                "enable_gnome_extension_integration": true,
                "preferred_recording_trigger_mode": "modifier-only",
                "preferred_microphone": null,
                "preferred_asr_model": "nemo-parakeet-tdt-0.6b-v2-int8",
                "preferred_cleanup_model": "qwen2.5-3b-instruct-q4_k_m.gguf",
                "cleanup_prompt_profile": "ordinary-dictation"
            })
        );
        assert_eq!(restored.preferred_asr_model, settings.preferred_asr_model);
        assert_eq!(
            restored.preferred_cleanup_model,
            settings.preferred_cleanup_model
        );
        assert_eq!(
            restored.cleanup_prompt_profile,
            settings.cleanup_prompt_profile
        );
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
                },
                "preferred_asr_model": "nemo-parakeet-tdt-0.6b-v2-int8",
                "preferred_cleanup_model": "qwen2.5-3b-instruct-q4_k_m.gguf",
                "cleanup_prompt_profile": "ordinary-dictation"
            })
        );
    }

    #[test]
    fn settings_load_preferred_microphone_from_state_root_file() {
        let _guard = env_lock().lock().unwrap();
        let state_root = temp_state_root();
        std::fs::create_dir_all(&state_root).unwrap();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        let expected = AppSettings {
            preferred_microphone: Some(SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )),
            ..AppSettings::default()
        };

        expected.save().expect("settings should save");
        let restored = AppSettings::load().expect("settings should load");

        assert_eq!(restored.preferred_microphone, expected.preferred_microphone);
        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn settings_launch_at_login_points_at_the_packaged_autostart_desktop_file() {
        assert_eq!(
            launch_at_login_desktop_file_path(),
            std::path::Path::new("/etc/xdg/autostart/pepper-x-autostart.desktop")
        );
        assert_eq!(
            LAUNCH_AT_LOGIN_DESKTOP_FILE_NAME,
            "pepper-x-autostart.desktop"
        );
    }

    #[test]
    fn settings_setup_state_defaults_to_incomplete_onboarding() {
        let settings = AppSetupState::default();

        assert!(!settings.onboarding_completed);
    }

    #[test]
    fn settings_setup_state_loads_from_state_root_file() {
        let _guard = env_lock().lock().unwrap();
        let state_root = temp_state_root();
        std::fs::create_dir_all(&state_root).unwrap();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        let expected = AppSetupState {
            onboarding_completed: true,
        };

        expected.save().expect("setup state should save");
        let restored = AppSetupState::load().expect("setup state should load");

        assert_eq!(restored, expected);
        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        let _ = std::fs::remove_dir_all(state_root);
    }
}
