# Pepper X

Local dictation for GNOME on Linux. Hold a shortcut, speak, release. Or load a recording you already made. The text lands in the focused app. Audio and models stay on the machine.

Pepper X started from a strong base: a GNOME app, a local ASR engine, and a small cleanup model. This tree keeps that shape and retunes it for an NVIDIA GPU. The configuration below is the one that currently feels immediate in daily use.

## Recommended setup

Use this if you have an NVIDIA GPU and about 8 GB of VRAM.

1. **ASR:** `fluid-speech-pro` or `parakeet-ultra`. Both are Parakeet TDT models in ONNX. They run as one pass on the finished recording, on the ONNX Runtime CUDA provider.
2. **Cleanup:** `Qwen3.5-4B-Q4_K_M`, with **Use GPU for cleanup** on and all layers offloaded. 2B is lighter. 9B is slower and not worth it for short dictation. 0.8B is the small CPU-era default and is easy to outgrow.
3. Leave the TDT model selected. The app loads it at startup and keeps the session for the life of the process. The next dictation reuses it.
4. Quit from the panel menu (**Quit Pepper X**) or with `gapplication action com.obra.PepperX quit`. Do not suspend the process with Ctrl-Z: it holds an AT-SPI keyboard grab and the session input freezes until the process exits cleanly.

Nemotron remains available. It streams audio while you hold the key. The overlay still waits until release, then cleanup, before it inserts text. On the machine below, a cached TDT model felt faster than that streaming path, and the transcript quality was better. Prefer Nemotron only if you need its multilingual language picker. Fluid Speech Pro and Parakeet Ultra choose the language from the audio and hide that control.

`PEPPERX_ASR_EP=cpu` forces the TDT models onto CPU so you can compare. The default tries CUDA, then falls back to CPU if the CUDA session does not load.

## What it does

- **Hold Alt+Super** (configurable) to record. Release to stop. There is also a toggle shortcut.
- **Transcription** after release for Fluid Speech Pro and Parakeet Ultra. Nemotron streams during the hold, then flushes on release.
- **Cleanup** with Qwen 3.5: fillers, punctuation, capitalization, self-corrections. The prompt closes the Qwen think block so the model does not answer with `Thinking Process:`.
- **Insertion** into the focused widget: AT-SPI first, then a clipboard paste, then an XKB-aware uinput helper for apps that ignore accessibility.
- **Widget context** for cleanup: the accessible text around the caret, not a screenshot.
- **Optional speaker filter** (experimental).
- **Audio files** from disk. The upstream app is built around the live shortcut. This tree also transcribes a file you bring from a dictaphone or another recorder, including long notes, then runs the same cleanup.

## Audio files

Open **History** and choose **Load audio file**. The picker accepts WAV, MP3, FLAC, and Ogg. The run shows up under **Audio files**, separate from live **Recordings**. The app writes a Markdown transcript next to the audio file: `notes.wav` becomes `notes.md` in the same folder. The file has the source path, the model, the raw transcript, and the cleaned text when cleanup is on.

The same transcription exists without the window. That command prints the text and does not write the Markdown file:

```sh
pepper-x --transcribe-wav-and-cleanup recording.wav
```

Fluid Speech Pro and Parakeet Ultra transcribe the whole file in one CUDA pass, then Qwen cleans it. That is the useful case for a long dictaphone note: the encoder dominates, the session is already cached, and you get a clean transcript without sitting through the recording again. Live holds still feel short. A multi-minute file is where the GPU path saves the most time.

## Performance

Older notes in this file described Nemotron on CPU, for example `transcribe=0.0s` because streaming overlapped the recording, then about half a second of cleanup. That no longer describes the recommended path.

These figures are from one RTX 3060, Qwen 3.5 4B Q4_K_M at 100 GPU layers, and a TDT model already cached on the ONNX CUDA provider. They are the cleanup step only, logged after the transcript exists. They are not a benchmark suite.

| Dictation length | Cleanup | Model |
| --- | --- | --- |
| 18–35 characters | 140–210 ms | Fluid Speech Pro, cached CUDA |
| ~60–80 characters | 230–360 ms | Fluid or Parakeet Ultra, cached CUDA |
| ~120–150 characters | 460–590 ms | Fluid Speech Pro, cached CUDA |
| 243 characters | 822 ms | Parakeet Ultra, cached CUDA |
| 455 characters | 1.5 s | Parakeet Ultra, cached CUDA |

The cleanup prompt prefill is about 230–250 ms at the start of a hold, on a model that is already resident (about 3.3 GiB of VRAM for the 4B). Short answers often change the text by only a character or two.

Memory, same session:

- **Qwen 3.5 4B** stays on the GPU. `nvidia-smi` shows it on `pepperx-cleanup-helper`, about 3.3 GiB.
- **Fluid Speech Pro** stays in the `pepper-x` process, about 2.6 GiB of RAM. Persistent VRAM for that process was about 20 MiB. CUDA still runs the forward pass; the weights are not parked in VRAM between dictations. Parakeet Ultra behaves the same way.
- The first load logs `ASR execution provider: cuda` and `ASR model cached (cuda)`. Later holds log `ASR reusing cached model (cuda)`.

## Models

Launch the app, open **Models**, and download anything missing. The catalog ids are:

| Role | Id | Notes |
| --- | --- | --- |
| ASR | `fluid-speech-pro` | TDT ONNX, ~2.6 GB encoder, batch, CUDA when the provider loads |
| ASR | `parakeet-ultra` | TDT ONNX, ~2.4 GB, same runtime path as Fluid |
| ASR | Nemotron 0.6B int8 | ~850 MB, streaming, CPU in this build |
| ASR | Nemotron 3.5 multilingual int8 | Streaming, language dropdown (`fr-FR` default, or `auto`) |
| Cleanup | `qwen3.5-4b-q4_k_m.gguf` | Recommended with GPU layers |
| Cleanup | `qwen3.5-2b-q4_k_m.gguf` | ~1.3 GB, faster, lighter |
| Cleanup | `qwen3.5-9b-q4_k_m.gguf` | ~5.3 GB, slower on short dictation |
| Cleanup | `qwen3.5-0.8b-q4_k_m.gguf` | ~500 MB |

## Install

Ubuntu 25.04+ or Fedora 42+. GNOME 48+ on Wayland. Rust 1.92 or newer (`gtk4-rs` 0.11). Distro `cargo` is often older; use [rustup](https://rustup.rs) if `rustc --version` is below 1.92.

```sh
# Ubuntu
sudo apt install \
  build-essential cmake clang libclang-dev \
  libadwaita-1-dev libatspi2.0-dev \
  libglib2.0-dev libgtk-4-dev \
  libpipewire-0.3-dev libxkbcommon-dev \
  pkg-config \
  wl-clipboard xclip

# Fedora
sudo dnf install \
  cmake gcc gcc-c++ clang clang-devel \
  at-spi2-core-devel glib2-devel \
  gtk4-devel libadwaita-devel libxkbcommon-devel pipewire-devel \
  pkgconf-pkg-config \
  wl-clipboard xclip
```

`libclang` is required by `llama-cpp-sys-4`. `wl-clipboard` or `xclip` lets the uinput helper paste characters the current layout cannot type.

Your user must be in the `input` group, and uinput needs a udev rule. Log out after the group change.

```sh
sudo usermod -aG input $USER
echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | sudo tee /etc/udev/rules.d/99-pepper-x-uinput.rules
echo 'SUBSYSTEM=="input", ATTRS{name}=="Pepper X virtual keyboard", ENV{ID_INPUT_KEYBOARD}="1"' | sudo tee /etc/udev/rules.d/99-pepper-x-keyboard.rules
sudo udevadm control --reload-rules
```

### GPU build

Cleanup uses llama.cpp CUDA in `pepperx-cleanup-helper`. ASR CUDA uses the ONNX Runtime CUDA provider inside `pepper-x` (`parakeet-rs` feature `cuda`). Both are on in the default `Cargo.toml`. You need a working `nvidia-smi` and, for the cleanup helper, a CUDA toolkit (`nvcc`, `libcudart`). If the toolkit is not in `/usr/local/cuda`:

```sh
CUDA_HOME=/opt/cuda cargo build --release
```

CPU-only cleanup: remove `"cuda"` from `llama-cpp-4` and `llama-cpp-sys-4` in `crates/pepperx-cleanup-helper/Cargo.toml`. CPU-only ASR: remove `"cuda"` from `parakeet-rs` in `crates/pepperx-asr/Cargo.toml`. Vulkan for AMD or Intel cleanup is not on by default; see the comment in the cleanup-helper manifest.

### Build and install

```sh
cargo build --release
sudo install -m 755 target/release/pepper-x /usr/local/bin/
sudo mkdir -p /usr/libexec/pepper-x
sudo install -m 755 target/release/pepperx-uinput-helper /usr/libexec/pepper-x/
sudo install -m 755 target/release/pepperx-cleanup-helper /usr/libexec/pepper-x/
bash scripts/dev-install-extension.sh
```

The app also looks for the helpers next to the `pepper-x` binary, so a copy under `~/.local/bin` works without root if that directory is on `PATH`.

Log out and back in once so GNOME Shell loads the extension. Shell keeps extension JavaScript in memory; `disable` / `enable` does not always reload it. The panel menu is **Open Pepper X Settings**, **Open Pepper X History**, and **Quit Pepper X**.

## Usage

```sh
pepper-x
```

On startup the app starts the extension, prewarms the Qwen helper, and preloads the selected TDT model when that directory is a Parakeet layout. Default shortcut: hold Alt+Super.

Only errors go to the terminal unless you ask for more:

```sh
PEPPERX_VERBOSE=1 pepper-x
```

That adds a per-dictation `perf:` line, streaming ASR timing, cleanup prefill, and uinput layout detection. The CUDA lines (`ASR execution provider`, `ASR reusing cached model`) are printed either way.

### Text insertion

1. **AT-SPI**, when the focused widget exposes editable text.
2. **Clipboard paste**, when a clipboard tool exists.
3. **`pepperx-uinput-helper`**, for terminals, Wine, and other apps that ignore accessibility.

The helper reads the active XKB layout (not a hard-coded US map): `PEPPERX_XKB_LAYOUT` / `PEPPERX_XKB_VARIANT` if set, otherwise GNOME `mru-sources`, then `setxkbmap -query`, then `/etc/default/keyboard`. It types chords, including dead keys, when every character exists on that layout. If any character does not, it pastes the whole string with `wl-copy`, `xclip`, or `xsel`, and restores the previous clipboard when it can. Some terminals want Ctrl+Shift+V and will ignore Ctrl+V.

### Settings

- **Recording** — hold shortcut, toggle shortcut, microphone, sounds, speaker filter, test dictation.
- **Cleanup** — on/off, GPU layers, widget context, prompt profile, custom prompt.
- **Corrections** — preferred spellings and mishear replacements.
- **Models** — ASR and cleanup choice, downloads, language dropdown for multilingual Nemotron only.
- **History** — replay a recording with other models, edit the prompt, diff, playback.
- **General** — launch at login.
- **Diagnostics** — runtime status.

### CLI

```sh
pepper-x --transcribe-wav recording.wav
pepper-x --transcribe-wav-and-cleanup recording.wav
pepper-x --rerun-archived-run <run-id>
```

## Architecture

- **`pepper-x`** — GTK4 / libadwaita. Recording, settings, history, and the ONNX ASR session.
- **`pepperx-cleanup-helper`** — long-lived llama.cpp process for Qwen. It is separate so ONNX Runtime and llama.cpp do not share one process.
- **`pepperx-uinput-helper`** — long-lived virtual keyboard.
- **`pepperx@obra`** — GNOME Shell extension: panel menu, status pill, D-Bus.

| Crate | Role |
| --- | --- |
| `pepperx-asr` | Nemotron streaming, Parakeet TDT batch, CUDA session cache |
| `pepperx-cleanup` | Prompt assembly and the helper protocol |
| `pepperx-cleanup-helper` | Qwen inference |
| `pepperx-audio` | PipeWire capture |
| `pepperx-corrections` | Replacement lists |
| `pepperx-models` | Catalog, download, readiness |
| `pepperx-platform-gnome` | evdev shortcut, AT-SPI insert and context |
| `pepperx-ipc` | D-Bus for the extension |
| `pepperx-uinput-helper` | Layout-aware typing and clipboard fallback |

## Tests

```sh
cargo test --workspace
```

## License

See the license files in each crate.
