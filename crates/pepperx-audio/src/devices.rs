use std::fmt;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
use cpal::traits::DeviceTrait;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEnumerationError {
    message: String,
}

impl DeviceEnumerationError {
    #[cfg(target_os = "linux")]
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
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
        enumerate_linux_microphones()
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(MicrophoneInventory::default())
    }
}

#[cfg(target_os = "linux")]
fn enumerate_linux_microphones() -> Result<MicrophoneInventory, DeviceEnumerationError> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|error| {
        DeviceEnumerationError::new(format!("failed to enumerate microphones: {error}"))
    })?;

    let microphones = devices
        .enumerate()
        .map(|(index, device)| microphone_from_device(host.id().name(), index, device))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(MicrophoneInventory::from_devices(microphones))
}

#[cfg(target_os = "linux")]
fn microphone_from_device(
    host_name: &str,
    index: usize,
    device: cpal::Device,
) -> Result<MicrophoneDevice, DeviceEnumerationError> {
    let display_name = device.name().map_err(|error| {
        DeviceEnumerationError::new(format!("failed to read microphone name: {error}"))
    })?;

    Ok(MicrophoneDevice::new(
        format!("{host_name}:{}:{index}", slugify(&display_name)),
        display_name,
    ))
}

#[cfg(target_os = "linux")]
fn slugify(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }

    slug.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
