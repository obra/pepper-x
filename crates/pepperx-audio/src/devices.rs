use std::fmt;

use serde::{Deserialize, Serialize};

#[cfg(test)]
use serde_json::{Map, Value};

#[cfg(target_os = "linux")]
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use pipewire as pw;

#[cfg(any(test, target_os = "linux"))]
trait PipeWirePropertySource {
    fn property_string(&self, key: &str) -> Option<String>;
}

#[cfg(test)]
impl PipeWirePropertySource for Map<String, Value> {
    fn property_string(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Some(Value::String(value)) if !value.is_empty() => Some(value.clone()),
            Some(Value::Bool(value)) => Some(value.to_string()),
            Some(Value::Number(value)) => Some(value.to_string()),
            _ => None,
        }
    }
}

#[cfg(target_os = "linux")]
impl PipeWirePropertySource for pw::spa::utils::dict::DictRef {
    fn property_string(&self, key: &str) -> Option<String> {
        self.get(key)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MicrophoneDevice {
    stable_id: String,
    display_name: String,
}

impl MicrophoneDevice {
    pub fn new(stable_id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            stable_id: stable_id.into(),
            display_name: display_name.into(),
        }
    }

    pub fn stable_id(&self) -> &str {
        &self.stable_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedMicrophone {
    stable_id: String,
    display_name: String,
}

impl SelectedMicrophone {
    pub fn new(stable_id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            stable_id: stable_id.into(),
            display_name: display_name.into(),
        }
    }

    pub fn stable_id(&self) -> &str {
        &self.stable_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

impl From<&MicrophoneDevice> for SelectedMicrophone {
    fn from(device: &MicrophoneDevice) -> Self {
        Self::new(device.stable_id(), device.display_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MicrophoneInventory {
    devices: Vec<MicrophoneDevice>,
}

impl MicrophoneInventory {
    pub fn from_devices(devices: Vec<MicrophoneDevice>) -> Self {
        Self { devices }
    }

    pub fn devices(&self) -> &[MicrophoneDevice] {
        &self.devices
    }

    pub fn resolve_selected(
        &self,
        preferred: Option<&SelectedMicrophone>,
    ) -> Option<SelectedMicrophone> {
        if let Some(preferred) = preferred {
            if let Some(device) = self
                .devices
                .iter()
                .find(|device| device.stable_id() == preferred.stable_id())
            {
                return Some(SelectedMicrophone::from(device));
            }
        }

        self.devices.first().map(SelectedMicrophone::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEnumerationError {
    message: String,
}

impl DeviceEnumerationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn unsupported_platform() -> Self {
        Self::new("microphone enumeration is only supported on linux")
    }
}

impl fmt::Display for DeviceEnumerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DeviceEnumerationError {}

pub fn enumerate_microphones() -> Result<MicrophoneInventory, DeviceEnumerationError> {
    #[cfg(target_os = "linux")]
    {
        return enumerate_linux_microphones();
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(DeviceEnumerationError::unsupported_platform())
    }
}

#[cfg(target_os = "linux")]
fn enumerate_linux_microphones() -> Result<MicrophoneInventory, DeviceEnumerationError> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|error| {
        DeviceEnumerationError::new(format!("failed to create PipeWire main loop: {error}"))
    })?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|error| {
        DeviceEnumerationError::new(format!("failed to create PipeWire context: {error}"))
    })?;
    let core = context.connect_rc(None).map_err(|error| {
        DeviceEnumerationError::new(format!("failed to connect to PipeWire: {error}"))
    })?;
    let registry = core.get_registry().map_err(|error| {
        DeviceEnumerationError::new(format!("failed to get PipeWire registry: {error}"))
    })?;

    let microphones = Rc::new(RefCell::new(Vec::new()));
    let enumeration_error = Rc::new(RefCell::new(None::<DeviceEnumerationError>));
    let done = Rc::new(Cell::new(false));

    let registry_microphones = Rc::clone(&microphones);
    let registry_error = Rc::clone(&enumeration_error);
    let registry_loop = mainloop.clone();
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            if global.type_.to_str() != "PipeWire:Interface:Node" {
                return;
            }

            let Some(properties) = global.props else {
                return;
            };

            match microphone_from_pipewire_dict(properties) {
                Ok(Some(device)) => registry_microphones.borrow_mut().push(device),
                Ok(None) => {}
                Err(error) => {
                    if registry_error.borrow().is_none() {
                        *registry_error.borrow_mut() = Some(error);
                    }
                    registry_loop.quit();
                }
            }
        })
        .register();

    let pending = core.sync(0).map_err(|error| {
        DeviceEnumerationError::new(format!("failed to sync PipeWire: {error}"))
    })?;

    let done_flag = Rc::clone(&done);
    let core_loop = mainloop.clone();
    let _core_listener = core
        .add_listener_local()
        .done(move |id, sequence| {
            if id == pw::core::PW_ID_CORE && sequence == pending {
                done_flag.set(true);
                core_loop.quit();
            }
        })
        .register();

    let enumeration_deadline = Instant::now() + Duration::from_secs(5);
    while !done.get() && enumeration_error.borrow().is_none() {
        if Instant::now() >= enumeration_deadline {
            return Err(DeviceEnumerationError::new(
                "PipeWire microphone enumeration timed out",
            ));
        }
        mainloop.loop_().iterate(Duration::from_millis(100));
    }

    if let Some(error) = enumeration_error.borrow_mut().take() {
        return Err(error);
    }

    let mut microphones = microphones.borrow().clone();
    microphones.sort_by(|left, right| {
        left.display_name()
            .cmp(right.display_name())
            .then_with(|| left.stable_id().cmp(right.stable_id()))
    });
    microphones.dedup_by(|left, right| left.stable_id() == right.stable_id());

    Ok(MicrophoneInventory::from_devices(microphones))
}

#[cfg(test)]
fn inventory_from_pipewire_objects(
    objects: &[Value],
) -> Result<MicrophoneInventory, DeviceEnumerationError> {
    let mut microphones = Vec::new();

    for object in objects {
        if let Some(device) = microphone_from_pipewire_object(object)? {
            microphones.push(device);
        }
    }

    Ok(MicrophoneInventory::from_devices(microphones))
}

#[cfg(test)]
fn microphone_from_pipewire_object(
    object: &Value,
) -> Result<Option<MicrophoneDevice>, DeviceEnumerationError> {
    if object.get("type").and_then(Value::as_str) != Some("PipeWire:Interface:Node") {
        return Ok(None);
    }

    let Some(properties) = object
        .get("info")
        .and_then(|info| info.get("props"))
        .and_then(Value::as_object)
    else {
        return Ok(None);
    };

    microphone_from_pipewire_properties(properties)
}

#[cfg(test)]
fn microphone_from_pipewire_properties(
    properties: &Map<String, Value>,
) -> Result<Option<MicrophoneDevice>, DeviceEnumerationError> {
    microphone_from_pipewire_source(properties)
}

#[cfg(target_os = "linux")]
fn microphone_from_pipewire_dict(
    properties: &pw::spa::utils::dict::DictRef,
) -> Result<Option<MicrophoneDevice>, DeviceEnumerationError> {
    microphone_from_pipewire_source(properties)
}

#[cfg(target_os = "linux")]
pub(crate) fn stable_pipewire_microphone_id(
    properties: &pw::spa::utils::dict::DictRef,
) -> Result<Option<String>, DeviceEnumerationError> {
    microphone_from_pipewire_dict(properties)
        .map(|device| device.map(|device| device.stable_id().to_string()))
}

#[cfg(any(test, target_os = "linux"))]
fn microphone_from_pipewire_source(
    properties: &impl PipeWirePropertySource,
) -> Result<Option<MicrophoneDevice>, DeviceEnumerationError> {
    if properties.property_string("media.class").as_deref() != Some("Audio/Source") {
        return Ok(None);
    }

    if properties.property_string("node.virtual").as_deref() == Some("true") {
        return Ok(None);
    }

    let node_name = properties.property_string("node.name");

    if node_name
        .as_deref()
        .is_some_and(|name| name.contains(".monitor"))
    {
        return Ok(None);
    }

    let stable_id = stable_pipewire_id(properties, node_name.as_deref())?;
    let display_name = properties
        .property_string("node.description")
        .or_else(|| properties.property_string("node.nick"))
        .or(node_name)
        .ok_or_else(|| {
            DeviceEnumerationError::new(
                "failed to read microphone display name from PipeWire properties",
            )
        })?;

    Ok(Some(MicrophoneDevice::new(stable_id, display_name)))
}

#[cfg(any(test, target_os = "linux"))]
fn stable_pipewire_id(
    properties: &impl PipeWirePropertySource,
    node_name: Option<&str>,
) -> Result<String, DeviceEnumerationError> {
    if let Some(node_name) = node_name {
        return Ok(format!("pipewire:node.name={node_name}"));
    }

    if let Some(object_path) = properties.property_string("object.path") {
        return Ok(format!("pipewire:object.path={object_path}"));
    }

    Err(DeviceEnumerationError::new(
        "failed to derive a stable microphone identifier from PipeWire properties",
    ))
}

#[cfg(test)]
fn microphone_from_pipewire_node(
    properties: &[(&str, &str)],
) -> Result<Option<MicrophoneDevice>, DeviceEnumerationError> {
    microphone_from_pipewire_properties(&test_pipewire_properties(properties))
}

#[cfg(test)]
fn inventory_from_pipewire_test_nodes(
    nodes: &[&[(&str, &str)]],
) -> Result<MicrophoneInventory, DeviceEnumerationError> {
    let objects = nodes
        .iter()
        .map(|properties| {
            let mut object = Map::new();
            let mut info = Map::new();

            object.insert(
                "type".into(),
                Value::String("PipeWire:Interface:Node".into()),
            );
            info.insert(
                "props".into(),
                Value::Object(test_pipewire_properties(properties)),
            );
            object.insert("info".into(), Value::Object(info));

            Value::Object(object)
        })
        .collect::<Vec<_>>();

    inventory_from_pipewire_objects(&objects)
}

#[cfg(test)]
fn test_pipewire_properties(properties: &[(&str, &str)]) -> Map<String, Value> {
    properties
        .iter()
        .map(|(key, value)| ((*key).to_string(), Value::String((*value).to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::SignalLevelSample;

    #[test]
    fn device_discovered_microphones_keep_stable_ids_and_display_names() {
        let device = MicrophoneDevice::new("alsa-input-usb-blue-yeti", "Blue Yeti");

        assert_eq!(device.stable_id(), "alsa-input-usb-blue-yeti");
        assert_eq!(device.display_name(), "Blue Yeti");
    }

    #[test]
    fn device_selected_microphone_carries_device_metadata() {
        let selected = SelectedMicrophone::new("alsa-input-pci-hda-intel", "Built-in Audio");

        assert_eq!(selected.stable_id(), "alsa-input-pci-hda-intel");
        assert_eq!(selected.display_name(), "Built-in Audio");
    }

    #[test]
    fn device_enumerator_returns_an_empty_but_valid_list_when_no_microphones_exist() {
        let inventory = MicrophoneInventory::from_devices(Vec::new());

        assert!(inventory.devices().is_empty());
    }

    #[test]
    fn device_inventory_resolves_saved_microphone_when_present() {
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

        let selected = inventory
            .resolve_selected(Some(&SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )))
            .expect("saved microphone should resolve");

        assert_eq!(
            selected,
            SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )
        );
    }

    #[test]
    fn device_inventory_falls_back_to_first_microphone_when_saved_device_is_missing() {
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

        let selected = inventory
            .resolve_selected(Some(&SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.missing-device",
                "Missing Device",
            )))
            .expect("inventory should fall back to a discovered microphone");

        assert_eq!(
            selected,
            SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )
        );
    }

    #[test]
    fn device_level_sample_reports_peak_signal_strength_from_pcm() {
        let sample = SignalLevelSample::from_pcm_samples(&[0, 21_299, -8_192]);

        assert!(sample.signal_present());
        assert!(sample.normalized_level() > 0.64);
    }

    #[test]
    fn selected_microphone_serializes_with_only_stable_id_and_display_name() {
        let microphone = SelectedMicrophone::new(
            "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
            "Blue Yeti",
        );

        let json = serde_json::to_value(&microphone).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "stable_id": "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "display_name": "Blue Yeti"
            })
        );
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn device_enumerator_reports_unsupported_platform_instead_of_empty_inventory() {
        let error = enumerate_microphones().unwrap_err();

        assert_eq!(
            error.to_string(),
            "microphone enumeration is only supported on linux"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn device_enumerator_does_not_require_pw_dump_binary() {
        const CHILD_ENV: &str = "PEPPERX_AUDIO_TEST_WITHOUT_PW_DUMP";
        const TEST_NAME: &str = "devices::tests::device_enumerator_does_not_require_pw_dump_binary";

        if std::env::var_os(CHILD_ENV).is_some() {
            if let Err(error) = enumerate_microphones() {
                assert!(
                    !error.to_string().contains("pw-dump"),
                    "expected PipeWire enumeration without pw-dump shelling, got: {error}"
                );
            }
            return;
        }

        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg(TEST_NAME)
            .env(CHILD_ENV, "1")
            .env("PATH", "/definitely-missing-pepperx-tools")
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "child test failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn pipewire_node_ids_do_not_depend_on_enumeration_order() {
        let first = microphone_from_pipewire_node(&[
            ("node.name", "alsa_input.usb-blue-yeti-00.analog-stereo"),
            ("node.description", "Blue Yeti"),
            ("media.class", "Audio/Source"),
        ])
        .unwrap()
        .unwrap();
        let second = microphone_from_pipewire_node(&[
            ("node.name", "alsa_input.usb-blue-yeti-00.analog-stereo"),
            ("node.description", "Blue Yeti"),
            ("media.class", "Audio/Source"),
        ])
        .unwrap()
        .unwrap();

        assert_eq!(
            first.stable_id(),
            "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo"
        );
        assert_eq!(second.stable_id(), first.stable_id());
    }

    #[test]
    fn pipewire_node_ids_prefer_node_name_for_target_selection() {
        let microphone = microphone_from_pipewire_node(&[
            (
                "object.path",
                "alsa:acp:Blue_Microphones_Yeti_Stereo_Microphone-00:analog-input-mic",
            ),
            ("node.name", "alsa_input.usb-blue-yeti-00.analog-stereo"),
            ("node.description", "Blue Yeti"),
            ("media.class", "Audio/Source"),
        ])
        .unwrap()
        .unwrap();

        assert_eq!(
            microphone.stable_id(),
            "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo"
        );
    }

    #[test]
    fn pipewire_inventory_filters_out_non_microphone_nodes() {
        let inventory = inventory_from_pipewire_test_nodes(&[
            &[
                ("node.name", "alsa_input.usb-blue-yeti-00.analog-stereo"),
                ("node.description", "Blue Yeti"),
                ("media.class", "Audio/Source"),
            ],
            &[
                (
                    "node.name",
                    "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
                ),
                ("node.description", "Monitor of Built-in Audio"),
                ("media.class", "Audio/Sink"),
            ],
            &[
                ("node.name", "loopback.capture"),
                ("node.description", "Loopback"),
                ("media.class", "Stream/Input/Audio"),
            ],
        ])
        .unwrap();

        assert_eq!(
            inventory.devices(),
            &[MicrophoneDevice::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )]
        );
    }
}
