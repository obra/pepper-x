use pepperx_session::TriggerSource;
use serde::{Deserialize, Serialize};

pub const SERVICE_NAME: &str = "com.obra.PepperX.Service";
pub const OBJECT_PATH: &str = "/com/obra/PepperX";
pub const INTERFACE_NAME: &str = "com.obra.PepperX";

pub const METHOD_PING: &str = "Ping";
pub const METHOD_START_RECORDING: &str = "StartRecording";
pub const METHOD_STOP_RECORDING: &str = "StopRecording";
pub const METHOD_SHOW_SETTINGS: &str = "ShowSettings";
pub const METHOD_SHOW_HISTORY: &str = "ShowHistory";
pub const METHOD_GET_CAPABILITIES: &str = "GetCapabilities";

pub const SUPPORTED_METHODS: [&str; 6] = [
    METHOD_PING,
    METHOD_START_RECORDING,
    METHOD_STOP_RECORDING,
    METHOD_SHOW_SETTINGS,
    METHOD_SHOW_HISTORY,
    METHOD_GET_CAPABILITIES,
];

pub const TRIGGER_SOURCE_MODIFIER_ONLY: &str = "modifier-only";
pub const TRIGGER_SOURCE_STANDARD_SHORTCUT: &str = "standard-shortcut";
pub const TRIGGER_SOURCE_SHELL_ACTION: &str = "shell-action";

pub type CapabilityPayload = (bool, bool, String);

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
}
