use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, SynchronizationCode};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

const SOCKET_ENV: &str = "PEPPERX_UINPUT_HELPER_SOCKET";
const STARTUP_DELAY: Duration = Duration::from_millis(250);

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
struct KeyStroke {
    key: KeyCode,
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
    let mut device = create_virtual_keyboard()?;

    loop {
        let (stream, _) = listener
            .accept()
            .map_err(|error| format!("failed to accept helper connection: {error}"))?;
        handle_connection(stream, &mut device)?;
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

fn create_virtual_keyboard() -> Result<VirtualDevice, String> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for key in supported_key_codes() {
        keys.insert(*key);
    }

    let device = VirtualDevice::builder()
        .map_err(|error| format!("failed to create uinput builder: {error}"))?
        .name("Pepper X virtual keyboard")
        .with_keys(&keys)
        .map_err(|error| format!("failed to configure keyboard capabilities: {error}"))?
        .build()
        .map_err(|error| format!("failed to create Pepper X uinput device: {error}"))?;

    // Give the kernel time to publish the virtual keyboard before the first write.
    std::thread::sleep(STARTUP_DELAY);
    Ok(device)
}

fn handle_connection(mut stream: UnixStream, device: &mut VirtualDevice) -> Result<(), String> {
    let request: UinputInsertRequest = serde_json::from_reader(BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("failed to clone helper stream: {error}"))?,
    ))
    .map_err(|error| format!("failed to parse helper request: {error}"))?;

    let response = match type_text(device, &request.text) {
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

fn type_text(device: &mut VirtualDevice, text: &str) -> Result<(), String> {
    for ch in text.chars() {
        let stroke = keystroke_for_char(ch)?;
        emit_keystroke(device, stroke)?;
    }

    Ok(())
}

fn emit_keystroke(device: &mut VirtualDevice, stroke: KeyStroke) -> Result<(), String> {
    if stroke.shift {
        emit_key(device, KeyCode::KEY_LEFTSHIFT, 1)?;
    }

    emit_key(device, stroke.key, 1)?;
    emit_key(device, stroke.key, 0)?;

    if stroke.shift {
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

fn supported_key_codes() -> &'static [KeyCode] {
    &[
        KeyCode::KEY_A,
        KeyCode::KEY_B,
        KeyCode::KEY_C,
        KeyCode::KEY_D,
        KeyCode::KEY_E,
        KeyCode::KEY_F,
        KeyCode::KEY_G,
        KeyCode::KEY_H,
        KeyCode::KEY_I,
        KeyCode::KEY_J,
        KeyCode::KEY_K,
        KeyCode::KEY_L,
        KeyCode::KEY_M,
        KeyCode::KEY_N,
        KeyCode::KEY_O,
        KeyCode::KEY_P,
        KeyCode::KEY_Q,
        KeyCode::KEY_R,
        KeyCode::KEY_S,
        KeyCode::KEY_T,
        KeyCode::KEY_U,
        KeyCode::KEY_V,
        KeyCode::KEY_W,
        KeyCode::KEY_X,
        KeyCode::KEY_Y,
        KeyCode::KEY_Z,
        KeyCode::KEY_0,
        KeyCode::KEY_1,
        KeyCode::KEY_2,
        KeyCode::KEY_3,
        KeyCode::KEY_4,
        KeyCode::KEY_5,
        KeyCode::KEY_6,
        KeyCode::KEY_7,
        KeyCode::KEY_8,
        KeyCode::KEY_9,
        KeyCode::KEY_SPACE,
        KeyCode::KEY_ENTER,
        KeyCode::KEY_TAB,
        KeyCode::KEY_MINUS,
        KeyCode::KEY_EQUAL,
        KeyCode::KEY_LEFTBRACE,
        KeyCode::KEY_RIGHTBRACE,
        KeyCode::KEY_SEMICOLON,
        KeyCode::KEY_APOSTROPHE,
        KeyCode::KEY_GRAVE,
        KeyCode::KEY_BACKSLASH,
        KeyCode::KEY_COMMA,
        KeyCode::KEY_DOT,
        KeyCode::KEY_SLASH,
        KeyCode::KEY_LEFTSHIFT,
    ]
}

fn keystroke_for_char(ch: char) -> Result<KeyStroke, String> {
    use evdev::KeyCode as K;

    let stroke = match ch {
        'a' => KeyStroke {
            key: K::KEY_A,
            shift: false,
        },
        'b' => KeyStroke {
            key: K::KEY_B,
            shift: false,
        },
        'c' => KeyStroke {
            key: K::KEY_C,
            shift: false,
        },
        'd' => KeyStroke {
            key: K::KEY_D,
            shift: false,
        },
        'e' => KeyStroke {
            key: K::KEY_E,
            shift: false,
        },
        'f' => KeyStroke {
            key: K::KEY_F,
            shift: false,
        },
        'g' => KeyStroke {
            key: K::KEY_G,
            shift: false,
        },
        'h' => KeyStroke {
            key: K::KEY_H,
            shift: false,
        },
        'i' => KeyStroke {
            key: K::KEY_I,
            shift: false,
        },
        'j' => KeyStroke {
            key: K::KEY_J,
            shift: false,
        },
        'k' => KeyStroke {
            key: K::KEY_K,
            shift: false,
        },
        'l' => KeyStroke {
            key: K::KEY_L,
            shift: false,
        },
        'm' => KeyStroke {
            key: K::KEY_M,
            shift: false,
        },
        'n' => KeyStroke {
            key: K::KEY_N,
            shift: false,
        },
        'o' => KeyStroke {
            key: K::KEY_O,
            shift: false,
        },
        'p' => KeyStroke {
            key: K::KEY_P,
            shift: false,
        },
        'q' => KeyStroke {
            key: K::KEY_Q,
            shift: false,
        },
        'r' => KeyStroke {
            key: K::KEY_R,
            shift: false,
        },
        's' => KeyStroke {
            key: K::KEY_S,
            shift: false,
        },
        't' => KeyStroke {
            key: K::KEY_T,
            shift: false,
        },
        'u' => KeyStroke {
            key: K::KEY_U,
            shift: false,
        },
        'v' => KeyStroke {
            key: K::KEY_V,
            shift: false,
        },
        'w' => KeyStroke {
            key: K::KEY_W,
            shift: false,
        },
        'x' => KeyStroke {
            key: K::KEY_X,
            shift: false,
        },
        'y' => KeyStroke {
            key: K::KEY_Y,
            shift: false,
        },
        'z' => KeyStroke {
            key: K::KEY_Z,
            shift: false,
        },
        'A' => KeyStroke {
            key: K::KEY_A,
            shift: true,
        },
        'B' => KeyStroke {
            key: K::KEY_B,
            shift: true,
        },
        'C' => KeyStroke {
            key: K::KEY_C,
            shift: true,
        },
        'D' => KeyStroke {
            key: K::KEY_D,
            shift: true,
        },
        'E' => KeyStroke {
            key: K::KEY_E,
            shift: true,
        },
        'F' => KeyStroke {
            key: K::KEY_F,
            shift: true,
        },
        'G' => KeyStroke {
            key: K::KEY_G,
            shift: true,
        },
        'H' => KeyStroke {
            key: K::KEY_H,
            shift: true,
        },
        'I' => KeyStroke {
            key: K::KEY_I,
            shift: true,
        },
        'J' => KeyStroke {
            key: K::KEY_J,
            shift: true,
        },
        'K' => KeyStroke {
            key: K::KEY_K,
            shift: true,
        },
        'L' => KeyStroke {
            key: K::KEY_L,
            shift: true,
        },
        'M' => KeyStroke {
            key: K::KEY_M,
            shift: true,
        },
        'N' => KeyStroke {
            key: K::KEY_N,
            shift: true,
        },
        'O' => KeyStroke {
            key: K::KEY_O,
            shift: true,
        },
        'P' => KeyStroke {
            key: K::KEY_P,
            shift: true,
        },
        'Q' => KeyStroke {
            key: K::KEY_Q,
            shift: true,
        },
        'R' => KeyStroke {
            key: K::KEY_R,
            shift: true,
        },
        'S' => KeyStroke {
            key: K::KEY_S,
            shift: true,
        },
        'T' => KeyStroke {
            key: K::KEY_T,
            shift: true,
        },
        'U' => KeyStroke {
            key: K::KEY_U,
            shift: true,
        },
        'V' => KeyStroke {
            key: K::KEY_V,
            shift: true,
        },
        'W' => KeyStroke {
            key: K::KEY_W,
            shift: true,
        },
        'X' => KeyStroke {
            key: K::KEY_X,
            shift: true,
        },
        'Y' => KeyStroke {
            key: K::KEY_Y,
            shift: true,
        },
        'Z' => KeyStroke {
            key: K::KEY_Z,
            shift: true,
        },
        '0' => KeyStroke {
            key: K::KEY_0,
            shift: false,
        },
        '1' => KeyStroke {
            key: K::KEY_1,
            shift: false,
        },
        '2' => KeyStroke {
            key: K::KEY_2,
            shift: false,
        },
        '3' => KeyStroke {
            key: K::KEY_3,
            shift: false,
        },
        '4' => KeyStroke {
            key: K::KEY_4,
            shift: false,
        },
        '5' => KeyStroke {
            key: K::KEY_5,
            shift: false,
        },
        '6' => KeyStroke {
            key: K::KEY_6,
            shift: false,
        },
        '7' => KeyStroke {
            key: K::KEY_7,
            shift: false,
        },
        '8' => KeyStroke {
            key: K::KEY_8,
            shift: false,
        },
        '9' => KeyStroke {
            key: K::KEY_9,
            shift: false,
        },
        '!' => KeyStroke {
            key: K::KEY_1,
            shift: true,
        },
        '@' => KeyStroke {
            key: K::KEY_2,
            shift: true,
        },
        '#' => KeyStroke {
            key: K::KEY_3,
            shift: true,
        },
        '$' => KeyStroke {
            key: K::KEY_4,
            shift: true,
        },
        '%' => KeyStroke {
            key: K::KEY_5,
            shift: true,
        },
        '^' => KeyStroke {
            key: K::KEY_6,
            shift: true,
        },
        '&' => KeyStroke {
            key: K::KEY_7,
            shift: true,
        },
        '*' => KeyStroke {
            key: K::KEY_8,
            shift: true,
        },
        '(' => KeyStroke {
            key: K::KEY_9,
            shift: true,
        },
        ')' => KeyStroke {
            key: K::KEY_0,
            shift: true,
        },
        ' ' => KeyStroke {
            key: K::KEY_SPACE,
            shift: false,
        },
        '\n' => KeyStroke {
            key: K::KEY_ENTER,
            shift: false,
        },
        '\t' => KeyStroke {
            key: K::KEY_TAB,
            shift: false,
        },
        '-' => KeyStroke {
            key: K::KEY_MINUS,
            shift: false,
        },
        '_' => KeyStroke {
            key: K::KEY_MINUS,
            shift: true,
        },
        '=' => KeyStroke {
            key: K::KEY_EQUAL,
            shift: false,
        },
        '+' => KeyStroke {
            key: K::KEY_EQUAL,
            shift: true,
        },
        '[' => KeyStroke {
            key: K::KEY_LEFTBRACE,
            shift: false,
        },
        '{' => KeyStroke {
            key: K::KEY_LEFTBRACE,
            shift: true,
        },
        ']' => KeyStroke {
            key: K::KEY_RIGHTBRACE,
            shift: false,
        },
        '}' => KeyStroke {
            key: K::KEY_RIGHTBRACE,
            shift: true,
        },
        ';' => KeyStroke {
            key: K::KEY_SEMICOLON,
            shift: false,
        },
        ':' => KeyStroke {
            key: K::KEY_SEMICOLON,
            shift: true,
        },
        '\'' => KeyStroke {
            key: K::KEY_APOSTROPHE,
            shift: false,
        },
        '"' => KeyStroke {
            key: K::KEY_APOSTROPHE,
            shift: true,
        },
        '`' => KeyStroke {
            key: K::KEY_GRAVE,
            shift: false,
        },
        '~' => KeyStroke {
            key: K::KEY_GRAVE,
            shift: true,
        },
        '\\' => KeyStroke {
            key: K::KEY_BACKSLASH,
            shift: false,
        },
        '|' => KeyStroke {
            key: K::KEY_BACKSLASH,
            shift: true,
        },
        ',' => KeyStroke {
            key: K::KEY_COMMA,
            shift: false,
        },
        '<' => KeyStroke {
            key: K::KEY_COMMA,
            shift: true,
        },
        '.' => KeyStroke {
            key: K::KEY_DOT,
            shift: false,
        },
        '>' => KeyStroke {
            key: K::KEY_DOT,
            shift: true,
        },
        '/' => KeyStroke {
            key: K::KEY_SLASH,
            shift: false,
        },
        '?' => KeyStroke {
            key: K::KEY_SLASH,
            shift: true,
        },
        _ => {
            return Err(format!(
                "Pepper X uinput helper cannot type unsupported character {:?}",
                ch
            ))
        }
    };

    Ok(stroke)
}
