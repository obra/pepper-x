use crate::service::PepperXService;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::mem;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

// Linux input event constants
const EV_KEY: u16 = 0x01;

// Modifier keycodes
const KEY_LEFTCTRL: u16 = 29;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_RIGHTSHIFT: u16 = 54;
const KEY_LEFTALT: u16 = 56;
const KEY_RIGHTALT: u16 = 100;
const KEY_LEFTMETA: u16 = 125;
const KEY_RIGHTMETA: u16 = 126;
const KEY_SPACE: u16 = 57;
const KEY_BACKSPACE: u16 = 14;
const KEY_TAB: u16 = 15;
const KEY_ENTER: u16 = 28;
const KEY_CAPSLOCK: u16 = 58;
const KEY_DELETE: u16 = 111;
const KEY_KATAKANAHIRAGANA: u16 = 93;
const KEY_HENKAN: u16 = 92;
const KEY_MUHENKAN: u16 = 94;

/// Paired modifier keys: each entry is (left, right). When a user presses
/// one side of a modifier, the trigger system should accept either side.
const MODIFIER_PAIRS: [(u16, u16); 4] = [
    (KEY_LEFTCTRL, KEY_RIGHTCTRL),
    (KEY_LEFTSHIFT, KEY_RIGHTSHIFT),
    (KEY_LEFTALT, KEY_RIGHTALT),
    (KEY_LEFTMETA, KEY_RIGHTMETA),
];

// ioctl constants for EVIOCGBIT: _IOR('E', 0x20+ev, len)
// _IOR(type, nr, size) = (2 << 30) | (type << 8) | nr | (size << 16)
const EVIOCGBIT_EV: libc::c_ulong = 0x80044520; // EVIOCGBIT(0, 4)
const EVIOCGBIT_KEY: libc::c_ulong = 0x80604521; // EVIOCGBIT(EV_KEY, 96)

/// Size of a raw `input_event` struct on this platform.
const INPUT_EVENT_SIZE: usize = mem::size_of::<InputEvent>();

/// Raw Linux `input_event` — matches the kernel's `struct input_event`.
#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    _tv_sec: libc::time_t,
    _tv_usec: libc::suseconds_t,
    type_: u16,
    code: u16,
    value: i32,
}

// ---------------------------------------------------------------------------
// Trigger configuration
// ---------------------------------------------------------------------------

/// Describes which modifier keys must be held simultaneously to trigger recording.
///
/// For a two-group combo like Alt+Super, `groups` contains two entries. Each
/// group lists the keycodes that satisfy it (e.g. left-alt OR right-alt).
/// All groups must have at least one key held to activate the trigger.
///
/// For a single-modifier trigger like "ctrl", there is one group with
/// `[KEY_LEFTCTRL, KEY_RIGHTCTRL]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerConfig {
    /// Each inner `Vec` is a group of keycodes — any one keycode in the group
    /// satisfies it. All groups must be satisfied simultaneously.
    pub groups: Vec<Vec<u16>>,
}

impl TriggerConfig {
    /// Parse a settings string into a config.
    ///
    /// Supports three formats:
    /// 1. Named presets (backward compat): "alt+super", "ctrl", "right_alt"
    /// 2. Comma-separated evdev keycodes: "56,100,125,126"
    ///
    /// Unrecognised strings fall back to alt+super.
    pub fn from_setting(setting: &str) -> Self {
        // Try named presets first for backward compatibility
        match setting {
            "ctrl" => {
                return Self {
                    groups: vec![vec![KEY_LEFTCTRL, KEY_RIGHTCTRL]],
                };
            }
            "right_alt" => {
                return Self {
                    groups: vec![vec![KEY_RIGHTALT]],
                };
            }
            "alt+super" => {
                return Self {
                    groups: vec![
                        vec![KEY_LEFTALT, KEY_RIGHTALT],
                        vec![KEY_LEFTMETA, KEY_RIGHTMETA],
                    ],
                };
            }
            _ => {}
        }

        // Try parsing as comma-separated keycodes
        if setting.contains(',') || setting.chars().all(|c| c.is_ascii_digit()) {
            let keycodes: Vec<u16> = setting
                .split(',')
                .filter_map(|s| s.trim().parse::<u16>().ok())
                .collect();
            if !keycodes.is_empty() {
                return Self::from_keycodes(&keycodes);
            }
        }

        // Fallback: alt+super
        Self {
            groups: vec![
                vec![KEY_LEFTALT, KEY_RIGHTALT],
                vec![KEY_LEFTMETA, KEY_RIGHTMETA],
            ],
        }
    }

    /// Build a `TriggerConfig` from a flat list of evdev keycodes.
    ///
    /// Modifier keys are grouped by type (e.g. left-alt and right-alt form
    /// one group). Non-modifier keys each get their own single-key group.
    pub fn from_keycodes(keycodes: &[u16]) -> Self {
        let mut groups: Vec<Vec<u16>> = Vec::new();
        let mut used = vec![false; keycodes.len()];

        for (i, &kc) in keycodes.iter().enumerate() {
            if used[i] {
                continue;
            }
            if let Some((left, right)) = modifier_pair_for(kc) {
                // Mark any other keycodes from the same pair as used
                for (j, &other_kc) in keycodes.iter().enumerate() {
                    if j != i && (other_kc == left || other_kc == right) {
                        used[j] = true;
                    }
                }
                used[i] = true;
                groups.push(vec![left, right]);
            } else {
                used[i] = true;
                groups.push(vec![kc]);
            }
        }

        Self { groups }
    }

    /// Serialize this config to a comma-separated keycode string suitable for
    /// storing in the settings file.
    ///
    /// The output lists every keycode from every group, deduplicated, sorted.
    pub fn to_setting(&self) -> String {
        let mut all: Vec<u16> = self.all_keycodes();
        all.sort_unstable();
        all.dedup();
        all.iter()
            .map(|kc| kc.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Return a flat list of every keycode that appears in any group.
    fn all_keycodes(&self) -> Vec<u16> {
        self.groups.iter().flat_map(|g| g.iter().copied()).collect()
    }

    /// Which group index (if any) does this keycode belong to?
    fn group_for_keycode(&self, keycode: u16) -> Option<usize> {
        self.groups
            .iter()
            .position(|group| group.contains(&keycode))
    }

    /// Check if a keycode is part of the trigger combo.
    fn is_trigger_key(&self, keycode: u16) -> bool {
        self.group_for_keycode(keycode).is_some()
    }

    /// Return a human-readable label for this trigger config (e.g. "Alt + Super").
    pub fn display_name(&self) -> String {
        let mut names: Vec<&str> = Vec::new();
        for group in &self.groups {
            if let Some(name) = group_display_name(group) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        if names.is_empty() {
            "No keys".into()
        } else {
            names.join(" + ")
        }
    }
}

/// Return the modifier pair (left, right) that `keycode` belongs to, if any.
fn modifier_pair_for(keycode: u16) -> Option<(u16, u16)> {
    MODIFIER_PAIRS
        .iter()
        .find(|(l, r)| keycode == *l || keycode == *r)
        .copied()
}

/// Human-readable name for a group of keycodes.
fn group_display_name(group: &[u16]) -> Option<&'static str> {
    // Check by first keycode in the group
    for &kc in group {
        match kc {
            KEY_LEFTCTRL | KEY_RIGHTCTRL => return Some("Ctrl"),
            KEY_LEFTSHIFT | KEY_RIGHTSHIFT => return Some("Shift"),
            KEY_LEFTALT | KEY_RIGHTALT => return Some("Alt"),
            KEY_LEFTMETA | KEY_RIGHTMETA => return Some("Super"),
            KEY_SPACE => return Some("Space"),
            KEY_BACKSPACE => return Some("Backspace"),
            KEY_TAB => return Some("Tab"),
            KEY_ENTER => return Some("Return"),
            KEY_CAPSLOCK => return Some("Caps Lock"),
            KEY_DELETE => return Some("Delete"),
            KEY_KATAKANAHIRAGANA => return Some("Kana"),
            KEY_HENKAN => return Some("Henkan"),
            KEY_MUHENKAN => return Some("Muhenkan"),
            _ => {}
        }
    }
    // For unknown keycodes, show the numeric value
    group.first().map(|_| "Key")
}

/// Convert a single evdev keycode to a human-readable name.
pub fn evdev_keycode_name(keycode: u16) -> &'static str {
    match keycode {
        KEY_LEFTCTRL => "Left Ctrl",
        KEY_RIGHTCTRL => "Right Ctrl",
        KEY_LEFTSHIFT => "Left Shift",
        KEY_RIGHTSHIFT => "Right Shift",
        KEY_LEFTALT => "Left Alt",
        KEY_RIGHTALT => "Right Alt",
        KEY_LEFTMETA => "Left Super",
        KEY_RIGHTMETA => "Right Super",
        KEY_SPACE => "Space",
        KEY_BACKSPACE => "Backspace",
        KEY_TAB => "Tab",
        KEY_ENTER => "Return",
        KEY_CAPSLOCK => "Caps Lock",
        KEY_DELETE => "Delete",
        KEY_KATAKANAHIRAGANA => "Kana",
        KEY_HENKAN => "Henkan",
        KEY_MUHENKAN => "Muhenkan",
        _ => "Unknown",
    }
}

/// Map a GDK keyval to an evdev keycode, if it is a recognised modifier or key.
///
/// This is used by the settings UI shortcut recorder to translate GTK key
/// events into evdev keycodes for storage.
pub fn gdk_keyval_to_evdev(keyval: u32) -> Option<u16> {
    match keyval {
        0xffe3 => Some(KEY_LEFTCTRL),          // GDK_KEY_Control_L
        0xffe4 => Some(KEY_RIGHTCTRL),         // GDK_KEY_Control_R
        0xffe1 => Some(KEY_LEFTSHIFT),         // GDK_KEY_Shift_L
        0xffe2 => Some(KEY_RIGHTSHIFT),        // GDK_KEY_Shift_R
        0xffe9 => Some(KEY_LEFTALT),           // GDK_KEY_Alt_L
        0xffea => Some(KEY_RIGHTALT),          // GDK_KEY_Alt_R
        0xffeb => Some(KEY_LEFTMETA),          // GDK_KEY_Super_L
        0xffec => Some(KEY_RIGHTMETA),         // GDK_KEY_Super_R
        0x20 => Some(KEY_SPACE),               // GDK_KEY_space
        0xff0d => Some(KEY_ENTER),             // GDK_KEY_Return
        0xff09 => Some(KEY_TAB),               // GDK_KEY_Tab
        0xff08 => Some(KEY_BACKSPACE),         // GDK_KEY_BackSpace
        0xffff => Some(KEY_DELETE),            // GDK_KEY_Delete
        0xffe5 => Some(KEY_CAPSLOCK),          // GDK_KEY_Caps_Lock
        0xff2d => Some(KEY_KATAKANAHIRAGANA),  // GDK_KEY_Kana_Lock
        0xff26 => Some(KEY_KATAKANAHIRAGANA),  // GDK_KEY_Katakana
        0xff25 => Some(KEY_KATAKANAHIRAGANA),  // GDK_KEY_Hiragana
        0xff23 => Some(KEY_HENKAN),            // GDK_KEY_Henkan_Mode
        0xff22 => Some(KEY_MUHENKAN),          // GDK_KEY_Muhenkan
        _ => None,
    }
}

/// Build a human-readable label from a setting string.
///
/// Handles both named presets and comma-separated keycodes.
pub fn trigger_keys_display_name(setting: &str) -> String {
    TriggerConfig::from_setting(setting).display_name()
}

/// Whether the trigger activates on hold-and-release or on toggle (tap to
/// start, tap again to stop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    /// Hold trigger keys to record, release to stop (default).
    Hold,
    /// Press trigger combo once to start, press again to stop.
    Toggle,
}

impl TriggerMode {
    /// Parse the `preferred_recording_trigger_mode` value from settings.
    /// The settings file uses kebab-case serde names: "modifier-only" maps to
    /// Hold, "standard-shortcut" to Toggle. We also accept the UI-facing
    /// values "hold" and "toggle" for convenience.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "toggle" | "standard-shortcut" => Self::Toggle,
            _ => Self::Hold,
        }
    }
}

// ---------------------------------------------------------------------------
// Modifier hold state machine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldSignal {
    Start,
    Stop,
}

/// Tracks which groups currently have a key held, and drives the start/stop
/// state machine for both Hold and Toggle modes.
#[derive(Debug, Clone)]
struct ModifierHoldState {
    /// Per-group: how many trigger keys are physically held.
    group_held: Vec<bool>,
    /// Recording is active (all groups held, no chord).
    active: bool,
    /// A non-modifier key was pressed during the hold — chord cancellation.
    chord_blocked: bool,
    /// In toggle mode: whether we are currently recording.
    recording_active: bool,
    config: TriggerConfig,
    mode: TriggerMode,
}

impl ModifierHoldState {
    fn new(config: TriggerConfig, mode: TriggerMode) -> Self {
        let group_count = config.groups.len();
        Self {
            group_held: vec![false; group_count],
            active: false,
            chord_blocked: false,
            recording_active: false,
            config,
            mode,
        }
    }

    fn all_groups_held(&self) -> bool {
        self.group_held.iter().all(|held| *held)
    }

    fn any_group_held(&self) -> bool {
        self.group_held.iter().any(|held| *held)
    }

    fn handle_key_event(&mut self, pressed: bool, keycode: u16) -> Option<HoldSignal> {
        let group_idx = self.config.group_for_keycode(keycode);

        if let Some(idx) = group_idx {
            if pressed {
                self.group_held[idx] = true;

                if self.all_groups_held() && !self.active && !self.chord_blocked {
                    match self.mode {
                        TriggerMode::Hold => {
                            self.active = true;
                            return Some(HoldSignal::Start);
                        }
                        TriggerMode::Toggle => {
                            // Only fire on the transition to all-held
                            self.active = true;
                        }
                    }
                }
                return None;
            }

            // Release
            self.group_held[idx] = false;

            match self.mode {
                TriggerMode::Hold => {
                    if self.active {
                        self.active = false;
                        self.chord_blocked = false;
                        return Some(HoldSignal::Stop);
                    }

                    if !self.any_group_held() {
                        self.chord_blocked = false;
                    }
                }
                TriggerMode::Toggle => {
                    // Fire toggle on release after all groups were held
                    if self.active && !self.chord_blocked {
                        self.active = false;
                        if self.recording_active {
                            self.recording_active = false;
                            return Some(HoldSignal::Stop);
                        } else {
                            self.recording_active = true;
                            return Some(HoldSignal::Start);
                        }
                    }
                    if !self.any_group_held() {
                        self.active = false;
                        self.chord_blocked = false;
                    }
                }
            }

            return None;
        }

        // Non-modifier key
        if !pressed {
            return None;
        }

        match self.mode {
            TriggerMode::Hold => {
                if self.active {
                    self.active = false;
                    self.chord_blocked = true;
                    return Some(HoldSignal::Stop);
                }

                if self.any_group_held() {
                    self.chord_blocked = true;
                }
            }
            TriggerMode::Toggle => {
                // In toggle mode, non-trigger keys during the hold just block
                // the pending toggle, but don't stop an already-active recording.
                if self.active && !self.recording_active {
                    self.chord_blocked = true;
                }
                if self.any_group_held() && !self.recording_active {
                    self.chord_blocked = true;
                }
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Evdev capture handle
// ---------------------------------------------------------------------------

/// Shared mutable config pair that both the UI and the capture thread can access.
pub type SharedTriggerConfig = Arc<Mutex<(TriggerConfig, TriggerConfig)>>;

#[derive(Debug)]
pub struct EvdevModifierCaptureHandle {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    shared_config: SharedTriggerConfig,
}

impl EvdevModifierCaptureHandle {
    pub fn start(
        service: PepperXService,
        hold_config: TriggerConfig,
        toggle_config: TriggerConfig,
    ) -> Result<Self, EvdevModifierCaptureError> {
        let devices = find_keyboard_devices()?;
        if devices.is_empty() {
            return Err(EvdevModifierCaptureError::NoKeyboardFound);
        }

        let shared_config: SharedTriggerConfig =
            Arc::new(Mutex::new((hold_config.clone(), toggle_config.clone())));
        let loop_config = shared_config.clone();

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = stop.clone();

        let thread = thread::Builder::new()
            .name("evdev-modifier-capture".into())
            .spawn(move || {
                if let Err(e) = run_capture_loop(
                    &devices,
                    &stop_clone,
                    &service,
                    hold_config,
                    toggle_config,
                    &loop_config,
                ) {
                    eprintln!("[Pepper X] evdev capture loop exited: {e}");
                }
            })
            .map_err(EvdevModifierCaptureError::SpawnFailed)?;

        Ok(Self {
            stop,
            thread: Some(thread),
            shared_config,
        })
    }

    /// Replace the trigger configs at runtime. The capture loop will pick up
    /// the new values on its next epoll wake cycle.
    pub fn update_config(&self, hold_config: TriggerConfig, toggle_config: TriggerConfig) {
        if let Ok(mut guard) = self.shared_config.lock() {
            *guard = (hold_config, toggle_config);
        }
    }

    /// Return a clone of the shared config so callers (e.g. the settings UI)
    /// can push config changes directly.
    pub fn shared_config(&self) -> &SharedTriggerConfig {
        &self.shared_config
    }
}

impl Drop for EvdevModifierCaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Debug)]
pub enum EvdevModifierCaptureError {
    NoKeyboardFound,
    SpawnFailed(io::Error),
}

impl fmt::Display for EvdevModifierCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoKeyboardFound => {
                f.write_str("no keyboard input devices found (is the user in the 'input' group?)")
            }
            Self::SpawnFailed(e) => write!(f, "failed to spawn evdev capture thread: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Device discovery
// ---------------------------------------------------------------------------

/// Find `/dev/input/event*` devices that have keyboard capability.
fn find_keyboard_devices() -> Result<Vec<PathBuf>, EvdevModifierCaptureError> {
    let mut keyboards = Vec::new();

    let entries =
        fs::read_dir("/dev/input").map_err(|_| EvdevModifierCaptureError::NoKeyboardFound)?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.starts_with("event") {
            continue;
        }

        if let Ok(file) = File::open(&path) {
            if is_keyboard_device(&file) {
                keyboards.push(path);
            }
        }
    }

    Ok(keyboards)
}

/// Check if a device supports EV_KEY events with modifier keys.
fn is_keyboard_device(file: &File) -> bool {
    unsafe {
        let mut ev_bits = [0u8; 4];
        if libc::ioctl(file.as_raw_fd(), EVIOCGBIT_EV, ev_bits.as_mut_ptr()) < 0 {
            return false;
        }
        if ev_bits[EV_KEY as usize / 8] & (1 << (EV_KEY % 8)) == 0 {
            return false;
        }

        let mut key_bits = [0u8; 96];
        if libc::ioctl(file.as_raw_fd(), EVIOCGBIT_KEY, key_bits.as_mut_ptr()) < 0 {
            return false;
        }
        // Check for KEY_LEFTALT (56) as a proxy for "this is a keyboard"
        let alt_byte = KEY_LEFTALT as usize / 8;
        let alt_bit = KEY_LEFTALT as usize % 8;
        key_bits[alt_byte] & (1 << alt_bit) != 0
    }
}

// ---------------------------------------------------------------------------
// Main capture loop
// ---------------------------------------------------------------------------

fn run_capture_loop(
    device_paths: &[PathBuf],
    stop: &AtomicBool,
    service: &PepperXService,
    hold_config: TriggerConfig,
    toggle_config: TriggerConfig,
    shared_config: &SharedTriggerConfig,
) -> Result<(), io::Error> {
    let epoll_fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if epoll_fd < 0 {
        return Err(io::Error::last_os_error());
    }

    let mut files: Vec<File> = Vec::new();
    for path in device_paths {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[Pepper X] evdev: skipping {}: {e}", path.display());
                continue;
            }
        };

        let mut event = libc::epoll_event {
            events: libc::EPOLLIN as u32,
            u64: files.len() as u64,
        };
        unsafe {
            if libc::epoll_ctl(epoll_fd, libc::EPOLL_CTL_ADD, file.as_raw_fd(), &mut event) < 0 {
                eprintln!(
                    "[Pepper X] evdev: epoll_ctl failed for {}: {}",
                    path.display(),
                    io::Error::last_os_error()
                );
                continue;
            }
        }
        files.push(file);
    }

    if files.is_empty() {
        unsafe { libc::close(epoll_fd) };
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no keyboard devices could be opened",
        ));
    }

    let hold_state = Mutex::new(ModifierHoldState::new(hold_config.clone(), TriggerMode::Hold));
    let toggle_state = Mutex::new(ModifierHoldState::new(toggle_config.clone(), TriggerMode::Toggle));
    let mut current_hold_config = hold_config;
    let mut current_toggle_config = toggle_config;
    let mut events = [libc::epoll_event { events: 0, u64: 0 }; 8];
    let mut buf = [0u8; INPUT_EVENT_SIZE * 16];

    while !stop.load(Ordering::Relaxed) {
        let nfds =
            unsafe { libc::epoll_wait(epoll_fd, events.as_mut_ptr(), events.len() as i32, 250) };

        if nfds < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            unsafe { libc::close(epoll_fd) };
            return Err(err);
        }

        // Check for config changes on each epoll wake
        if let Ok(guard) = shared_config.lock() {
            let (ref new_hold, ref new_toggle) = *guard;
            if *new_hold != current_hold_config || *new_toggle != current_toggle_config {
                current_hold_config = new_hold.clone();
                current_toggle_config = new_toggle.clone();
                *hold_state.lock().expect("modifier hold state lock poisoned") =
                    ModifierHoldState::new(current_hold_config.clone(), TriggerMode::Hold);
                *toggle_state.lock().expect("modifier toggle state lock poisoned") =
                    ModifierHoldState::new(current_toggle_config.clone(), TriggerMode::Toggle);
            }
        }

        for i in 0..nfds as usize {
            let file_idx = events[i].u64 as usize;
            let file = &files[file_idx];

            let n = match (&*file).read(&mut buf) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(_) => continue,
            };

            let event_count = n / INPUT_EVENT_SIZE;
            for j in 0..event_count {
                let offset = j * INPUT_EVENT_SIZE;
                let input_event: InputEvent =
                    unsafe { std::ptr::read_unaligned(buf.as_ptr().add(offset).cast()) };

                if input_event.type_ != EV_KEY {
                    continue;
                }

                // value: 0 = release, 1 = press, 2 = repeat (ignore repeats)
                let pressed = match input_event.value {
                    0 => false,
                    1 => true,
                    _ => continue,
                };

                let hold_signal = hold_state
                    .lock()
                    .expect("modifier hold state lock poisoned")
                    .handle_key_event(pressed, input_event.code);

                let toggle_recording_active = toggle_state
                    .lock()
                    .expect("modifier toggle state lock poisoned")
                    .recording_active;

                let toggle_signal = toggle_state
                    .lock()
                    .expect("modifier toggle state lock poisoned")
                    .handle_key_event(pressed, input_event.code);

                // When the toggle machine is actively recording, suppress
                // Stop signals from the hold machine so that releasing shared
                // prefix keys does not prematurely end a toggle recording.
                let effective_hold_signal = match hold_signal {
                    Some(HoldSignal::Stop) if toggle_recording_active => None,
                    other => other,
                };

                // Hold takes priority if both fire simultaneously
                let signal = effective_hold_signal.or(toggle_signal);
                match signal {
                    Some(HoldSignal::Start) => service.start_modifier_only_recording(),
                    Some(HoldSignal::Stop) => service.stop_modifier_only_recording(),
                    None => {}
                }
            }
        }
    }

    unsafe { libc::close(epoll_fd) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alt_super_hold() -> ModifierHoldState {
        ModifierHoldState::new(TriggerConfig::from_setting("alt+super"), TriggerMode::Hold)
    }

    fn ctrl_hold() -> ModifierHoldState {
        ModifierHoldState::new(TriggerConfig::from_setting("ctrl"), TriggerMode::Hold)
    }

    fn right_alt_hold() -> ModifierHoldState {
        ModifierHoldState::new(TriggerConfig::from_setting("right_alt"), TriggerMode::Hold)
    }

    fn alt_super_toggle() -> ModifierHoldState {
        ModifierHoldState::new(
            TriggerConfig::from_setting("alt+super"),
            TriggerMode::Toggle,
        )
    }

    // ---- Hold mode: alt+super (original behavior) ----

    #[test]
    fn hold_requires_both_alt_and_super() {
        let mut state = alt_super_hold();
        assert_eq!(state.handle_key_event(true, KEY_LEFTALT), None);
        assert_eq!(
            state.handle_key_event(true, KEY_LEFTMETA),
            Some(HoldSignal::Start)
        );
        assert_eq!(
            state.handle_key_event(false, KEY_LEFTALT),
            Some(HoldSignal::Stop)
        );
    }

    #[test]
    fn hold_super_then_alt() {
        let mut state = alt_super_hold();
        assert_eq!(state.handle_key_event(true, KEY_LEFTMETA), None);
        assert_eq!(
            state.handle_key_event(true, KEY_LEFTALT),
            Some(HoldSignal::Start)
        );
        assert_eq!(
            state.handle_key_event(false, KEY_LEFTMETA),
            Some(HoldSignal::Stop)
        );
    }

    #[test]
    fn chord_cancels_hold() {
        let mut state = alt_super_hold();
        state.handle_key_event(true, KEY_LEFTALT);
        state.handle_key_event(true, KEY_LEFTMETA);
        assert_eq!(
            state.handle_key_event(true, 46), // KEY_C
            Some(HoldSignal::Stop)
        );
        assert_eq!(state.handle_key_event(false, KEY_LEFTALT), None);
        assert_eq!(state.handle_key_event(false, KEY_LEFTMETA), None);
    }

    #[test]
    fn alt_alone_then_other_key_doesnt_trigger() {
        let mut state = alt_super_hold();
        assert_eq!(state.handle_key_event(true, KEY_LEFTALT), None);
        assert_eq!(state.handle_key_event(true, 46), None);
        assert_eq!(state.handle_key_event(true, KEY_LEFTMETA), None);
        assert_eq!(state.handle_key_event(false, KEY_LEFTALT), None);
        assert_eq!(state.handle_key_event(false, KEY_LEFTMETA), None);
        assert_eq!(state.handle_key_event(true, KEY_LEFTALT), None);
        assert_eq!(
            state.handle_key_event(true, KEY_LEFTMETA),
            Some(HoldSignal::Start)
        );
    }

    #[test]
    fn right_modifiers_work() {
        let mut state = alt_super_hold();
        assert_eq!(state.handle_key_event(true, KEY_RIGHTALT), None);
        assert_eq!(
            state.handle_key_event(true, KEY_RIGHTMETA),
            Some(HoldSignal::Start)
        );
        assert_eq!(
            state.handle_key_event(false, KEY_RIGHTMETA),
            Some(HoldSignal::Stop)
        );
    }

    // ---- Hold mode: ctrl ----

    #[test]
    fn ctrl_hold_starts_on_any_ctrl_press() {
        let mut state = ctrl_hold();
        assert_eq!(
            state.handle_key_event(true, KEY_LEFTCTRL),
            Some(HoldSignal::Start)
        );
        assert_eq!(
            state.handle_key_event(false, KEY_LEFTCTRL),
            Some(HoldSignal::Stop)
        );
    }

    #[test]
    fn ctrl_hold_chord_cancels() {
        let mut state = ctrl_hold();
        state.handle_key_event(true, KEY_LEFTCTRL);
        assert_eq!(
            state.handle_key_event(true, 46),
            Some(HoldSignal::Stop)
        );
        assert_eq!(state.handle_key_event(false, KEY_LEFTCTRL), None);
    }

    #[test]
    fn ctrl_hold_right_ctrl_works() {
        let mut state = ctrl_hold();
        assert_eq!(
            state.handle_key_event(true, KEY_RIGHTCTRL),
            Some(HoldSignal::Start)
        );
        assert_eq!(
            state.handle_key_event(false, KEY_RIGHTCTRL),
            Some(HoldSignal::Stop)
        );
    }

    // ---- Hold mode: right_alt ----

    #[test]
    fn right_alt_hold_only_responds_to_right_alt() {
        let mut state = right_alt_hold();
        // Left alt should not trigger
        assert_eq!(state.handle_key_event(true, KEY_LEFTALT), None);
        assert_eq!(state.handle_key_event(false, KEY_LEFTALT), None);
        // Right alt triggers
        assert_eq!(
            state.handle_key_event(true, KEY_RIGHTALT),
            Some(HoldSignal::Start)
        );
        assert_eq!(
            state.handle_key_event(false, KEY_RIGHTALT),
            Some(HoldSignal::Stop)
        );
    }

    // ---- Toggle mode: alt+super ----

    #[test]
    fn toggle_starts_on_release_and_stops_on_second_press() {
        let mut state = alt_super_toggle();
        // Press both — no signal yet
        assert_eq!(state.handle_key_event(true, KEY_LEFTALT), None);
        assert_eq!(state.handle_key_event(true, KEY_LEFTMETA), None);
        // Release one — fires Start
        assert_eq!(
            state.handle_key_event(false, KEY_LEFTMETA),
            Some(HoldSignal::Start)
        );
        assert_eq!(state.handle_key_event(false, KEY_LEFTALT), None);
        // Press both again
        assert_eq!(state.handle_key_event(true, KEY_LEFTALT), None);
        assert_eq!(state.handle_key_event(true, KEY_LEFTMETA), None);
        // Release — fires Stop
        assert_eq!(
            state.handle_key_event(false, KEY_LEFTMETA),
            Some(HoldSignal::Stop)
        );
    }

    #[test]
    fn toggle_chord_blocks_pending_toggle() {
        let mut state = alt_super_toggle();
        assert_eq!(state.handle_key_event(true, KEY_LEFTALT), None);
        assert_eq!(state.handle_key_event(true, KEY_LEFTMETA), None);
        // Non-modifier key while holding — blocks the toggle
        assert_eq!(state.handle_key_event(true, 46), None);
        // Release — no signal because chord blocked
        assert_eq!(state.handle_key_event(false, KEY_LEFTMETA), None);
        assert_eq!(state.handle_key_event(false, KEY_LEFTALT), None);
        // Next attempt should work
        assert_eq!(state.handle_key_event(true, KEY_LEFTALT), None);
        assert_eq!(state.handle_key_event(true, KEY_LEFTMETA), None);
        assert_eq!(
            state.handle_key_event(false, KEY_LEFTALT),
            Some(HoldSignal::Start)
        );
    }

    // ---- TriggerConfig parsing ----

    #[test]
    fn trigger_config_parses_alt_super() {
        let config = TriggerConfig::from_setting("alt+super");
        assert_eq!(config.groups.len(), 2);
        assert!(config.is_trigger_key(KEY_LEFTALT));
        assert!(config.is_trigger_key(KEY_RIGHTALT));
        assert!(config.is_trigger_key(KEY_LEFTMETA));
        assert!(config.is_trigger_key(KEY_RIGHTMETA));
        assert!(!config.is_trigger_key(KEY_LEFTCTRL));
    }

    #[test]
    fn trigger_config_parses_ctrl() {
        let config = TriggerConfig::from_setting("ctrl");
        assert_eq!(config.groups.len(), 1);
        assert!(config.is_trigger_key(KEY_LEFTCTRL));
        assert!(config.is_trigger_key(KEY_RIGHTCTRL));
        assert!(!config.is_trigger_key(KEY_LEFTALT));
    }

    #[test]
    fn trigger_config_parses_right_alt() {
        let config = TriggerConfig::from_setting("right_alt");
        assert_eq!(config.groups.len(), 1);
        assert!(config.is_trigger_key(KEY_RIGHTALT));
        assert!(!config.is_trigger_key(KEY_LEFTALT));
    }

    #[test]
    fn trigger_config_unknown_falls_back_to_alt_super() {
        let config = TriggerConfig::from_setting("unknown");
        assert_eq!(config.groups.len(), 2);
        assert!(config.is_trigger_key(KEY_LEFTALT));
        assert!(config.is_trigger_key(KEY_LEFTMETA));
    }

    #[test]
    fn trigger_mode_parses_hold() {
        assert_eq!(TriggerMode::from_setting("hold"), TriggerMode::Hold);
        assert_eq!(
            TriggerMode::from_setting("modifier-only"),
            TriggerMode::Hold
        );
    }

    #[test]
    fn trigger_mode_parses_toggle() {
        assert_eq!(TriggerMode::from_setting("toggle"), TriggerMode::Toggle);
        assert_eq!(
            TriggerMode::from_setting("standard-shortcut"),
            TriggerMode::Toggle
        );
    }

    // ---- Comma-separated keycode parsing ----

    #[test]
    fn trigger_config_parses_comma_separated_keycodes() {
        // "56,100,125,126" = left-alt, right-alt, left-meta, right-meta
        let config = TriggerConfig::from_setting("56,100,125,126");
        assert_eq!(config.groups.len(), 2);
        assert!(config.is_trigger_key(KEY_LEFTALT));
        assert!(config.is_trigger_key(KEY_RIGHTALT));
        assert!(config.is_trigger_key(KEY_LEFTMETA));
        assert!(config.is_trigger_key(KEY_RIGHTMETA));
    }

    #[test]
    fn trigger_config_parses_single_keycode() {
        // "100" = right-alt only, but grouped with left-alt
        let config = TriggerConfig::from_setting("100");
        assert_eq!(config.groups.len(), 1);
        assert!(config.is_trigger_key(KEY_LEFTALT));
        assert!(config.is_trigger_key(KEY_RIGHTALT));
    }

    #[test]
    fn trigger_config_from_keycodes_groups_modifiers() {
        // Press left-alt + left-super -> groups = [[56,100], [125,126]]
        let config = TriggerConfig::from_keycodes(&[KEY_LEFTALT, KEY_LEFTMETA]);
        assert_eq!(config.groups.len(), 2);
        assert_eq!(config.groups[0], vec![KEY_LEFTALT, KEY_RIGHTALT]);
        assert_eq!(config.groups[1], vec![KEY_LEFTMETA, KEY_RIGHTMETA]);
    }

    #[test]
    fn trigger_config_from_keycodes_non_modifier_gets_own_group() {
        let config = TriggerConfig::from_keycodes(&[KEY_LEFTALT, KEY_SPACE]);
        assert_eq!(config.groups.len(), 2);
        assert_eq!(config.groups[0], vec![KEY_LEFTALT, KEY_RIGHTALT]);
        assert_eq!(config.groups[1], vec![KEY_SPACE]);
    }

    #[test]
    fn trigger_config_from_keycodes_deduplicates_pairs() {
        // If user pressed both left-alt and right-alt, still one group
        let config = TriggerConfig::from_keycodes(&[KEY_LEFTALT, KEY_RIGHTALT, KEY_LEFTMETA]);
        assert_eq!(config.groups.len(), 2);
        assert_eq!(config.groups[0], vec![KEY_LEFTALT, KEY_RIGHTALT]);
        assert_eq!(config.groups[1], vec![KEY_LEFTMETA, KEY_RIGHTMETA]);
    }

    // ---- to_setting serialization ----

    #[test]
    fn to_setting_round_trips_alt_super() {
        let config = TriggerConfig::from_setting("alt+super");
        let setting = config.to_setting();
        let reparsed = TriggerConfig::from_setting(&setting);
        assert_eq!(config, reparsed);
    }

    #[test]
    fn to_setting_round_trips_ctrl() {
        let config = TriggerConfig::from_setting("ctrl");
        let setting = config.to_setting();
        let reparsed = TriggerConfig::from_setting(&setting);
        assert_eq!(config, reparsed);
    }

    #[test]
    fn to_setting_produces_sorted_keycodes() {
        let config = TriggerConfig::from_keycodes(&[KEY_LEFTMETA, KEY_LEFTALT]);
        let setting = config.to_setting();
        // Keycodes should be sorted: 56,100,125,126
        assert_eq!(setting, "56,100,125,126");
    }

    // ---- Display names ----

    #[test]
    fn display_name_alt_super() {
        let config = TriggerConfig::from_setting("alt+super");
        assert_eq!(config.display_name(), "Alt + Super");
    }

    #[test]
    fn display_name_ctrl() {
        let config = TriggerConfig::from_setting("ctrl");
        assert_eq!(config.display_name(), "Ctrl");
    }

    #[test]
    fn display_name_from_keycodes() {
        let config = TriggerConfig::from_keycodes(&[KEY_LEFTSHIFT, KEY_LEFTMETA]);
        assert_eq!(config.display_name(), "Shift + Super");
    }

    #[test]
    fn trigger_keys_display_name_for_named_presets() {
        assert_eq!(trigger_keys_display_name("alt+super"), "Alt + Super");
        assert_eq!(trigger_keys_display_name("ctrl"), "Ctrl");
        assert_eq!(trigger_keys_display_name("right_alt"), "Alt");
    }

    #[test]
    fn trigger_keys_display_name_for_keycodes() {
        assert_eq!(trigger_keys_display_name("29,97"), "Ctrl");
        assert_eq!(trigger_keys_display_name("56,100,125,126"), "Alt + Super");
    }

    // ---- GDK keyval mapping ----

    #[test]
    fn gdk_keyval_maps_to_evdev() {
        assert_eq!(gdk_keyval_to_evdev(0xffe3), Some(KEY_LEFTCTRL));
        assert_eq!(gdk_keyval_to_evdev(0xffe9), Some(KEY_LEFTALT));
        assert_eq!(gdk_keyval_to_evdev(0xffeb), Some(KEY_LEFTMETA));
        assert_eq!(gdk_keyval_to_evdev(0x20), Some(KEY_SPACE));
        assert_eq!(gdk_keyval_to_evdev(0x41), None); // 'A' — not mapped
    }
}
