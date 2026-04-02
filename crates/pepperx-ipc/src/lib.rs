use pepperx_session::TriggerSource;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

pub const SERVICE_NAME: &str = "com.obra.PepperX.Service";
pub const OBJECT_PATH: &str = "/com/obra/PepperX";
pub const INTERFACE_NAME: &str = "com.obra.PepperX";

pub const METHOD_PING: &str = "Ping";
pub const METHOD_START_RECORDING: &str = "StartRecording";
pub const METHOD_STOP_RECORDING: &str = "StopRecording";
pub const METHOD_SHOW_SETTINGS: &str = "ShowSettings";
pub const METHOD_SHOW_HISTORY: &str = "ShowHistory";
pub const METHOD_GET_CAPABILITIES: &str = "GetCapabilities";
pub const METHOD_GET_LIVE_STATUS: &str = "GetLiveStatus";

pub const SUPPORTED_METHODS: [&str; 7] = [
    METHOD_PING,
    METHOD_START_RECORDING,
    METHOD_STOP_RECORDING,
    METHOD_SHOW_SETTINGS,
    METHOD_SHOW_HISTORY,
    METHOD_GET_CAPABILITIES,
    METHOD_GET_LIVE_STATUS,
];

pub const TRIGGER_SOURCE_MODIFIER_ONLY: &str = "modifier-only";
pub const TRIGGER_SOURCE_STANDARD_SHORTCUT: &str = "standard-shortcut";
pub const TRIGGER_SOURCE_SHELL_ACTION: &str = "shell-action";

pub type CapabilityPayload = (bool, bool, String);
pub type LiveStatusPayload = (String, String);

pub const LIVE_STATUS_READY: &str = "ready";
pub const LIVE_STATUS_RECORDING: &str = "recording";
pub const LIVE_STATUS_TRANSCRIBING: &str = "transcribing";
pub const LIVE_STATUS_CLEANING_UP: &str = "cleaning-up";
pub const LIVE_STATUS_CLIPBOARD_FALLBACK: &str = "clipboard-fallback";
pub const LIVE_STATUS_ERROR: &str = "error";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub modifier_only_supported: bool,
    pub extension_connected: bool,
    pub version: String,
}

impl Capabilities {
    pub fn shell_default(version: impl Into<String>) -> Self {
        Self {
            modifier_only_supported: false,
            extension_connected: false,
            version: version.into(),
        }
    }

    pub fn to_dbus_payload(&self) -> CapabilityPayload {
        (
            self.modifier_only_supported,
            self.extension_connected,
            self.version.clone(),
        )
    }

    pub fn from_dbus_payload(payload: CapabilityPayload) -> Self {
        let (modifier_only_supported, extension_connected, version) = payload;

        Self {
            modifier_only_supported,
            extension_connected,
            version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LiveStatus {
    Ready,
    Recording,
    Transcribing,
    CleaningUp,
    ClipboardFallback(String),
    Error(String),
}

impl LiveStatus {
    pub fn ready() -> Self {
        Self::Ready
    }

    pub fn recording() -> Self {
        Self::Recording
    }

    pub fn transcribing() -> Self {
        Self::Transcribing
    }

    pub fn cleaning_up() -> Self {
        Self::CleaningUp
    }

    pub fn clipboard_fallback(message: impl Into<String>) -> Self {
        Self::ClipboardFallback(message.into())
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::Error(message.into())
    }

    /// Returns `true` when the pipeline is actively processing (recording,
    /// transcribing, or cleaning up).
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Recording | Self::Transcribing | Self::CleaningUp
        )
    }

    pub fn to_dbus_payload(&self) -> LiveStatusPayload {
        match self {
            Self::Ready => (LIVE_STATUS_READY.into(), String::new()),
            Self::Recording => (LIVE_STATUS_RECORDING.into(), String::new()),
            Self::Transcribing => (LIVE_STATUS_TRANSCRIBING.into(), String::new()),
            Self::CleaningUp => (LIVE_STATUS_CLEANING_UP.into(), String::new()),
            Self::ClipboardFallback(message) => {
                (LIVE_STATUS_CLIPBOARD_FALLBACK.into(), message.clone())
            }
            Self::Error(message) => (LIVE_STATUS_ERROR.into(), message.clone()),
        }
    }

    pub fn from_dbus_payload(payload: LiveStatusPayload) -> Self {
        let (state, detail) = payload;
        match state.as_str() {
            LIVE_STATUS_RECORDING => Self::Recording,
            LIVE_STATUS_TRANSCRIBING => Self::Transcribing,
            LIVE_STATUS_CLEANING_UP => Self::CleaningUp,
            LIVE_STATUS_CLIPBOARD_FALLBACK => Self::ClipboardFallback(detail),
            LIVE_STATUS_ERROR => Self::Error(detail),
            _ => Self::Ready,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SharedLiveStatus {
    current: Arc<Mutex<LiveStatus>>,
}

impl Default for SharedLiveStatus {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedLiveStatus {
    pub fn new() -> Self {
        Self {
            current: Arc::new(Mutex::new(LiveStatus::ready())),
        }
    }

    pub fn replace(&self, next: LiveStatus) {
        *self.current.lock().expect("live status lock poisoned") = next;
    }

    pub fn snapshot(&self) -> LiveStatus {
        self.current
            .lock()
            .expect("live status lock poisoned")
            .clone()
    }
}

pub fn trigger_source_name(source: TriggerSource) -> &'static str {
    match source {
        TriggerSource::ModifierOnly => TRIGGER_SOURCE_MODIFIER_ONLY,
        TriggerSource::StandardShortcut => TRIGGER_SOURCE_STANDARD_SHORTCUT,
        TriggerSource::ShellAction => TRIGGER_SOURCE_SHELL_ACTION,
    }
}

pub fn parse_trigger_source(value: &str) -> Result<TriggerSource, String> {
    match value {
        TRIGGER_SOURCE_MODIFIER_ONLY => Ok(TriggerSource::ModifierOnly),
        TRIGGER_SOURCE_STANDARD_SHORTCUT => Ok(TriggerSource::StandardShortcut),
        TRIGGER_SOURCE_SHELL_ACTION => Ok(TriggerSource::ShellAction),
        _ => Err(format!("unsupported trigger source: {value}")),
    }
}

#[cfg(test)]
mod ipc_contract {
    use super::*;

    #[test]
    fn ipc_contract_supports_expected_methods() {
        assert_eq!(
            SUPPORTED_METHODS,
            [
                "Ping",
                "StartRecording",
                "StopRecording",
                "ShowSettings",
                "ShowHistory",
                "GetCapabilities",
                "GetLiveStatus",
            ]
        );
    }

    #[test]
    fn ipc_contract_roundtrips_capability_payload() {
        let capabilities = Capabilities {
            modifier_only_supported: true,
            extension_connected: false,
            version: "0.1.0".to_string(),
        };

        let round_trip = Capabilities::from_dbus_payload(capabilities.to_dbus_payload());

        assert_eq!(round_trip, capabilities);
    }

    #[test]
    fn ipc_contract_roundtrips_trigger_sources() {
        let source = TriggerSource::ModifierOnly;

        assert_eq!(
            parse_trigger_source(trigger_source_name(source)).unwrap(),
            source
        );
    }

    #[test]
    fn ipc_contract_roundtrips_live_status_payload() {
        let status = LiveStatus::clipboard_fallback("Copied to clipboard. Press Ctrl+V to paste.");

        let round_trip = LiveStatus::from_dbus_payload(status.to_dbus_payload());

        assert_eq!(round_trip, status);
    }

    #[test]
    fn ipc_contract_shared_live_status_tracks_latest_value() {
        let status = SharedLiveStatus::new();

        status.replace(LiveStatus::transcribing());

        assert_eq!(status.snapshot(), LiveStatus::transcribing());
    }
}
