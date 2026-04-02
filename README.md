# Pepper X

GNOME-first local dictation for Linux. Hold a key combo, speak, release — your words appear in the focused app. Everything runs locally, no cloud.

## What it does

- **Hold Alt+Super** (configurable) to record, release to stop
- **Streaming transcription** via Nemotron 0.6B — text is ready the instant you stop talking
- **LLM cleanup** via Qwen 3.5 — fixes filler words, punctuation, capitalization, self-corrections
- **Text insertion** via uinput virtual keyboard — types directly into any focused app
- **Window OCR context** — captures screen text to help the cleanup model disambiguate names and terms
- **Speaker diarization** — filters out other voices (experimental)

## Performance

On an Intel Core Ultra 7 155U (no GPU):

```
record=3.2s  transcribe=0.0s  cleanup=0.5s  insert=0.2s  total=0.7s
```

Transcription happens during recording (streaming). Cleanup uses a pre-warmed KV cache.

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
  pkg-config tesseract-ocr

# Fedora
sudo dnf install \
  cargo cmake gcc gcc-c++ \
  at-spi2-core-devel glib2-devel gobject-introspection-devel \
  gtk4-devel libadwaita-devel libxkbcommon-devel vulkan-loader-devel \
  pkgconf-pkg-config tesseract
```

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

### Settings

The app window is organized into sections:

- **Recording** — Shortcut recorders (hold-to-record + toggle-to-record), mic picker, sound effects, speaker filtering, test dictation
- **Cleanup** — Enable/disable, window context toggle, prompt profile, custom prompt editor
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
- **`pepperx-uinput-helper`** — Persistent subprocess with XKB-aware virtual keyboard for text injection
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
| `pepperx-uinput-helper` | XKB-aware keystroke injection |

## Tests

```sh
cargo test --workspace
```

## License

See individual crate licenses.
