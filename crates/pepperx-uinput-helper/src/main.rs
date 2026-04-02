use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, SynchronizationCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;
use xkbcommon::xkb;

const SOCKET_ENV: &str = "PEPPERX_UINPUT_HELPER_SOCKET";
const STARTUP_DELAY: Duration = Duration::from_millis(250);
const KEY_HOLD_DELAY: Duration = Duration::from_millis(2);
const INTER_KEY_DELAY: Duration = Duration::from_millis(1);

/// Evdev keycodes start at 8 below XKB keycodes (XKB keycode = evdev keycode + 8).
const XKB_EVDEV_OFFSET: u32 = 8;

#[derive(Debug, Deserialize)]
struct UinputInsertRequest {
    text: String,
}

#[derive(Debug, Serialize)]
struct UinputInsertResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct KeyChord {
    keycode: KeyCode,
    shift: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let socket_path = configured_socket_path()?;
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create helper socket directory: {error}"))?;
    }

    if socket_path.exists() {
        std::fs::remove_file(&socket_path)
            .map_err(|error| format!("failed to remove stale helper socket: {error}"))?;
    }

    let listener = UnixListener::bind(&socket_path).map_err(|error| {
        format!(
            "failed to bind helper socket {}: {error}",
            socket_path.display()
        )
    })?;

    let char_map = build_xkb_char_map()?;
    let mut device = create_virtual_keyboard(&char_map)?;

    loop {
        let (stream, _) = listener
            .accept()
            .map_err(|error| format!("failed to accept helper connection: {error}"))?;
        handle_connection(stream, &mut device, &char_map)?;
    }
}

fn configured_socket_path() -> Result<PathBuf, String> {
    if let Some(socket_path) = std::env::var_os(SOCKET_ENV) {
        return Ok(PathBuf::from(socket_path));
    }

    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| "PEPPERX_UINPUT_HELPER_SOCKET or XDG_RUNTIME_DIR must be set".to_string())?;
    Ok(PathBuf::from(runtime_dir)
        .join("pepper-x")
        .join("uinput-helper.sock"))
}

// ---------------------------------------------------------------------------
// XKB keymap → character mapping
// ---------------------------------------------------------------------------

fn build_xkb_char_map() -> Result<HashMap<char, KeyChord>, String> {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

    let layout = std::env::var("PEPPERX_XKB_LAYOUT").unwrap_or_else(|_| detect_layout());
    let variant = std::env::var("PEPPERX_XKB_VARIANT").unwrap_or_default();

    let keymap = xkb::Keymap::new_from_names(
        &context,
        "",      // rules (default)
        "",      // model (default)
        &layout,
        &variant,
        None,    // options
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or_else(|| {
        format!("failed to compile XKB keymap for layout '{layout}', variant '{variant}'")
    })?;

    let state = xkb::State::new(&keymap);
    let mut char_map = HashMap::new();

    let min_keycode = keymap.min_keycode().raw();
    let max_keycode = keymap.max_keycode().raw();

    for raw_kc in min_keycode..=max_keycode {
        let xkb_keycode = xkb::Keycode::new(raw_kc);
        let num_levels = keymap.num_levels_for_key(xkb_keycode, 0);

        for level in 0..num_levels {
            // For now, only support levels 0 and 1 (unshifted and shifted).
            // AltGr and other modifiers can be added later.
            if level > 1 {
                continue;
            }

            let syms = keymap.key_get_syms_by_level(xkb_keycode, 0, level);

            for sym in syms {
                let ch = xkb::keysym_to_utf32(*sym);
                if ch == 0 || ch > 0x10FFFF {
                    continue;
                }
                let Some(ch) = char::from_u32(ch) else {
                    continue;
                };

                // Skip control characters except newline and tab
                if ch.is_control() && ch != '\n' && ch != '\t' {
                    continue;
                }

                // Only map if we haven't seen this character at a simpler level
                if char_map.contains_key(&ch) {
                    continue;
                }

                // Convert XKB keycode to evdev keycode
                let evdev_code = raw_kc - XKB_EVDEV_OFFSET;

                // Determine if shift is needed: level 0 = no shift, level 1 = shift
                let shift = level == 1;

                char_map.insert(
                    ch,
                    KeyChord {
                        keycode: KeyCode::new(evdev_code as u16),
                        shift,
                    },
                );
            }
        }
    }

    // Ensure space, enter, tab are mapped even if the keymap is weird
    char_map
        .entry(' ')
        .or_insert(KeyChord { keycode: KeyCode::KEY_SPACE, shift: false });
    char_map
        .entry('\n')
        .or_insert(KeyChord { keycode: KeyCode::KEY_ENTER, shift: false });
    char_map
        .entry('\t')
        .or_insert(KeyChord { keycode: KeyCode::KEY_TAB, shift: false });

    eprintln!(
        "[Pepper X uinput] XKB layout '{layout}' loaded, {} characters mapped",
        char_map.len()
    );

    Ok(char_map)
}

fn detect_layout() -> String {
    // Try reading from gsettings
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.input-sources", "sources"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Format: [('xkb', 'us'), ('xkb', 'de')]
        // Extract the first layout
        if let Some(start) = stdout.find("'xkb', '") {
            let rest = &stdout[start + 8..];
            if let Some(end) = rest.find('\'') {
                let layout = &rest[..end];
                if !layout.is_empty() {
                    eprintln!("[Pepper X uinput] detected layout from gsettings: {layout}");
                    return layout.to_string();
                }
            }
        }
    }

    // Try /etc/default/keyboard
    if let Ok(content) = std::fs::read_to_string("/etc/default/keyboard") {
        for line in content.lines() {
            if let Some(layout) = line.strip_prefix("XKBLAYOUT=") {
                let layout = layout.trim_matches('"').trim();
                if !layout.is_empty() {
                    let first = layout.split(',').next().unwrap_or(layout);
                    eprintln!(
                        "[Pepper X uinput] detected layout from /etc/default/keyboard: {first}"
                    );
                    return first.to_string();
                }
            }
        }
    }

    eprintln!("[Pepper X uinput] no layout detected, defaulting to 'us'");
    "us".to_string()
}

// ---------------------------------------------------------------------------
// Virtual keyboard
// ---------------------------------------------------------------------------

fn create_virtual_keyboard(
    char_map: &HashMap<char, KeyChord>,
) -> Result<VirtualDevice, String> {
    let mut keys = AttributeSet::<KeyCode>::new();

    // Register all keycodes from the character map
    for chord in char_map.values() {
        keys.insert(chord.keycode);
    }
    // Always include modifiers
    keys.insert(KeyCode::KEY_LEFTSHIFT);
    keys.insert(KeyCode::KEY_RIGHTSHIFT);

    let device = VirtualDevice::builder()
        .map_err(|error| format!("failed to create uinput builder: {error}"))?
        .name("Pepper X virtual keyboard")
        .with_keys(&keys)
        .map_err(|error| format!("failed to configure keyboard capabilities: {error}"))?
        .build()
        .map_err(|error| format!("failed to create Pepper X uinput device: {error}"))?;

    std::thread::sleep(STARTUP_DELAY);
    Ok(device)
}

// ---------------------------------------------------------------------------
// Connection handling
// ---------------------------------------------------------------------------

fn handle_connection(
    mut stream: UnixStream,
    device: &mut VirtualDevice,
    char_map: &HashMap<char, KeyChord>,
) -> Result<(), String> {
    let request: UinputInsertRequest = serde_json::from_reader(BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("failed to clone helper stream: {error}"))?,
    ))
    .map_err(|error| format!("failed to parse helper request: {error}"))?;

    let response = match type_text(device, &request.text, char_map) {
        Ok(()) => UinputInsertResponse {
            ok: true,
            error: None,
        },
        Err(error) => UinputInsertResponse {
            ok: false,
            error: Some(error),
        },
    };

    serde_json::to_writer(&mut stream, &response)
        .map_err(|error| format!("failed to encode helper response: {error}"))?;
    stream
        .write_all(b"\n")
        .map_err(|error| format!("failed to finish helper response: {error}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Text emission
// ---------------------------------------------------------------------------

fn type_text(
    device: &mut VirtualDevice,
    text: &str,
    char_map: &HashMap<char, KeyChord>,
) -> Result<(), String> {
    // Pre-validate: ensure every character is mappable before typing anything
    for ch in text.chars() {
        if !char_map.contains_key(&ch) {
            return Err(format!(
                "unmappable character {:?} (U+{:04X}) in current keyboard layout",
                ch, ch as u32
            ));
        }
    }

    for ch in text.chars() {
        let chord = char_map[&ch];
        emit_chord(device, chord)?;
        std::thread::sleep(INTER_KEY_DELAY);
    }

    Ok(())
}

fn emit_chord(device: &mut VirtualDevice, chord: KeyChord) -> Result<(), String> {
    if chord.shift {
        emit_key(device, KeyCode::KEY_LEFTSHIFT, 1)?;
    }

    emit_key(device, chord.keycode, 1)?;
    std::thread::sleep(KEY_HOLD_DELAY);
    emit_key(device, chord.keycode, 0)?;

    if chord.shift {
        emit_key(device, KeyCode::KEY_LEFTSHIFT, 0)?;
    }

    Ok(())
}

fn emit_key(device: &mut VirtualDevice, key: KeyCode, value: i32) -> Result<(), String> {
    let events = [
        InputEvent::new(EventType::KEY.0, key.0, value),
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        ),
    ];
    device
        .emit(&events)
        .map_err(|error| format!("failed to emit uinput key event: {error}"))
}
