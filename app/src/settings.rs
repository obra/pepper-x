use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

use pepperx_audio::{
    enumerate_microphones, InputLevelError, InputLevelErrorKind, InputLevelSample,
    MicrophoneDevice, MicrophoneInventory, SelectedMicrophone,
};
use pepperx_models::{default_model, ModelKind};

use crate::transcript_log::state_root;

const SETTINGS_FILE_NAME: &str = "settings.json";
const SETUP_STATE_FILE_NAME: &str = "setup.json";
pub const DEFAULT_CLEANUP_PROMPT_PROFILE: &str = "ordinary-dictation";
pub const LAUNCH_AT_LOGIN_DESKTOP_FILE_NAME: &str = "pepper-x-autostart.desktop";
pub const LAUNCH_AT_LOGIN_DESKTOP_FILE_PATH: &str = "/etc/xdg/autostart/pepper-x-autostart.desktop";
const LAUNCH_AT_LOGIN_DESKTOP_TEMPLATE: &str =
    include_str!("../../packaging/deb/pepper-x-autostart.desktop");

fn default_play_sounds() -> bool {
    true
}

fn default_enable_post_paste_learning() -> bool {
    true
}

fn default_hold_trigger_keys() -> String {
    "56,100,125,126".into()
}

fn default_toggle_trigger_keys() -> String {
    "56,57,100,125,126".into()
}

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
    pub cleanup_enabled: bool,
    pub preferred_asr_model: String,
    pub preferred_cleanup_model: String,
    pub cleanup_prompt_profile: String,
    pub cleanup_custom_prompt: String,
    #[serde(default = "default_play_sounds")]
    pub play_sounds: bool,
    #[serde(default)]
    pub enable_window_context: bool,
    #[serde(default = "default_hold_trigger_keys")]
    pub hold_trigger_keys: String,
    #[serde(default = "default_toggle_trigger_keys")]
    pub toggle_trigger_keys: String,
    #[serde(default)]
    pub ignore_other_speakers: bool,
    #[serde(default = "default_enable_post_paste_learning")]
    pub enable_post_paste_learning: bool,
    /// Legacy field — migrated to `hold_trigger_keys` on load.
    #[serde(default, skip_serializing)]
    pub(crate) preferred_trigger_keys: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppSetupState {
    pub onboarding_completed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MicrophoneUiState {
    pub devices: Vec<MicrophoneDevice>,
    pub selected_microphone: Option<SelectedMicrophone>,
    pub level_fraction: f64,
    pub status_label: String,
    pub recovery_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicrophoneSelectionState {
    pub available_microphones: Vec<SelectedMicrophone>,
    pub selected_microphone: Option<SelectedMicrophone>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            enable_gnome_extension_integration: true,
            preferred_recording_trigger_mode: RecordingTriggerMode::ModifierOnly,
            preferred_microphone: None,
            cleanup_enabled: true,
            preferred_asr_model: default_model(ModelKind::Asr).id.into(),
            preferred_cleanup_model: default_model(ModelKind::Cleanup).id.into(),
            cleanup_prompt_profile: DEFAULT_CLEANUP_PROMPT_PROFILE.into(),
            cleanup_custom_prompt: String::new(),
            play_sounds: true,
            enable_window_context: false,
            hold_trigger_keys: default_hold_trigger_keys(),
            toggle_trigger_keys: default_toggle_trigger_keys(),
            ignore_other_speakers: false,
            enable_post_paste_learning: true,
            preferred_trigger_keys: None,
        }
    }
}

impl AppSettings {
    pub fn load() -> io::Result<Self> {
        let settings_path = settings_path();
        if !settings_path.is_file() {
            let mut settings = Self::default();
            if let Ok(launch_at_login) = current_launch_at_login_state() {
                settings.launch_at_login = launch_at_login;
            }
            return Ok(settings);
        }

        let settings_json = std::fs::read_to_string(&settings_path)?;
        let mut settings: Self = serde_json::from_str(&settings_json).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "failed to parse Pepper X settings {}: {error}",
                    settings_path.display()
                ),
            )
        })?;
        // Migrate legacy preferred_trigger_keys -> hold_trigger_keys
        if let Some(legacy_keys) = settings.preferred_trigger_keys.take() {
            if settings.hold_trigger_keys == default_hold_trigger_keys() {
                settings.hold_trigger_keys = legacy_keys;
            }
        }
        if let Ok(launch_at_login) = current_launch_at_login_state() {
            settings.launch_at_login = launch_at_login;
        }
        Ok(settings)
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

    pub fn effective_cleanup_custom_prompt(&self) -> Option<String> {
        self.cleanup_custom_prompt
            .chars()
            .any(|character| !character.is_whitespace())
            .then(|| self.cleanup_custom_prompt.clone())
    }

    pub fn microphone_selection_state(
        &self,
        inventory: &MicrophoneInventory,
    ) -> MicrophoneSelectionState {
        MicrophoneSelectionState {
            available_microphones: inventory
                .devices()
                .iter()
                .map(SelectedMicrophone::from)
                .collect(),
            selected_microphone: inventory.resolve_selected(self.preferred_microphone.as_ref()),
        }
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

pub fn user_launch_at_login_desktop_file_path() -> io::Result<PathBuf> {
    Ok(user_config_home()?
        .join("autostart")
        .join(LAUNCH_AT_LOGIN_DESKTOP_FILE_NAME))
}

pub fn save_launch_at_login(enabled: bool) -> io::Result<()> {
    let autostart_path = user_launch_at_login_desktop_file_path()?;
    if enabled {
        if let Some(parent) = autostart_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&autostart_path, launch_at_login_desktop_contents(true))?;
    } else if autostart_path.exists() {
        std::fs::remove_file(&autostart_path)?;
    }

    let mut settings = AppSettings::load_or_default();
    settings.launch_at_login = enabled;
    settings.save()
}

pub fn save_preferred_microphone(
    selected_microphone: Option<SelectedMicrophone>,
) -> io::Result<()> {
    let mut settings = AppSettings::load_or_default();
    settings.preferred_microphone = selected_microphone;
    settings.save()
}

fn user_config_home() -> io::Result<PathBuf> {
    if let Some(xdg_config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg_config_home));
    }

    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

fn current_launch_at_login_state() -> io::Result<bool> {
    let autostart_path = user_launch_at_login_desktop_file_path()?;
    if !autostart_path.is_file() {
        return Ok(false);
    }

    let contents = std::fs::read_to_string(autostart_path)?;
    Ok(
        match contents
            .lines()
            .find_map(|line| line.strip_prefix("X-GNOME-Autostart-enabled="))
        {
            Some(value) => value.trim() == "true",
            None => true,
        },
    )
}

fn launch_at_login_desktop_contents(enabled: bool) -> String {
    LAUNCH_AT_LOGIN_DESKTOP_TEMPLATE.replace(
        "X-GNOME-Autostart-enabled=false",
        &format!("X-GNOME-Autostart-enabled={enabled}"),
    )
}

pub fn microphone_ui_state(
    settings: &AppSettings,
    inventory: &MicrophoneInventory,
    level_probe: Option<Result<InputLevelSample, InputLevelError>>,
) -> MicrophoneUiState {
    let selection = settings.microphone_selection_state(inventory);
    let devices = inventory.devices().to_vec();

    if devices.is_empty() {
        return MicrophoneUiState {
            devices,
            selected_microphone: None,
            level_fraction: 0.0,
            status_label: "No microphone detected.".into(),
            recovery_message: Some("Connect a microphone to continue.".into()),
        };
    }

    match level_probe {
        Some(Ok(sample)) => MicrophoneUiState {
            devices,
            selected_microphone: selection.selected_microphone,
            level_fraction: sample.normalized_level() as f64,
            status_label: "Sound check looks healthy.".into(),
            recovery_message: None,
        },
        Some(Err(error)) => {
            let (status_label, recovery_message) = match error.kind() {
                InputLevelErrorKind::NoSignal => (
                    "No sound detected.".into(),
                    Some("Check the selected microphone and speak again.".into()),
                ),
                InputLevelErrorKind::UnsupportedPlatform => (
                    "Microphone checks are unavailable.".into(),
                    Some(error.to_string()),
                ),
                InputLevelErrorKind::CaptureFailed => {
                    ("Microphone check failed.".into(), Some(error.to_string()))
                }
            };

            MicrophoneUiState {
                devices,
                selected_microphone: selection.selected_microphone,
                level_fraction: 0.0,
                status_label,
                recovery_message,
            }
        }
        None => MicrophoneUiState {
            devices,
            selected_microphone: selection.selected_microphone,
            level_fraction: 0.0,
            status_label: "Run a sound check.".into(),
            recovery_message: None,
        },
    }
}

pub fn load_microphone_ui_state(
    level_probe: Option<Result<InputLevelSample, InputLevelError>>,
) -> MicrophoneUiState {
    let settings = AppSettings::load_or_default();
    match enumerate_microphones() {
        Ok(inventory) => microphone_ui_state(&settings, &inventory, level_probe),
        Err(error) => MicrophoneUiState {
            devices: Vec::new(),
            selected_microphone: settings.preferred_microphone,
            level_fraction: 0.0,
            status_label: "Microphone list unavailable.".into(),
            recovery_message: Some(error.to_string()),
        },
    }
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
            cleanup_enabled: true,
            preferred_asr_model: "nemo-parakeet-tdt-0.6b-v3-int8".into(),
            preferred_cleanup_model: "qwen3.5-2b-q4_k_m.gguf".into(),
            cleanup_prompt_profile: "ordinary-dictation".into(),
            cleanup_custom_prompt: String::new(),
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
                "cleanup_enabled": true,
                "preferred_asr_model": "nemo-parakeet-tdt-0.6b-v3-int8",
                "preferred_cleanup_model": "qwen3.5-2b-q4_k_m.gguf",
                "cleanup_prompt_profile": "ordinary-dictation",
                "cleanup_custom_prompt": "",
                "play_sounds": true,
                "enable_window_context": false,
                "hold_trigger_keys": "56,100,125,126",
                "toggle_trigger_keys": "56,57,100,125,126",
                "ignore_other_speakers": false,
                "enable_post_paste_learning": true
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
        assert_eq!(restored.cleanup_enabled, settings.cleanup_enabled);
        assert_eq!(
            restored.cleanup_custom_prompt,
            settings.cleanup_custom_prompt
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
                "cleanup_enabled": true,
                "preferred_asr_model": "nemotron-speech-streaming-en-0.6b",
                "preferred_cleanup_model": "qwen3.5-2b-q4_k_m.gguf",
                "cleanup_prompt_profile": "ordinary-dictation",
                "cleanup_custom_prompt": "",
                "play_sounds": true,
                "enable_window_context": false,
                "hold_trigger_keys": "56,100,125,126",
                "toggle_trigger_keys": "56,57,100,125,126",
                "ignore_other_speakers": false,
                "enable_post_paste_learning": true
            })
        );
    }

    #[test]
    fn settings_round_trip_cleanup_toggle_launch_at_login_and_custom_prompt() {
        let settings = AppSettings {
            launch_at_login: true,
            cleanup_enabled: false,
            cleanup_prompt_profile: "literal-dictation".into(),
            cleanup_custom_prompt: "Keep product names verbatim.".into(),
            ..AppSettings::default()
        };

        let json = serde_json::to_value(&settings).unwrap();
        let restored: AppSettings = serde_json::from_value(json.clone()).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "launch_at_login": true,
                "enable_gnome_extension_integration": true,
                "preferred_recording_trigger_mode": "modifier-only",
                "preferred_microphone": null,
                "cleanup_enabled": false,
                "preferred_asr_model": "nemotron-speech-streaming-en-0.6b",
                "preferred_cleanup_model": "qwen3.5-2b-q4_k_m.gguf",
                "cleanup_prompt_profile": "literal-dictation",
                "cleanup_custom_prompt": "Keep product names verbatim.",
                "play_sounds": true,
                "enable_window_context": false,
                "hold_trigger_keys": "56,100,125,126",
                "toggle_trigger_keys": "56,57,100,125,126",
                "ignore_other_speakers": false,
                "enable_post_paste_learning": true
            })
        );
        assert!(restored.launch_at_login);
        assert!(!restored.cleanup_enabled);
        assert_eq!(restored.cleanup_prompt_profile, "literal-dictation");
        assert_eq!(
            restored.cleanup_custom_prompt,
            "Keep product names verbatim."
        );
    }

    #[test]
    fn settings_round_trip_preserves_custom_prompt_whitespace() {
        let custom_prompt = "\n  Keep product names verbatim.\n\nDo not normalize punctuation.\n";
        let settings = AppSettings {
            cleanup_custom_prompt: custom_prompt.into(),
            ..AppSettings::default()
        };

        let json = serde_json::to_value(&settings).unwrap();
        let restored: AppSettings = serde_json::from_value(json).unwrap();

        assert_eq!(restored.cleanup_custom_prompt, custom_prompt);
        assert_eq!(
            restored.effective_cleanup_custom_prompt().as_deref(),
            Some(custom_prompt)
        );

        let whitespace_only_settings = AppSettings {
            cleanup_custom_prompt: " \n\t ".into(),
            ..AppSettings::default()
        };
        assert_eq!(
            whitespace_only_settings.effective_cleanup_custom_prompt(),
            None
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
    fn settings_microphone_ui_state_prefers_the_saved_microphone_when_available() {
        let settings = AppSettings {
            preferred_microphone: Some(SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )),
            ..AppSettings::default()
        };
        let inventory = MicrophoneInventory::from_devices(vec![
            MicrophoneDevice::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            ),
            MicrophoneDevice::new(
                "pipewire:node.name=alsa_input.pci-built-in-00.analog-stereo",
                "Built-in Audio",
            ),
        ]);

        let state = microphone_ui_state(
            &settings,
            &inventory,
            Some(Ok(InputLevelSample::from_pcm_samples(&[0, 14_745]))),
        );

        assert_eq!(state.selected_microphone, settings.preferred_microphone);
        assert_eq!(state.devices.len(), 2);
        assert_eq!(state.status_label, "Sound check looks healthy.");
        assert!(state.recovery_message.is_none());
        assert!(state.level_fraction > 0.4);
    }

    #[test]
    fn settings_microphone_ui_state_falls_back_to_the_first_available_microphone() {
        let inventory = MicrophoneInventory::from_devices(vec![
            MicrophoneDevice::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            ),
            MicrophoneDevice::new(
                "pipewire:node.name=alsa_input.pci-built-in-00.analog-stereo",
                "Built-in Audio",
            ),
        ]);

        let state = microphone_ui_state(
            &AppSettings {
                preferred_microphone: Some(SelectedMicrophone::new(
                    "pipewire:node.name=alsa_input.missing-device",
                    "Missing Device",
                )),
                ..AppSettings::default()
            },
            &inventory,
            None,
        );

        assert_eq!(
            state.selected_microphone,
            Some(SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            ))
        );
        assert_eq!(state.status_label, "Run a sound check.");
        assert_eq!(state.recovery_message, None);
    }

    #[test]
    fn settings_microphone_ui_state_surfaces_no_signal_recovery_copy() {
        let inventory = MicrophoneInventory::from_devices(vec![MicrophoneDevice::new(
            "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
            "Blue Yeti",
        )]);

        let state = microphone_ui_state(
            &AppSettings::default(),
            &inventory,
            Some(Err(InputLevelError::new(
                InputLevelErrorKind::NoSignal,
                "Pepper X did not detect microphone signal",
            ))),
        );

        assert_eq!(state.level_fraction, 0.0);
        assert_eq!(state.status_label, "No sound detected.");
        assert_eq!(
            state.recovery_message.as_deref(),
            Some("Check the selected microphone and speak again.")
        );
    }

    #[test]
    fn settings_microphone_selection_prefers_saved_device_when_inventory_contains_it() {
        let settings = AppSettings {
            preferred_microphone: Some(SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )),
            ..AppSettings::default()
        };
        let inventory = pepperx_audio::MicrophoneInventory::from_devices(vec![
            pepperx_audio::MicrophoneDevice::new(
                "pipewire:node.name=alsa_input.pci-0000_00_1f.3.analog-stereo",
                "Built-in Audio",
            ),
            pepperx_audio::MicrophoneDevice::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            ),
        ]);

        let selection = settings.microphone_selection_state(&inventory);

        assert_eq!(
            selection.available_microphones,
            vec![
                SelectedMicrophone::new(
                    "pipewire:node.name=alsa_input.pci-0000_00_1f.3.analog-stereo",
                    "Built-in Audio",
                ),
                SelectedMicrophone::new(
                    "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                    "Blue Yeti",
                ),
            ]
        );
        assert_eq!(selection.selected_microphone, settings.preferred_microphone);
    }

    #[test]
    fn settings_microphone_selection_falls_back_to_first_live_device_when_saved_one_is_missing() {
        let settings = AppSettings {
            preferred_microphone: Some(SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-missing-00.analog-stereo",
                "Missing Mic",
            )),
            ..AppSettings::default()
        };
        let inventory = pepperx_audio::MicrophoneInventory::from_devices(vec![
            pepperx_audio::MicrophoneDevice::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            ),
            pepperx_audio::MicrophoneDevice::new(
                "pipewire:node.name=alsa_input.usb-rode-00.analog-stereo",
                "Rode NT-USB",
            ),
        ]);

        let selection = settings.microphone_selection_state(&inventory);

        assert_eq!(
            selection.selected_microphone,
            Some(SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            ))
        );
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
    fn settings_launch_at_login_writes_enabled_user_autostart_entry() {
        let _guard = env_lock().lock().unwrap();
        let state_root = temp_state_root();
        let config_root = state_root.join("config");
        std::fs::create_dir_all(&state_root).unwrap();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        let previous_xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        std::env::set_var("XDG_CONFIG_HOME", &config_root);

        save_launch_at_login(true).expect("launch at login should save");

        let autostart_path =
            user_launch_at_login_desktop_file_path().expect("user autostart path should resolve");
        let autostart_entry = std::fs::read_to_string(&autostart_path)
            .expect("user autostart desktop entry should exist");
        let settings = AppSettings::load().expect("settings should load");

        assert!(settings.launch_at_login);
        assert!(autostart_entry.contains("Exec=pepper-x"));
        assert!(autostart_entry.contains("X-GNOME-Autostart-enabled=true"));

        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        set_or_remove_env_var("XDG_CONFIG_HOME", previous_xdg_config_home);
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn settings_launch_at_login_removes_user_autostart_entry_when_disabled() {
        let _guard = env_lock().lock().unwrap();
        let state_root = temp_state_root();
        let config_root = state_root.join("config");
        std::fs::create_dir_all(&state_root).unwrap();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        let previous_xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        std::env::set_var("XDG_CONFIG_HOME", &config_root);

        save_launch_at_login(true).expect("launch at login should save");
        save_launch_at_login(false).expect("launch at login should disable");

        let autostart_path =
            user_launch_at_login_desktop_file_path().expect("user autostart path should resolve");
        let settings = AppSettings::load().expect("settings should load");

        assert!(!settings.launch_at_login);
        assert!(!autostart_path.exists());

        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        set_or_remove_env_var("XDG_CONFIG_HOME", previous_xdg_config_home);
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn settings_load_launch_at_login_from_user_autostart_entry() {
        let _guard = env_lock().lock().unwrap();
        let state_root = temp_state_root();
        let config_root = state_root.join("config");
        std::fs::create_dir_all(&state_root).unwrap();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        let previous_xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        std::env::set_var("XDG_CONFIG_HOME", &config_root);

        AppSettings {
            launch_at_login: false,
            ..AppSettings::default()
        }
        .save()
        .expect("settings should save");

        let autostart_path =
            user_launch_at_login_desktop_file_path().expect("user autostart path should resolve");
        std::fs::create_dir_all(autostart_path.parent().expect("autostart parent")).unwrap();
        std::fs::write(&autostart_path, launch_at_login_desktop_contents(true)).unwrap();

        let settings = AppSettings::load().expect("settings should load");

        assert!(settings.launch_at_login);

        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        set_or_remove_env_var("XDG_CONFIG_HOME", previous_xdg_config_home);
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn settings_load_launch_at_login_respects_disabled_user_autostart_entry() {
        let _guard = env_lock().lock().unwrap();
        let state_root = temp_state_root();
        let config_root = state_root.join("config");
        std::fs::create_dir_all(&state_root).unwrap();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        let previous_xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        std::env::set_var("XDG_CONFIG_HOME", &config_root);

        AppSettings {
            launch_at_login: true,
            ..AppSettings::default()
        }
        .save()
        .expect("settings should save");

        let autostart_path =
            user_launch_at_login_desktop_file_path().expect("user autostart path should resolve");
        std::fs::create_dir_all(autostart_path.parent().expect("autostart parent")).unwrap();
        std::fs::write(&autostart_path, launch_at_login_desktop_contents(false)).unwrap();

        let settings = AppSettings::load().expect("settings should load");

        assert!(!settings.launch_at_login);

        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        set_or_remove_env_var("XDG_CONFIG_HOME", previous_xdg_config_home);
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn settings_load_launch_at_login_from_user_autostart_without_settings_file() {
        let _guard = env_lock().lock().unwrap();
        let state_root = temp_state_root();
        let config_root = state_root.join("config");
        std::fs::create_dir_all(&state_root).unwrap();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        let previous_xdg_config_home = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        std::env::set_var("XDG_CONFIG_HOME", &config_root);

        let autostart_path =
            user_launch_at_login_desktop_file_path().expect("user autostart path should resolve");
        std::fs::create_dir_all(autostart_path.parent().expect("autostart parent")).unwrap();
        std::fs::write(&autostart_path, launch_at_login_desktop_contents(true)).unwrap();

        let settings = AppSettings::load().expect("settings should load");

        assert!(settings.launch_at_login);

        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        set_or_remove_env_var("XDG_CONFIG_HOME", previous_xdg_config_home);
        let _ = std::fs::remove_dir_all(state_root);
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
