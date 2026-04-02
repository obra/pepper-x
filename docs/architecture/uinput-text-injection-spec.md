# Behavioral Spec: uinput Text Injection on Wayland/GNOME

## Context

This spec covers the uinput-based text injection backend for Pepper X on GNOME Shell (Wayland). This backend sits at the bottom of the insertion fallback chain defined in `text-insertion-strategy.md`:

1. AT-SPI EditableText (semantic insertion)
2. AT-SPI `generate_keyboard_event` with `ATSPI_KEY_STRING`
3. Clipboard-assisted paste (wl-copy + Ctrl+V)
4. **uinput text injection (this spec)**

The uinput backend is the last resort for targets that do not expose accessibility interfaces: terminals, Xwayland apps, Wine, canvas-heavy custom UIs.

## Why Not wtype?

wtype uses the `zwp_virtual_keyboard_v1` Wayland protocol with dynamically generated XKB keymaps. This is an elegant approach -- it builds a custom XKB keymap containing only the needed characters, uploads it to the compositor via a file descriptor, and then sends key press/release events referencing that keymap. This makes arbitrary Unicode trivial.

However, **GNOME Shell / Mutter does not implement the `zwp_virtual_keyboard_v1` protocol**. This is a well-documented limitation (wtype issues #29, #34, #45). wtype prints "Compositor does not support the virtual keyboard protocol" and exits. This rules out the wtype approach entirely for a GNOME-targeted application.

The protocol is supported by wlroots-based compositors (Sway, Hyprland, etc.) and Phosh, but not by Mutter or KDE's KWin.

## Why uinput?

On GNOME Wayland, the only way to inject synthetic keyboard input from a background process without compositor cooperation is through the Linux kernel's uinput subsystem. This is the approach used by ydotool and dotool.

uinput creates a virtual input device at `/dev/uinput`. The kernel treats events written to this device identically to events from a physical keyboard. The compositor (Mutter) reads these events through its normal evdev input path, so no special protocol support is needed.

## How wtype Works (for reference)

Even though wtype is incompatible with GNOME, its approach is worth understanding because it solves the Unicode problem cleanly:

1. Parse the input text into wide characters.
2. For each unique character, call `xkb_utf32_to_keysym()` to get an XKB keysym.
3. Assign each keysym a unique keycode (starting at keycode 9).
4. Generate a temporary XKB keymap file containing all needed keycodes and their keysym bindings.
5. Upload the keymap to the compositor via `zwp_virtual_keyboard_v1_keymap()`.
6. For each character, send `zwp_virtual_keyboard_v1_key()` with the assigned keycode (press, 2ms hold, release).
7. Between characters, apply a configurable inter-key delay.

Key insight: wtype never needs to know the user's physical keyboard layout. It defines its own keymap, so any Unicode character maps to a known keycode. Modifier keys (shift, etc.) are never needed for text characters because each character gets its own dedicated keycode.

## How ydotool Works (and its Unicode limitation)

ydotool uses a persistent daemon (`ydotoold`) that holds a uinput virtual device open. The client sends commands over a Unix socket. For `type`, ydotool sends raw evdev key events (EV_KEY with Linux keycodes like KEY_A = 30).

The critical limitation: **ydotool cannot type Unicode characters**. It only understands raw Linux keycodes, which are physical key positions (KEY_A, KEY_SEMICOLON, etc.). There is no Unicode-to-keycode mapping -- the tool literally drops non-ASCII characters. GitHub issue #249 confirms this: `ydotool type "aaaa"` works but `ydotool type "aaaa"` (with Unicode) produces nothing or partial output.

ydotool does not use XKB at all. It has no awareness of the active keyboard layout. If the user has a non-US layout, the wrong characters will be produced even for ASCII text, because KEY_A on a French AZERTY keyboard produces "q".

## How dotool Works (the better uinput approach)

dotool is a Go-based tool that improves on ydotool by adding XKB awareness:

1. On startup, load an XKB keymap matching the user's keyboard layout (configurable via `DOTOOL_XKB_LAYOUT` and `DOTOOL_XKB_VARIANT` environment variables, defaulting to "us").
2. For each character to type, convert it to an XKB keysym via `xkb_utf32_to_keysym()`.
3. Look up the keysym in the loaded keymap to find the (keycode, modifiers) chord needed to produce it. This returns the physical key plus any required modifiers (shift, altgr, etc.).
4. If no direct mapping exists, try dead-key sequences (e.g., dead_acute + e = e).
5. Emit the modifier key-down events, then the main key press/release, then modifier key-up events via uinput.
6. Apply timing delays: 8ms key hold, 2ms inter-key delay.

This approach handles any character that exists in the active XKB layout, including shifted characters, AltGr characters, and dead-key combinations. Characters outside the active layout cannot be typed.

## Recommended Approach for Pepper X

### Architecture: Persistent uinput daemon with XKB-aware character mapping

The approach should combine ydotool's persistent daemon architecture (which Pepper X already uses via `pepperx-uinput-helper`) with dotool's XKB-aware character-to-keycode resolution.

### Input

- Arbitrary UTF-8 text string, delivered over a Unix domain socket as a JSON message.
- The text may contain: ASCII letters (upper/lowercase), digits, punctuation, whitespace (space, tab, newline), and common Unicode characters (accented Latin, punctuation marks, currency symbols, etc.).

### Output

- The text appears in the currently focused application as if typed on a physical keyboard.
- Characters appear in order, with no dropped or reordered characters.
- Focus is not stolen from the target application.

### Platform Requirements

- Linux with Wayland, specifically GNOME Shell 46+ (Mutter compositor).
- The helper process runs as a background daemon, not the focused window.
- Requires write access to `/dev/uinput` (typically via `input` group membership or a udev rule granting access to the helper binary).

### Character Mapping Strategy

The helper must resolve each character to a (keycode, modifier_mask) pair using XKB:

1. **Initialize XKB context and keymap.** On startup, create an `xkb_context`, then compile a keymap for the user's active layout. The layout can be determined by:
   - Reading the `DOTOOL_XKB_LAYOUT` / `DOTOOL_XKB_VARIANT` environment variables (for compatibility).
   - Querying the system's configured layout via `gsettings get org.gnome.desktop.input-sources sources` or by reading `/etc/default/keyboard`.
   - Defaulting to `"us"` if nothing else is available.

2. **For each character in the input text:**
   a. Convert the Unicode codepoint to an XKB keysym via `xkb_utf32_to_keysym(codepoint)`.
   b. Iterate over all keycodes in the keymap. For each keycode, iterate over all layout groups and shift levels. Check if any (keycode, level) combination produces the target keysym via `xkb_keymap_key_get_syms_by_level()`.
   c. Determine which modifier keys correspond to the matched level (typically: level 0 = no modifiers, level 1 = Shift, level 2 = AltGr, level 3 = Shift+AltGr).
   d. If no match is found in the primary layout, attempt dead-key composition: search for a dead-key keysym followed by a base keysym that composes to the target character.
   e. If still no match, the character is **unmappable** in the current layout. Fall back to the clipboard-paste path for this specific character, or reject the entire string and let the caller use a different backend.

3. **Cache the mapping.** Build a `HashMap<char, KeyChord>` on first use and reuse it. The keymap rarely changes at runtime.

### Keystroke Emission

For each resolved character:

1. Press all required modifier keys (e.g., KEY_LEFTSHIFT for shift, KEY_RIGHTALT for AltGr). Emit EV_KEY events with value=1 (press), followed by EV_SYN/SYN_REPORT.
2. Press the main key (value=1), then SYN_REPORT.
3. Release the main key (value=0), then SYN_REPORT.
4. Release all modifier keys (value=0), then SYN_REPORT.

For dead-key sequences (e.g., typing e = dead_acute + e):
1. Emit the dead key press/release.
2. Emit the base key press/release.

### Timing

- **Inter-event delay:** No explicit delay between individual EV_KEY events within a single keystroke (press/release of one character). The kernel and compositor handle these at input-event granularity.
- **Inter-character delay:** A small delay (1-2ms) between characters to avoid event coalescing or compositor-side rate limiting. dotool uses 2ms by default.
- **Key hold time:** At least 1ms between press and release of the same key. dotool uses 8ms. This is important because some applications ignore zero-duration key events.
- **Startup delay:** After creating the uinput virtual device, wait 200-300ms before emitting events. The compositor needs time to enumerate and configure the new device. The current `pepperx-uinput-helper` uses 250ms, which is appropriate.
- **Latency budget for short strings:** For a 100-character string at 10ms per character (8ms hold + 2ms inter-key), total emission time is ~1 second. For 500 characters, ~5 seconds. This is perceptible. To reduce latency:
  - Reduce hold time to 2ms and inter-key delay to 1ms, yielding ~3ms per character (~150ms for 50 chars, ~1.5s for 500 chars).
  - For strings longer than ~200 characters where character-by-character typing is too slow, the caller should prefer clipboard-paste instead.

### Virtual Device Setup

The uinput virtual device must declare capabilities for all keys it may emit:

- All letter keys (KEY_A through KEY_Z)
- All digit keys (KEY_0 through KEY_9)
- All punctuation keys present on a standard keyboard
- Modifier keys: KEY_LEFTSHIFT, KEY_RIGHTSHIFT, KEY_LEFTALT, KEY_RIGHTALT, KEY_LEFTCTRL, KEY_RIGHTCTRL
- KEY_SPACE, KEY_ENTER, KEY_TAB, KEY_BACKSPACE
- Any additional keys required by the active XKB layout

The device should be created once at daemon startup and kept alive for the lifetime of the daemon. Creating and destroying the device per-request adds 200-300ms of latency per insertion and may cause compositor-side issues with rapid device enumeration.

### Error Handling

**Errors the caller must handle:**

| Error | Cause | Recommended action |
|-------|-------|--------------------|
| `DeviceCreateFailed` | `/dev/uinput` not accessible (permissions) | Surface a diagnostic: "Pepper X needs input device access. Add your user to the `input` group or install the udev rule." |
| `UnmappableCharacter(char)` | Character not in active keyboard layout | Fall back to clipboard-paste for the full string. Log which character was unmappable. |
| `EmitFailed(io::Error)` | Kernel rejected the write (device destroyed, fd closed) | Attempt to recreate the virtual device. If that fails, surface an error. |
| `SocketConnectFailed` | Helper daemon not running | Attempt to start the helper, or fall back to clipboard-paste. |
| `LayoutDetectionFailed` | Could not determine active keyboard layout | Default to US layout. Log a warning. |
| `KeymapCompileFailed` | XKB keymap compilation failed for the detected layout | Default to US layout. Log a warning. |

**Race conditions and edge cases:**

- **Focus change during typing:** If the user switches windows while characters are being emitted, some characters will go to the wrong window. There is no way to prevent this with uinput -- it is a kernel-level input device with no concept of window focus. Mitigation: keep total emission time short; prefer clipboard-paste for long strings.
- **Modifier key state conflict:** If the user is physically holding a modifier key (e.g., Shift) while the daemon emits keystrokes, the injected modifier events may interfere. Mitigation: the daemon should track its own modifier state and always emit explicit modifier release events after each character, regardless of assumed state.
- **Compositor key repeat:** If the inter-event timing is too slow, the compositor may trigger key repeat on the held key. Mitigation: keep the hold time short (2-8ms), well under the typical repeat delay (500ms).
- **Layout mismatch:** If the XKB layout loaded by the helper does not match the layout active in the compositor, every character will be wrong. Mitigation: detect the layout at request time, not just at startup. Re-read the system layout when a request arrives if a layout change is suspected.

### Rust Implementation Guidance

**XKB integration:** Use the `xkbcommon` crate (Rust bindings to libxkbcommon). Key APIs:
- `xkb::Context::new()` -- create context
- `xkb::Keymap::new_from_names()` -- compile keymap from layout name
- `xkb::Keymap::key_get_syms_by_level()` -- look up keysyms for a given (keycode, group, level)
- `xkb::keysym_from_char()` or manual `xkb_utf32_to_keysym()` -- convert Unicode to keysym

**uinput integration:** Use the `evdev` crate (already a dependency via `pepperx-uinput-helper`). Key APIs:
- `VirtualDevice::builder()` -- create the virtual device
- `device.emit(&[InputEvent])` -- write events

**Socket protocol:** Keep the existing JSON-over-Unix-socket protocol from `pepperx-uinput-helper`. The request is `{"text": "..."}`, the response is `{"ok": true}` or `{"ok": false, "error": "..."}`.

### What Changes From the Current Implementation

The current `pepperx-uinput-helper` (`/home/jesse/git/pepper-x/crates/pepperx-uinput-helper/src/main.rs`) uses a hardcoded US-layout character-to-keycode table (`keystroke_for_char`). This spec replaces that with XKB-based lookup, which:

1. Supports non-US keyboard layouts without code changes.
2. Supports a broader set of characters (anything in the active layout, including AltGr characters, dead-key compositions).
3. Can report unmappable characters explicitly rather than returning a generic error.
4. Remains intentionally limited to characters available in the keyboard layout -- this is the correct behavior for a uinput backend, since uinput operates at the keycode level.

Characters outside the active layout (emoji, CJK, arbitrary Unicode) should be handled by the clipboard-paste backend, not by uinput. The insertion fallback chain already handles this: the caller should detect unmappable characters before reaching uinput and route to clipboard-paste instead.

### Comparison Summary

| Property | wtype | ydotool | dotool | This spec |
|----------|-------|---------|--------|-----------|
| Injection mechanism | Wayland protocol | uinput | uinput | uinput |
| GNOME support | No | Yes | Yes | Yes |
| Unicode support | Full (dynamic keymap) | None (ASCII only) | Layout-dependent | Layout-dependent |
| XKB awareness | Builds own keymap | None | Yes | Yes |
| Keyboard layout independence | Full | Broken for non-US | Must match system | Must match system |
| Modifier handling | Bitmask sent to compositor | Manual shift only | XKB-resolved | XKB-resolved |
| Daemon architecture | No (one-shot) | Yes (ydotoold) | No (one-shot) | Yes (persistent) |
| Dead key support | N/A (direct keysyms) | None | Yes | Yes |
| Privilege requirement | Wayland socket access | /dev/uinput | /dev/uinput | /dev/uinput |

### Open Questions

1. **Layout change detection:** Should the helper re-read the active layout on every request, or only at startup? Re-reading on every request adds a small latency cost but prevents stale-layout bugs. Recommendation: re-read on every request; the XKB compilation cost is negligible compared to the 250ms device-creation delay that is already amortized.

2. **Hybrid insertion for mixed content:** When a string contains both mappable and unmappable characters (e.g., "Hello World!" where the emoji is unmappable), should the helper type the mappable prefix and report failure at the unmappable character, or reject the entire string? Recommendation: reject the entire string and let the caller use clipboard-paste. Partial injection creates a confusing user experience.

3. **Multi-layout keyboards:** Users with multiple keyboard layouts (e.g., US + Russian) may have some characters available only in a specific group. Should the helper search all groups in the keymap? Recommendation: yes, search all groups, and if a character is found in a non-default group, emit the group-switch key (if determinable) before the character. However, this is complex and may not be needed for V1. For V1, search only the default group and fall back to clipboard-paste for characters in other groups.
