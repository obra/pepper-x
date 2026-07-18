use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, SynchronizationCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use xkbcommon::xkb;

const SOCKET_ENV: &str = "PEPPERX_UINPUT_HELPER_SOCKET";
const STARTUP_DELAY: Duration = Duration::from_millis(250);
const KEY_HOLD_DELAY: Duration = Duration::from_millis(2);
const INTER_KEY_DELAY: Duration = Duration::from_millis(1);
const DEAD_KEY_DELAY: Duration = Duration::from_millis(8);
/// GTK/IBus needs a beat after Ctrl+Shift+U before hex digits are accepted.
const UNICODE_MODE_DELAY: Duration = Duration::from_millis(80);
/// Let the focused app read clipboard contents before we restore the previous value.
const CLIPBOARD_PASTE_DELAY: Duration = Duration::from_millis(80);

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

/// One physical key press, optionally with Shift and/or AltGr (ISO Level3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KeyChord {
    keycode: KeyCode,
    shift: bool,
    alt_gr: bool,
}

/// How to emit one character: layout chord sequence, or Unicode hex entry.
#[derive(Debug, Clone)]
enum CharStroke {
    /// One or more chords (single key, or dead-key + base).
    Chords(Vec<KeyChord>),
    /// Ctrl+Shift+U hex codepoint entry (GNOME/IBus/GTK).
    UnicodeHex(u32),
}

struct CharMapper {
    /// Preferred layout-based sequences (shortest wins at build time).
    map: HashMap<char, Vec<KeyChord>>,
    /// Digits/letters needed for Unicode hex entry.
    hex_digits: HashMap<char, KeyChord>,
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

    // Build from the layout that is actually active right now (not the first listed source).
    // Virtual device keycaps are layout-agnostic (full alphanumeric + modifiers), so we
    // can rebuild only the char→chord map when the user switches layouts mid-session.
    let mut session = LayoutSession::open()?;
    let mut device = create_virtual_keyboard(&session.mapper)?;

    loop {
        let (stream, _) = listener
            .accept()
            .map_err(|error| format!("failed to accept helper connection: {error}"))?;
        handle_connection(stream, &mut device, &mut session)?;
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
// Active layout detection + XKB keymap → character mapping
// ---------------------------------------------------------------------------

/// Identifies an XKB layout/variant pair used to compile a reverse key map.
///
/// uinput injects *keycodes*, and the compositor re-interprets them through
/// the currently selected input source. Our reverse map must therefore match
/// the *active* source at insert time, not the first entry in `sources`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LayoutId {
    layout: String,
    variant: String,
}

impl LayoutId {
    fn new(layout: impl Into<String>, variant: impl Into<String>) -> Self {
        Self {
            layout: layout.into(),
            variant: variant.into(),
        }
    }

    fn display(&self) -> String {
        if self.variant.is_empty() {
            self.layout.clone()
        } else {
            format!("{}+{}", self.layout, self.variant)
        }
    }
}

/// Cached reverse map for the layout that was active at last refresh.
struct LayoutSession {
    id: LayoutId,
    mapper: CharMapper,
    /// How the layout was resolved (for logs).
    source: String,
}

impl LayoutSession {
    fn open() -> Result<Self, String> {
        let (id, source) = resolve_active_layout();
        let mapper = build_char_mapper(&id.layout, &id.variant)?;
        eprintln!(
            "[Pepper X uinput] active layout '{}' (source={source})",
            id.display()
        );
        Ok(Self { id, mapper, source })
    }

    /// Re-probe the active layout. Rebuild the mapper only when it changed.
    fn refresh_if_needed(&mut self) -> Result<(), String> {
        let (id, source) = resolve_active_layout();
        if id == self.id {
            return Ok(());
        }
        eprintln!(
            "[Pepper X uinput] layout switched '{}' → '{}' (source={source})",
            self.id.display(),
            id.display()
        );
        let mapper = build_char_mapper(&id.layout, &id.variant)?;
        self.id = id;
        self.mapper = mapper;
        self.source = source;
        Ok(())
    }
}

/// Resolve layout: env override → GNOME active source → setxkbmap → /etc → us.
fn resolve_active_layout() -> (LayoutId, String) {
    if let Ok(layout_raw) = std::env::var("PEPPERX_XKB_LAYOUT") {
        if !layout_raw.is_empty() {
            let variant_env = std::env::var("PEPPERX_XKB_VARIANT").unwrap_or_default();
            let (layout, variant) = split_layout_variant(&layout_raw, &variant_env);
            return (
                LayoutId::new(layout, variant),
                "env:PEPPERX_XKB_LAYOUT".into(),
            );
        }
    }

    if let Some(id) = detect_gnome_active_layout() {
        return (id, "gsettings:active".into());
    }

    if let Some(id) = detect_setxkbmap_layout() {
        return (id, "setxkbmap".into());
    }

    if let Some(id) = detect_etc_default_keyboard() {
        return (id, "/etc/default/keyboard".into());
    }

    eprintln!("[Pepper X uinput] no layout detected, defaulting to 'us'");
    (LayoutId::new("us", ""), "default".into())
}

/// GNOME: prefer `mru-sources[0]` (currently active), else `sources[current]`.
fn detect_gnome_active_layout() -> Option<LayoutId> {
    // mru-sources[0] is the live selection after Super+Space switches.
    if let Some(raw) = gsettings_get("org.gnome.desktop.input-sources", "mru-sources") {
        let entries = parse_gsettings_input_sources(&raw);
        if let Some(id) = first_xkb_entry(&entries) {
            eprintln!(
                "[Pepper X uinput] detected active layout from mru-sources: {}",
                id.display()
            );
            return Some(id);
        }
    }

    // Fallback: sources[current]
    let sources_raw = gsettings_get("org.gnome.desktop.input-sources", "sources")?;
    let entries = parse_gsettings_input_sources(&sources_raw);
    if entries.is_empty() {
        return None;
    }

    let index = gsettings_get("org.gnome.desktop.input-sources", "current")
        .and_then(|s| parse_gsettings_uint32(&s))
        .unwrap_or(0) as usize;

    let chosen = entries
        .get(index)
        .or_else(|| entries.first())
        .cloned()?;

    if chosen.0 != "xkb" {
        eprintln!(
            "[Pepper X uinput] active input source is '{}' ('{}'), not xkb — using 'us' for keycodes + unicode hex",
            chosen.0, chosen.1
        );
        return Some(LayoutId::new("us", ""));
    }

    let (layout, variant) = split_layout_variant(&chosen.1, "");
    let id = LayoutId::new(layout, variant);
    eprintln!(
        "[Pepper X uinput] detected active layout from sources[{index}]: {}",
        id.display()
    );
    Some(id)
}

fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(stdout)
    }
}

/// Parse `uint32 N` or bare integer from gsettings.
fn parse_gsettings_uint32(raw: &str) -> Option<u32> {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix("uint32") {
        return rest.trim().parse().ok();
    }
    s.parse().ok()
}

/// Parse `[('xkb', 'fr+mac'), ('xkb', 'us'), ('ibus', 'mozc-jp')]`.
fn parse_gsettings_input_sources(raw: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(open) = rest.find('(') {
        rest = &rest[open..];
        match parse_gsettings_pair(rest) {
            Some((kind, id, consumed)) => {
                out.push((kind, id));
                rest = &rest[consumed..];
            }
            None => {
                rest = &rest[1..];
            }
        }
    }
    out
}

/// Parse a single `('type', 'id')` starting at `s[0] == '('`. Returns bytes consumed.
fn parse_gsettings_pair(s: &str) -> Option<(String, String, usize)> {
    if !s.starts_with('(') {
        return None;
    }
    let kind_start = s.find('\'')?;
    let kind_end = kind_start + 1 + s[kind_start + 1..].find('\'')?;
    let kind = s[kind_start + 1..kind_end].to_string();

    let after_kind = &s[kind_end + 1..];
    let id_rel = after_kind.find('\'')?;
    let id_start = kind_end + 1 + id_rel;
    let id_end = id_start + 1 + s[id_start + 1..].find('\'')?;
    let id = s[id_start + 1..id_end].to_string();

    let after_id = &s[id_end + 1..];
    let close_rel = after_id.find(')')?;
    let consumed = id_end + 1 + close_rel + 1;
    Some((kind, id, consumed))
}

fn first_xkb_entry(entries: &[(String, String)]) -> Option<LayoutId> {
    let first = entries.first()?;
    if first.0 != "xkb" {
        // Active source is an IME — keycodes still go through an underlying xkb map.
        // Prefer the first xkb entry in the list; otherwise fall back to us.
        if let Some((_, id)) = entries.iter().find(|(k, _)| k == "xkb") {
            let (layout, variant) = split_layout_variant(id, "");
            return Some(LayoutId::new(layout, variant));
        }
        eprintln!(
            "[Pepper X uinput] active input source is non-xkb ('{}', '{}') — using 'us' + unicode hex",
            first.0, first.1
        );
        return Some(LayoutId::new("us", ""));
    }
    let (layout, variant) = split_layout_variant(&first.1, "");
    Some(LayoutId::new(layout, variant))
}

fn detect_setxkbmap_layout() -> Option<LayoutId> {
    let output = std::process::Command::new("setxkbmap")
        .args(["-query"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut layout_line = None;
    let mut variant_line = None;
    for line in stdout.lines() {
        if let Some(v) = line.strip_prefix("layout:") {
            layout_line = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("variant:") {
            variant_line = Some(v.trim().to_string());
        }
    }
    let layout_csv = layout_line?;
    // setxkbmap may list all layouts comma-separated; first is often the active group on X11.
    let layout = layout_csv.split(',').next()?.trim();
    if layout.is_empty() {
        return None;
    }
    let variant = variant_line
        .as_deref()
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("");
    let id = LayoutId::new(layout, variant);
    eprintln!(
        "[Pepper X uinput] detected layout from setxkbmap: {}",
        id.display()
    );
    Some(id)
}

fn detect_etc_default_keyboard() -> Option<LayoutId> {
    let content = std::fs::read_to_string("/etc/default/keyboard").ok()?;
    let mut layout = None;
    let mut variant = None;
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("XKBLAYOUT=") {
            layout = Some(v.trim_matches('"').trim().to_string());
        } else if let Some(v) = line.strip_prefix("XKBVARIANT=") {
            variant = Some(v.trim_matches('"').trim().to_string());
        }
    }
    let layout_csv = layout?;
    let layout = layout_csv.split(',').next()?.trim();
    if layout.is_empty() {
        return None;
    }
    let variant = variant
        .as_deref()
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("");
    let id = LayoutId::new(layout, variant);
    eprintln!(
        "[Pepper X uinput] detected layout from /etc/default/keyboard: {}",
        id.display()
    );
    Some(id)
}

fn build_char_mapper(layout: &str, variant: &str) -> Result<CharMapper, String> {
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

    let keymap = xkb::Keymap::new_from_names(
        &context,
        "", // rules (default)
        "", // model (default)
        layout,
        variant,
        None, // options
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or_else(|| {
        format!("failed to compile XKB keymap for layout '{layout}', variant '{variant}'")
    })?;

    let shift_idx = keymap.mod_get_index(xkb::MOD_NAME_SHIFT);
    let level3_idx = keymap.mod_get_index(xkb::MOD_NAME_ISO_LEVEL3_SHIFT);

    let shift_mask = mod_bit(shift_idx);
    let level3_mask = mod_bit(level3_idx);

    let mod_combos: &[(u32, bool, bool)] = &[
        (0, false, false),
        (shift_mask, true, false),
        (level3_mask, false, true),
        (shift_mask | level3_mask, true, true),
    ];

    let mut state = xkb::State::new(&keymap);
    let mut map: HashMap<char, Vec<KeyChord>> = HashMap::new();
    // Base keysyms that can be typed with a single chord (for dead-key compose).
    let mut base_keysyms: Vec<(xkb::Keysym, KeyChord)> = Vec::new();
    let mut dead_keys: Vec<(xkb::Keysym, KeyChord)> = Vec::new();

    let min_keycode = keymap.min_keycode().raw();
    let max_keycode = keymap.max_keycode().raw();

    for &(mods, shift, alt_gr) in mod_combos {
        if (shift && shift_mask == 0) || (alt_gr && level3_mask == 0) {
            continue;
        }

        state.update_mask(mods, 0, 0, 0, 0, 0);

        for raw_kc in min_keycode..=max_keycode {
            let xkb_keycode = xkb::Keycode::new(raw_kc);
            let evdev_code = match raw_kc.checked_sub(XKB_EVDEV_OFFSET) {
                Some(code) if code > 0 && code <= u16::MAX as u32 => code as u16,
                _ => continue,
            };
            let chord = KeyChord {
                keycode: KeyCode::new(evdev_code),
                shift,
                alt_gr,
            };

            let sym = state.key_get_one_sym(xkb_keycode);
            if sym.raw() == 0 {
                continue;
            }

            let name = xkb::keysym_get_name(sym);
            if name.starts_with("dead_") {
                // Prefer unshifted dead keys when possible.
                if !dead_keys.iter().any(|(s, _)| *s == sym) {
                    dead_keys.push((sym, chord));
                }
                continue;
            }

            let utf32 = state.key_get_utf32(xkb_keycode);
            if utf32 == 0 || utf32 > 0x10FFFF {
                continue;
            }
            let Some(ch) = char::from_u32(utf32) else {
                continue;
            };
            if ch.is_control() && ch != '\n' && ch != '\t' {
                continue;
            }

            // Prefer shorter / simpler chords (no AltGr over AltGr, no shift over shift).
            let seq = vec![chord];
            insert_preferred_sequence(&mut map, ch, seq);

            if !base_keysyms.iter().any(|(s, _)| *s == sym) {
                base_keysyms.push((sym, chord));
            }
        }
    }

    // Compose dead-key sequences (e.g. dead_circumflex + e → ê on French AZERTY).
    let compose_added = expand_dead_keys(&context, &dead_keys, &base_keysyms, &mut map);

    // Ensure space, enter, tab are mapped even if the keymap is weird.
    map.entry(' ').or_insert_with(|| {
        vec![KeyChord {
            keycode: KeyCode::KEY_SPACE,
            shift: false,
            alt_gr: false,
        }]
    });
    map.entry('\n').or_insert_with(|| {
        vec![KeyChord {
            keycode: KeyCode::KEY_ENTER,
            shift: false,
            alt_gr: false,
        }]
    });
    map.entry('\t').or_insert_with(|| {
        vec![KeyChord {
            keycode: KeyCode::KEY_TAB,
            shift: false,
            alt_gr: false,
        }]
    });

    let hex_digits = build_hex_digit_map(&map);

    eprintln!(
        "[Pepper X uinput] XKB layout '{layout}' variant '{variant}' loaded, {} characters mapped ({} dead-key combos, {} hex digits)",
        map.len(),
        compose_added,
        hex_digits.len()
    );

    Ok(CharMapper { map, hex_digits })
}

fn mod_bit(index: xkb::ModIndex) -> u32 {
    if index == xkb::MOD_INVALID || index >= 31 {
        0
    } else {
        1u32 << index
    }
}

fn split_layout_variant<'a>(layout_raw: &'a str, variant_env: &'a str) -> (&'a str, &'a str) {
    if !variant_env.is_empty() {
        return (layout_raw, variant_env);
    }
    // gsettings may report "fr+mac" as a single token.
    if let Some((layout, variant)) = layout_raw.split_once('+') {
        (layout, variant)
    } else {
        (layout_raw, "")
    }
}

fn insert_preferred_sequence(map: &mut HashMap<char, Vec<KeyChord>>, ch: char, seq: Vec<KeyChord>) {
    match map.get(&ch) {
        None => {
            map.insert(ch, seq);
        }
        Some(existing) => {
            if sequence_rank(&seq) < sequence_rank(existing) {
                map.insert(ch, seq);
            }
        }
    }
}

/// Lower is better: fewer chords, fewer modifiers.
fn sequence_rank(seq: &[KeyChord]) -> (usize, usize) {
    let mod_cost: usize = seq
        .iter()
        .map(|c| usize::from(c.shift) + usize::from(c.alt_gr))
        .sum();
    (seq.len(), mod_cost)
}

fn expand_dead_keys(
    context: &xkb::Context,
    dead_keys: &[(xkb::Keysym, KeyChord)],
    base_keysyms: &[(xkb::Keysym, KeyChord)],
    map: &mut HashMap<char, Vec<KeyChord>>,
) -> usize {
    if dead_keys.is_empty() || base_keysyms.is_empty() {
        return 0;
    }

    let locale = compose_locale();
    let table = match xkb::compose::Table::new_from_locale(
        context,
        OsStr::new(&locale),
        xkb::compose::COMPILE_NO_FLAGS,
    ) {
        Ok(t) => t,
        Err(()) => {
            eprintln!(
                "[Pepper X uinput] compose table unavailable for locale '{locale}'; dead keys limited"
            );
            return 0;
        }
    };

    let mut added = 0usize;
    for &(dead_sym, dead_chord) in dead_keys {
        for &(base_sym, base_chord) in base_keysyms {
            let mut compose = xkb::compose::State::new(&table, xkb::compose::STATE_NO_FLAGS);
            compose.feed(dead_sym);
            if compose.status() != xkb::compose::Status::Composing {
                continue;
            }
            compose.feed(base_sym);
            if compose.status() != xkb::compose::Status::Composed {
                continue;
            }
            let Some(utf8) = compose.utf8() else {
                continue;
            };
            let mut chars = utf8.chars();
            let Some(ch) = chars.next() else {
                continue;
            };
            if chars.next().is_some() {
                // Multi-codepoint compose result — skip (rare).
                continue;
            }
            if ch.is_control() && ch != '\n' && ch != '\t' {
                continue;
            }

            let seq = vec![dead_chord, base_chord];
            let replace = match map.get(&ch) {
                None => true,
                Some(existing) => sequence_rank(&seq) < sequence_rank(existing),
            };
            if replace {
                map.insert(ch, seq);
                added += 1;
            }
        }
    }
    added
}

fn compose_locale() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_CTYPE"))
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .filter(|s| !s.is_empty() && s != "C" && !s.starts_with("C."))
        .unwrap_or_else(|| "en_US.UTF-8".into())
}

fn build_hex_digit_map(map: &HashMap<char, Vec<KeyChord>>) -> HashMap<char, KeyChord> {
    let mut hex = HashMap::new();
    for ch in "0123456789abcdef".chars() {
        if let Some(seq) = map.get(&ch) {
            if seq.len() == 1 {
                hex.insert(ch, seq[0]);
            }
        }
    }
    // Hard fallbacks for ASCII hex if layout map is incomplete.
    const FALLBACKS: &[(char, KeyCode, bool)] = &[
        ('0', KeyCode::KEY_0, false),
        ('1', KeyCode::KEY_1, false),
        ('2', KeyCode::KEY_2, false),
        ('3', KeyCode::KEY_3, false),
        ('4', KeyCode::KEY_4, false),
        ('5', KeyCode::KEY_5, false),
        ('6', KeyCode::KEY_6, false),
        ('7', KeyCode::KEY_7, false),
        ('8', KeyCode::KEY_8, false),
        ('9', KeyCode::KEY_9, false),
        ('a', KeyCode::KEY_A, false),
        ('b', KeyCode::KEY_B, false),
        ('c', KeyCode::KEY_C, false),
        ('d', KeyCode::KEY_D, false),
        ('e', KeyCode::KEY_E, false),
        ('f', KeyCode::KEY_F, false),
    ];
    for &(ch, keycode, shift) in FALLBACKS {
        hex.entry(ch).or_insert(KeyChord {
            keycode,
            shift,
            alt_gr: false,
        });
    }
    hex
}

fn resolve_stroke(mapper: &CharMapper, ch: char) -> CharStroke {
    if let Some(seq) = mapper.map.get(&ch) {
        return CharStroke::Chords(seq.clone());
    }
    // Any remaining Unicode: Ctrl+Shift+U hex entry.
    CharStroke::UnicodeHex(ch as u32)
}

// ---------------------------------------------------------------------------
// Virtual keyboard
// ---------------------------------------------------------------------------

fn create_virtual_keyboard(mapper: &CharMapper) -> Result<VirtualDevice, String> {
    let mut keys = AttributeSet::<KeyCode>::new();

    for seq in mapper.map.values() {
        for chord in seq {
            keys.insert(chord.keycode);
        }
    }
    for chord in mapper.hex_digits.values() {
        keys.insert(chord.keycode);
    }

    // Modifiers + Unicode entry + clipboard paste helpers
    keys.insert(KeyCode::KEY_LEFTSHIFT);
    keys.insert(KeyCode::KEY_RIGHTSHIFT);
    keys.insert(KeyCode::KEY_LEFTCTRL);
    keys.insert(KeyCode::KEY_RIGHTCTRL);
    keys.insert(KeyCode::KEY_LEFTALT);
    keys.insert(KeyCode::KEY_RIGHTALT); // AltGr / ISO_Level3_Shift
    keys.insert(KeyCode::KEY_U);
    keys.insert(KeyCode::KEY_V); // Ctrl+V clipboard paste fallback
    keys.insert(KeyCode::KEY_SPACE);
    keys.insert(KeyCode::KEY_ENTER);
    keys.insert(KeyCode::KEY_TAB);

    // Full alphanumeric set so Unicode hex works even if layout map was sparse.
    for code in [
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
        KeyCode::KEY_A,
        KeyCode::KEY_B,
        KeyCode::KEY_C,
        KeyCode::KEY_D,
        KeyCode::KEY_E,
        KeyCode::KEY_F,
    ] {
        keys.insert(code);
    }

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
    session: &mut LayoutSession,
) -> Result<(), String> {
    let request: UinputInsertRequest = serde_json::from_reader(BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("failed to clone helper stream: {error}"))?,
    ))
    .map_err(|error| format!("failed to parse helper request: {error}"))?;

    // Layout may have changed since last insert (Super+Space). Rebuild map first.
    if let Err(error) = session.refresh_if_needed() {
        eprintln!("[Pepper X uinput] layout refresh failed, keeping previous map: {error}");
    }

    let response = match type_text(device, &request.text, &session.mapper) {
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

fn type_text(device: &mut VirtualDevice, text: &str, mapper: &CharMapper) -> Result<(), String> {
    // Resolve every character first so we never partially inject then fail mid-string
    // for a reason other than device I/O.
    let strokes: Vec<(char, CharStroke)> = text
        .chars()
        .map(|ch| (ch, resolve_stroke(mapper, ch)))
        .collect();

    let needs_unicode = strokes
        .iter()
        .any(|(_, stroke)| matches!(stroke, CharStroke::UnicodeHex(_)));

    // Plain QWERTY (us) has no dead keys / accent keys. Ctrl+Shift+U via uinput is
    // notoriously flaky (sticky modifiers, half-committed hex → garbage like ¾/control
    // pictures). When any glyph is missing from the active layout, paste the whole
    // string via clipboard + Ctrl+V — atomic and layout-independent.
    if needs_unicode {
        let unmapped = strokes
            .iter()
            .filter(|(_, s)| matches!(s, CharStroke::UnicodeHex(_)))
            .count();
        match try_paste_via_clipboard(device, text) {
            Ok(()) => {
                eprintln!(
                    "[Pepper X uinput] pasted {} chars via clipboard ({} not on active layout)",
                    strokes.len(),
                    unmapped
                );
                return Ok(());
            }
            Err(error) => {
                eprintln!(
                    "[Pepper X uinput] clipboard paste unavailable ({error}); falling back to unicode hex"
                );
            }
        }
    }

    // Start clean: never inherit a stuck Ctrl/Shift/AltGr from a previous insert.
    release_all_modifiers(device)?;

    let mut unicode_fallbacks = 0u32;
    for (ch, stroke) in &strokes {
        match stroke {
            CharStroke::Chords(seq) => {
                for (i, chord) in seq.iter().enumerate() {
                    emit_chord(device, *chord)?;
                    if i + 1 < seq.len() {
                        // Dead-key sequences need a beat for the client compose state.
                        std::thread::sleep(DEAD_KEY_DELAY);
                    } else {
                        std::thread::sleep(INTER_KEY_DELAY);
                    }
                }
            }
            CharStroke::UnicodeHex(cp) => {
                unicode_fallbacks += 1;
                eprintln!(
                    "[Pepper X uinput] unicode hex fallback for {:?} (U+{cp:04X})",
                    ch
                );
                emit_unicode_hex(device, *cp, mapper)?;
                // Hex entry is easy to leave half-open; force modifiers up before next char.
                release_all_modifiers(device)?;
                std::thread::sleep(INTER_KEY_DELAY);
            }
        }
    }

    release_all_modifiers(device)?;

    if unicode_fallbacks > 0 {
        eprintln!(
            "[Pepper X uinput] typed {} chars ({} via unicode hex)",
            strokes.len(),
            unicode_fallbacks
        );
    }

    Ok(())
}

fn emit_chord(device: &mut VirtualDevice, chord: KeyChord) -> Result<(), String> {
    if chord.alt_gr {
        emit_key(device, KeyCode::KEY_RIGHTALT, 1)?;
    }
    if chord.shift {
        emit_key(device, KeyCode::KEY_LEFTSHIFT, 1)?;
    }

    emit_key(device, chord.keycode, 1)?;
    std::thread::sleep(KEY_HOLD_DELAY);
    emit_key(device, chord.keycode, 0)?;

    if chord.shift {
        emit_key(device, KeyCode::KEY_LEFTSHIFT, 0)?;
    }
    if chord.alt_gr {
        emit_key(device, KeyCode::KEY_RIGHTALT, 0)?;
    }

    Ok(())
}

/// Force-release modifiers we may have pressed. Spurious key-up is harmless; a stuck
/// Ctrl after Ctrl+Shift+U is not (turns later letters into control chars).
fn release_all_modifiers(device: &mut VirtualDevice) -> Result<(), String> {
    for key in [
        KeyCode::KEY_LEFTCTRL,
        KeyCode::KEY_RIGHTCTRL,
        KeyCode::KEY_LEFTSHIFT,
        KeyCode::KEY_RIGHTSHIFT,
        KeyCode::KEY_LEFTALT,
        KeyCode::KEY_RIGHTALT,
    ] {
        emit_key(device, key, 0)?;
    }
    Ok(())
}

/// GNOME/IBus/GTK Unicode entry: Ctrl+Shift+U, hex digits, Space to commit.
///
/// Last-resort path when the active layout cannot type a codepoint and clipboard
/// paste is unavailable. Easy to desync — callers must `release_all_modifiers` after.
fn emit_unicode_hex(
    device: &mut VirtualDevice,
    codepoint: u32,
    mapper: &CharMapper,
) -> Result<(), String> {
    release_all_modifiers(device)?;

    // Enter unicode mode: hold Ctrl+Shift, tap U, then fully release modifiers
    // before any hex digit (GTK rejects digits while modifiers are still down).
    emit_key(device, KeyCode::KEY_LEFTCTRL, 1)?;
    emit_key(device, KeyCode::KEY_LEFTSHIFT, 1)?;
    std::thread::sleep(KEY_HOLD_DELAY);
    emit_key(device, KeyCode::KEY_U, 1)?;
    std::thread::sleep(KEY_HOLD_DELAY);
    emit_key(device, KeyCode::KEY_U, 0)?;
    std::thread::sleep(KEY_HOLD_DELAY);
    emit_key(device, KeyCode::KEY_LEFTSHIFT, 0)?;
    emit_key(device, KeyCode::KEY_LEFTCTRL, 0)?;
    std::thread::sleep(UNICODE_MODE_DELAY);

    let hex = format!("{codepoint:x}");
    for digit in hex.chars() {
        let chord = mapper.hex_digits.get(&digit).copied().ok_or_else(|| {
            format!("internal error: missing hex digit mapping for {digit:?}")
        })?;
        // Hex digits must be plain key presses — never with leftover AltGr/Shift
        // from a previous dead-key chord on another layout.
        if chord.shift || chord.alt_gr {
            emit_chord(device, chord)?;
        } else {
            emit_key(device, chord.keycode, 1)?;
            std::thread::sleep(KEY_HOLD_DELAY);
            emit_key(device, chord.keycode, 0)?;
        }
        std::thread::sleep(DEAD_KEY_DELAY);
    }

    // Commit (Space is safer than Enter — Enter can submit forms).
    emit_key(device, KeyCode::KEY_SPACE, 1)?;
    std::thread::sleep(KEY_HOLD_DELAY);
    emit_key(device, KeyCode::KEY_SPACE, 0)?;
    std::thread::sleep(UNICODE_MODE_DELAY);

    release_all_modifiers(device)?;
    Ok(())
}

/// Paste `text` via the session clipboard + Ctrl+V.
///
/// Used when the active XKB layout cannot express one or more characters as key
/// chords (typical: French accents on plain US QWERTY). Far more reliable than
/// synthetic Ctrl+Shift+U through uinput.
fn try_paste_via_clipboard(device: &mut VirtualDevice, text: &str) -> Result<(), String> {
    let previous = read_clipboard_text();
    set_clipboard_text(text)?;
    std::thread::sleep(Duration::from_millis(20));

    release_all_modifiers(device)?;

    emit_key(device, KeyCode::KEY_LEFTCTRL, 1)?;
    emit_key(device, KeyCode::KEY_V, 1)?;
    std::thread::sleep(KEY_HOLD_DELAY);
    emit_key(device, KeyCode::KEY_V, 0)?;
    emit_key(device, KeyCode::KEY_LEFTCTRL, 0)?;
    release_all_modifiers(device)?;

    // Give the target app time to request clipboard contents before restore.
    std::thread::sleep(CLIPBOARD_PASTE_DELAY);

    match previous {
        Some(prev) => {
            if let Err(error) = set_clipboard_text(&prev) {
                eprintln!("[Pepper X uinput] failed to restore clipboard: {error}");
            }
        }
        None => {
            // Best-effort clear so we do not leave the insert text sitting around.
            let _ = clear_clipboard();
        }
    }

    Ok(())
}

fn set_clipboard_text(text: &str) -> Result<(), String> {
    // Prefer Wayland, then X11 tools. Stdin avoids shell/argv quoting issues.
    if pipe_to_command("wl-copy", &["--type", "text/plain"], text).is_ok() {
        return Ok(());
    }
    if pipe_to_command("xclip", &["-selection", "clipboard"], text).is_ok() {
        return Ok(());
    }
    if pipe_to_command("xsel", &["--clipboard", "--input"], text).is_ok() {
        return Ok(());
    }
    Err("no clipboard tool found (need wl-copy, xclip, or xsel)".into())
}

fn read_clipboard_text() -> Option<String> {
    if let Ok(text) = stdout_from_command("wl-paste", &["--no-newline"]) {
        return Some(text);
    }
    if let Ok(text) = stdout_from_command("xclip", &["-selection", "clipboard", "-o"]) {
        return Some(text);
    }
    if let Ok(text) = stdout_from_command("xsel", &["--clipboard", "--output"]) {
        return Some(text);
    }
    None
}

fn clear_clipboard() -> Result<(), String> {
    set_clipboard_text("")
}

fn pipe_to_command(bin: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{bin}: {error}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| format!("{bin}: stdin not piped"))?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("{bin}: write failed: {error}"))?;
    }
    let status = child
        .wait()
        .map_err(|error| format!("{bin}: wait failed: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{bin}: exit {status}"))
    }
}

fn stdout_from_command(bin: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| format!("{bin}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{bin}: exit {}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("{bin}: utf8: {error}"))
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

// ---------------------------------------------------------------------------
// Tests (layout map only — no /dev/uinput required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn mapper_for_layout(layout: &str) -> CharMapper {
        build_char_mapper(layout, "").expect("keymap should compile")
    }

    #[test]
    fn french_layout_maps_circumflex_e() {
        let mapper = mapper_for_layout("fr");
        // ê must be reachable (dead key ^ + e on basic AZERTY).
        let stroke = resolve_stroke(&mapper, 'ê');
        match stroke {
            CharStroke::Chords(seq) => {
                assert!(
                    seq.len() >= 1,
                    "ê should map to at least one chord, got {seq:?}"
                );
                // Prefer real layout sequence over unicode when possible.
                assert!(
                    seq.len() <= 2,
                    "ê sequence should be short (direct or dead+base), got len {}",
                    seq.len()
                );
            }
            CharStroke::UnicodeHex(cp) => {
                panic!("ê should be layout-mappable on fr, got unicode fallback U+{cp:04X}");
            }
        }
    }

    #[test]
    fn french_layout_maps_common_accents() {
        let mapper = mapper_for_layout("fr");
        for ch in ['é', 'è', 'à', 'ù', 'ç', 'â', 'ê', 'î', 'ô', 'û', 'ë', 'ï', 'ü'] {
            match resolve_stroke(&mapper, ch) {
                CharStroke::Chords(seq) => assert!(!seq.is_empty(), "{ch} empty sequence"),
                CharStroke::UnicodeHex(_) => {
                    // Dead-key expand may miss some depending on compose locale;
                    // unicode fallback still types them.
                }
            }
            // All must resolve without error.
            let _ = resolve_stroke(&mapper, ch);
        }
        // Direct AZERTY base letters with accent keys:
        assert!(mapper.map.contains_key(&'é'), "é should be direct on fr");
        assert!(mapper.map.contains_key(&'è'), "è should be direct on fr");
        assert!(mapper.map.contains_key(&'à'), "à should be direct on fr");
        assert!(mapper.map.contains_key(&'ç'), "ç should be direct on fr");
        assert!(
            mapper.map.contains_key(&'ê'),
            "ê should be in map via dead keys on fr (got {} chars)",
            mapper.map.len()
        );
    }

    #[test]
    fn any_unicode_resolves_via_hex_fallback() {
        let mapper = mapper_for_layout("us");
        for ch in ['😀', '中', 'ß', 'œ', '€'] {
            match resolve_stroke(&mapper, ch) {
                CharStroke::Chords(_) => {}
                CharStroke::UnicodeHex(cp) => assert_eq!(cp, ch as u32),
            }
        }
    }

    #[test]
    fn split_layout_variant_handles_gsettings_plus_form() {
        assert_eq!(split_layout_variant("fr+mac", ""), ("fr", "mac"));
        assert_eq!(split_layout_variant("fr", "oss"), ("fr", "oss"));
        assert_eq!(split_layout_variant("us", ""), ("us", ""));
    }

    #[test]
    fn parse_gsettings_input_sources_list() {
        let raw = "[('xkb', 'fr+mac'), ('xkb', 'fr'), ('xkb', 'us')]";
        let entries = parse_gsettings_input_sources(raw);
        assert_eq!(
            entries,
            vec![
                ("xkb".into(), "fr+mac".into()),
                ("xkb".into(), "fr".into()),
                ("xkb".into(), "us".into()),
            ]
        );
    }

    #[test]
    fn parse_gsettings_input_sources_with_ibus() {
        let raw = "[('ibus', 'mozc-jp'), ('xkb', 'us')]";
        let entries = parse_gsettings_input_sources(raw);
        assert_eq!(entries[0], ("ibus".into(), "mozc-jp".into()));
        assert_eq!(entries[1], ("xkb".into(), "us".into()));
    }

    #[test]
    fn first_xkb_entry_prefers_active_xkb() {
        let entries = vec![
            ("xkb".into(), "us".into()),
            ("xkb".into(), "fr+mac".into()),
        ];
        let id = first_xkb_entry(&entries).unwrap();
        assert_eq!(id, LayoutId::new("us", ""));
    }

    #[test]
    fn first_xkb_entry_skips_ime_to_underlying_xkb() {
        let entries = vec![
            ("ibus".into(), "mozc-jp".into()),
            ("xkb".into(), "fr+mac".into()),
        ];
        let id = first_xkb_entry(&entries).unwrap();
        assert_eq!(id, LayoutId::new("fr", "mac"));
    }

    #[test]
    fn parse_gsettings_uint32_forms() {
        assert_eq!(parse_gsettings_uint32("uint32 2"), Some(2));
        assert_eq!(parse_gsettings_uint32("0"), Some(0));
    }

    #[test]
    fn layout_id_display() {
        assert_eq!(LayoutId::new("fr", "mac").display(), "fr+mac");
        assert_eq!(LayoutId::new("us", "").display(), "us");
    }

    #[test]
    fn apostrophe_phrase_fully_resolves_on_fr() {
        let mapper = mapper_for_layout("fr");
        let text = "C'est peut-être bon";
        for ch in text.chars() {
            let _ = resolve_stroke(&mapper, ch);
            // Must not panic; layout or unicode covers everything.
            match resolve_stroke(&mapper, ch) {
                CharStroke::Chords(seq) => assert!(!seq.is_empty(), "empty for {ch:?}"),
                CharStroke::UnicodeHex(_) => {}
            }
        }
        assert!(
            matches!(
                resolve_stroke(&mapper, 'ê'),
                CharStroke::Chords(_)
            ),
            "ê in sample phrase must use layout chords on fr"
        );
    }

    #[test]
    fn plain_us_has_no_dead_keys_so_french_accents_need_fallback() {
        // Root cause of QWERTY garbage: plain `us` has zero dead keys, so è/ê/… cannot
        // be typed as chords. type_text must then paste (clipboard) or unicode-hex.
        let mapper = mapper_for_layout("us");
        assert!(
            !mapper.map.contains_key(&'è'),
            "plain us must not claim a direct/dead-key mapping for è"
        );
        assert!(
            !mapper.map.contains_key(&'ê'),
            "plain us must not claim a direct/dead-key mapping for ê"
        );
        match resolve_stroke(&mapper, 'è') {
            CharStroke::UnicodeHex(cp) => assert_eq!(cp, 'è' as u32),
            CharStroke::Chords(seq) => panic!("è should not be a chord on plain us: {seq:?}"),
        }
        // ASCII still direct — so mixed strings only paste when an accent appears.
        match resolve_stroke(&mapper, 'T') {
            CharStroke::Chords(seq) => assert_eq!(seq.len(), 1),
            CharStroke::UnicodeHex(_) => panic!("ASCII T must be a chord on us"),
        }
    }

    #[test]
    fn us_intl_maps_french_accents_via_dead_keys() {
        let mapper = build_char_mapper("us", "intl").expect("us(intl) should compile");
        for ch in ['è', 'ê', 'é', 'à'] {
            match resolve_stroke(&mapper, ch) {
                CharStroke::Chords(seq) => {
                    assert!(
                        !seq.is_empty() && seq.len() <= 2,
                        "{ch} should be direct or dead+base on us(intl), got {seq:?}"
                    );
                }
                CharStroke::UnicodeHex(cp) => {
                    panic!("{ch} should be layout-mappable on us(intl), got U+{cp:04X}");
                }
            }
        }
    }

    #[test]
    fn french_sentence_on_us_triggers_unicode_for_accent_only() {
        let mapper = mapper_for_layout("us");
        let text = "Très bien que non";
        let mut unicode_chars = Vec::new();
        for ch in text.chars() {
            if matches!(resolve_stroke(&mapper, ch), CharStroke::UnicodeHex(_)) {
                unicode_chars.push(ch);
            }
        }
        assert_eq!(
            unicode_chars,
            vec!['è'],
            "only è needs fallback on plain us for this phrase"
        );
    }
}
