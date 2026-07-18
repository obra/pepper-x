# Pepper X

GNOME-first local dictation for Linux. Hold a key combo, speak, release — your words appear in the focused app. Everything runs locally, no cloud.

## What it does

- **Hold Alt+Super** (configurable) to record, release to stop
- **Streaming transcription** via Nemotron 0.6B — text is ready the instant you stop talking
- **LLM cleanup** via Qwen 3.5 — fixes filler words, punctuation, capitalization, self-corrections
- **Text insertion** — AT-SPI first when possible; XKB-aware uinput fallback for hostile apps (Wine, some terminals, custom UIs)
- **Window OCR context** — captures screen text to help the cleanup model disambiguate names and terms
- **Speaker diarization** — filters out other voices (experimental)

## Performance

On an Intel Core Ultra 7 155U (CPU only):

```
record=3.2s  transcribe=0.0s  cleanup=0.5s  insert=0.2s  total=0.7s
```

Transcription happens during recording (streaming). Cleanup uses a pre-warmed KV cache.

With an NVIDIA GPU and CUDA enabled, cleanup is typically ~0.1s instead of several seconds on some CPUs.

## Install

### Prerequisites

Ubuntu 25.04+ or Fedora 42+. GNOME 48+ on Wayland.

```sh
# Ubuntu
sudo apt install \
  build-essential cargo cmake \
  libadwaita-1-dev libatspi2.0-dev libgirepository1.0-dev \
  libglib2.0-dev libgtk-4-dev libgtk4-layer-shell-dev \
  libvulkan-dev libxkbcommon-dev \
  pkg-config tesseract-ocr \
  wl-clipboard xclip

# Fedora
sudo dnf install \
  cargo cmake gcc gcc-c++ \
  at-spi2-core-devel glib2-devel gobject-introspection-devel \
  gtk4-devel libadwaita-devel libxkbcommon-devel vulkan-loader-devel \
  pkgconf-pkg-config tesseract \
  wl-clipboard xclip
```

`wl-clipboard` (`wl-copy`) and/or `xclip` (or `xsel`) are recommended so the uinput helper can paste glyphs that the active keyboard layout cannot type (accents on plain US QWERTY, CJK, Thai, etc.).

#### GPU acceleration for cleanup (optional, NVIDIA)

`pepperx-cleanup-helper` is built with optional GPU acceleration. CUDA support by default. This offloads the Qwen cleanup model to an NVIDIA GPU and makes cleanup much faster.

**System dependencies:**

1. **NVIDIA driver** — usually already installed if `nvidia-smi` works.
2. **CUDA toolkit** — required at build time (provides `nvcc` and `libcudart`).

```sh
# Verify the driver
nvidia-smi

# Verify the toolkit (after install)
nvcc --version
ls /usr/local/cuda/lib64/libcudart.so
```

Install the CUDA toolkit from [NVIDIA's CUDA downloads](https://developer.nvidia.com/cuda-downloads) (Linux → your distro → run the installer or repo packages). The default install path is `/usr/local/cuda`. If you use a different path, set `CUDA_HOME` when building:

```sh
CUDA_HOME=/opt/cuda cargo build --release
```

**CPU-only build:** if you do not have CUDA installed, remove `"cuda"` from the `features` list in `crates/pepperx-cleanup-helper/Cargo.toml` (both `llama-cpp-4` and `llama-cpp-sys-4`).

**Vulkan (AMD/Intel):** not enabled by default — the build needs `glslc` (shaderc / Vulkan SDK). See the comment in `crates/pepperx-cleanup-helper/Cargo.toml` to re-enable it.

**Runtime:** in the app, open **Cleanup** → enable **Use GPU for cleanup**. Adjust **GPU layers** if needed (default offloads all layers).

Your user must be in the `input` group for hotkey capture and text injection:

```sh
sudo usermod -aG input $USER
# Log out and back in
```

A udev rule is needed for the virtual keyboard:

```sh
echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | sudo tee /etc/udev/rules.d/99-pepper-x-uinput.rules
echo 'SUBSYSTEM=="input", ATTRS{name}=="Pepper X virtual keyboard", ENV{ID_INPUT_KEYBOARD}="1"' | sudo tee /etc/udev/rules.d/99-pepper-x-keyboard.rules
sudo udevadm control --reload-rules
```

### Build and install

```sh
cargo build --release
sudo install -m 755 target/release/pepper-x /usr/local/bin/
sudo mkdir -p /usr/libexec/pepper-x
sudo install -m 755 target/release/pepperx-uinput-helper /usr/libexec/pepper-x/
sudo install -m 755 target/release/pepperx-cleanup-helper /usr/libexec/pepper-x/
bash scripts/dev-install-extension.sh
```

Log out and back in for the GNOME extension to load.

### Download models

Launch the app, go to the **Models** section, and click **Download Missing Models**. Or download manually:

- **ASR**: Nemotron 0.6B int8 (~850MB) — downloaded from HuggingFace on first run
- **Cleanup**: Qwen 3.5 0.8B Q4_K_M (~500MB) or 2B Q4_K_M (~1.3GB)

## Usage

```sh
pepper-x
```

That's it. The app:
1. Starts the GNOME Shell extension (tray icon + status pill)
2. Pre-warms the cleanup model in the background
3. Listens for your trigger keys (Alt+Super by default)

### Text insertion -multilingual support with dynamic XKB + smart clipboard fallback-

Pepper X inserts the final transcript into the focused app using a fallback chain:

1. **AT-SPI** — semantic editable-text / key-string when the app exposes accessibility
2. **Clipboard paste** (platform path) — when available
3. **`pepperx-uinput-helper`** — last resort for apps that ignore accessibility (some terminals, Wine, canvas UIs)

The uinput helper does **not** assume a fixed US keymap. On every insert it:

1. **Detects the currently active layout** (what Super+Space selected), not only the first GNOME source:
   - `PEPPERX_XKB_LAYOUT` / `PEPPERX_XKB_VARIANT` if set (override)
   - GNOME `mru-sources[0]`, else `sources[current]`
   - `setxkbmap -query`, then `/etc/default/keyboard`, then default `us`
2. **Builds an XKB reverse map** for that layout: direct chords (including Shift/AltGr) plus **dead-key** sequences (e.g. `^` + `e` → `ê` on French AZERTY / Mac variants).
3. **Types chords** when every character is on the layout.
4. **Pastes the whole string** (clipboard + Ctrl+V) when **any** character is missing — e.g. French accents on plain `us` (no dead keys), or Chinese/Japanese/Thai/etc. Ctrl+Shift+U via uinput is avoided as primary path because sticky modifiers produce garbage.
5. **Unicode hex entry** only if no clipboard tool is installed.

Clipboard tools tried in order: `wl-copy`, `xclip`, `xsel`. Previous clipboard contents are restored after paste when possible.

**Limits:** apps that block paste or use a non-Ctrl+V paste binding (many terminals want Ctrl+Shift+V) may still fail on unmappable glyphs; install a clipboard tool for best results when switching between layouts (AZERTY ↔ QWERTY) mid-session.

### Settings

The app window is organized into sections:

- **Recording** — Shortcut recorders (hold-to-record + toggle-to-record), mic picker, sound effects, speaker filtering, test dictation
- **Cleanup** — Enable/disable, GPU offload toggle, window context toggle, prompt profile, custom prompt editor
- **Corrections** — Editable preferred transcriptions and commonly misheard replacements
- **Models** — ASR and cleanup model selection with download progress
- **History** — Transcription lab with per-stage model pickers, inline prompt editor, word-level diff, audio playback, diarization timeline
- **General** — Launch at login
- **Diagnostics** — Runtime status

### CLI

```sh
# Transcribe a WAV file
pepper-x --transcribe-wav recording.wav

# Transcribe + cleanup
pepper-x --transcribe-wav-and-cleanup recording.wav

# Rerun an archived recording
pepper-x --rerun-archived-run <run-id>
```

## Architecture

- **`pepper-x`** — GTK4/libadwaita app, owns the recording pipeline, settings, history
- **`pepperx-cleanup-helper`** — Persistent subprocess running llama.cpp (llama-cpp-4) for Qwen 3.5 inference, isolated to avoid ONNX Runtime symbol collision with the ASR engine
- **`pepperx-uinput-helper`** — Persistent uinput daemon: active-layout XKB reverse map, dead keys, clipboard paste for off-layout Unicode
- **`pepperx@obra` GNOME extension** — Tray icon, floating status pill overlay, D-Bus bridge

### Key crates

| Crate | Purpose |
|-------|---------|
| `pepperx-asr` | Streaming ASR via parakeet-rs (Nemotron 0.6B) |
| `pepperx-cleanup` | Cleanup prompt assembly, subprocess communication |
| `pepperx-cleanup-helper` | llama-cpp-4 inference (Qwen 3.5) |
| `pepperx-audio` | PipeWire recording with streaming chunk delivery |
| `pepperx-corrections` | Preferred transcriptions and misheard replacements store |
| `pepperx-models` | Model catalog, download, readiness checking |
| `pepperx-platform-gnome` | evdev modifier capture, AT-SPI text insertion, OCR context |
| `pepperx-ipc` | D-Bus service for extension communication |
| `pepperx-uinput-helper` | Active-layout XKB chords + clipboard Unicode fallback |

## Tests

```sh
cargo test --workspace
```

## License

See individual crate licenses.
