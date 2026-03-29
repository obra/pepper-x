# Pepper X

Pepper X is a GNOME-first local Linux dictation utility inspired by Ghost Pepper.

This repo starts from the approved design and implementation plan:

- [Pepper X design](docs/superpowers/specs/2026-03-27-pepper-x-design.md)
- [Pepper X shell and GNOME integration plan](docs/superpowers/plans/2026-03-27-pepper-x-shell-and-gnome-integration.md)
- [Pepper X GNOME 48 recovery plan](docs/superpowers/plans/2026-03-28-pepper-x-gnome48-recovery.md)

Current direction:

- brand new app with zero shared code from Ghost Pepper
- GNOME 48+ baseline, Wayland-only for V1
- Rust + GTK4/libadwaita app
- app-first ownership of product logic
- thin GNOME Shell extension for shell-facing integration
- unsandboxed `.deb` and `.rpm` distribution
- fully local runtime, no cloud dependencies

The first execution phase is repo bootstrap, app shell, GNOME extension scaffold, IPC, and modifier-only hold-to-talk signaling. Modifier-only capture is not assumed to be extension-only.

## Local prerequisites

Pepper X V1 targets GNOME 48+ on Wayland. The practical distro floor for this path is Ubuntu 25.04+ and Fedora 42+. The workspace is unsandboxed and currently bootstraps the Rust app shell plus GNOME integration scaffolding.

The current GTK/libadwaita dependency set requires Rust 1.92 or newer.

### Fedora

Install the toolchain and native development packages:

```sh
sudo dnf install \
  at-spi2-core-devel \
  cargo \
  gcc \
  glib2-devel \
  gobject-introspection-devel \
  gtk4-devel \
  libadwaita-devel \
  pkgconf-pkg-config
```

### Ubuntu

Install the toolchain and native development packages:

```sh
sudo apt install \
  build-essential \
  cargo \
  libadwaita-1-dev \
  libatspi2.0-dev \
  libgirepository1.0-dev \
  libglib2.0-dev \
  libgtk-4-dev \
  pkg-config
```

## Verification

Run the current automated checks from the repository root:

```sh
cargo fmt --check
cargo test --workspace
cargo check --workspace
bash tests/smoke/test_extension_ipc.sh
./scripts/smoke-hotkey.sh
python3 -m pytest packaging/tests -q
```

For live GNOME 48+ modifier capture validation on a real Wayland session, run:

```sh
./scripts/gnome48-smoke-hotkey.sh
```

Run that helper inside the live GNOME session, or export that session's `DBUS_SESSION_BUS_ADDRESS` first. It only proves the live-session prerequisites. The final press/release check still needs a physical keyboard on the live GNOME session because QEMU `send-key`, VNC, and noVNC injection were not authoritative for the app-owned AT-SPI watcher path.

## Loop 1 Prerecorded ASR

Loop 1 uses a real local `sherpa-onnx` Parakeet bundle, with prerecorded WAV input as the only simplification. Point `PEPPERX_PARAKEET_MODEL_DIR` at the extracted `nemo-parakeet-tdt-0.6b-v2-int8` model directory before running the ASR checks.

The stable non-GUI entrypoint inside the Pepper X app binary is:

```sh
cargo run -p pepper-x-app -- --transcribe-wav tests/fixtures/loop1-hello.wav
```

It reuses the same supported Rust, GTK4, and libadwaita environment as the GUI shell. Loop 1 does not split this into a separate ASR-only binary.

To run the loop-1 verification path with a caller-owned state root:

```sh
export PEPPERX_STATE_ROOT="$(mktemp -d)"
PEPPERX_PARAKEET_MODEL_DIR=/path/to/model cargo test -p pepperx-asr transcriber_real_ -- --ignored --nocapture
PEPPERX_PARAKEET_MODEL_DIR=/path/to/model PEPPERX_STATE_ROOT="$PEPPERX_STATE_ROOT" tests/smoke/test_prerecorded_asr.sh
PEPPERX_STATE_ROOT="$PEPPERX_STATE_ROOT" cargo run -p pepper-x-app
```

## Loop 2 Friendly Insertion

Loop 2 keeps the loop-1 prerecorded WAV path and adds one narrow insertion backend:

- GNOME 48+ Wayland session only
- GNOME Text Editor only
- semantic `EditableText` insertion only
- no broader accessible-app support yet
- no clipboard, string-injection, or `uinput` fallback yet

The loop-2 dev entrypoint is:

```sh
PEPPERX_PARAKEET_MODEL_DIR=/path/to/model \
cargo run -p pepper-x-app -- --transcribe-wav-and-insert-friendly tests/fixtures/loop1-hello.wav
```

For the live-session insertion smoke, start a GNOME Text Editor document, focus the caret where you want the transcript inserted, and run:

```sh
./scripts/smoke-insert-friendly.sh
```

Run that helper inside the live GNOME 48+ Wayland session, or export that session's `DBUS_SESSION_BUS_ADDRESS`, `XDG_RUNTIME_DIR`, and `XDG_SESSION_TYPE=wayland` first. The helper is only meant for a real in-session GNOME client; SSH into the VM is not an authoritative AT-SPI insertion surface.

## Loop 3 Common Accessible Insertion

Loop 3 broadens the semantic AT-SPI insertion path to a small declared set of accessible target classes:

- `text-editor`
- `browser-textarea`

This loop is still semantic insertion only. Pepper X does not promise clipboard fallback, AT-SPI string injection, or `uinput` behavior here yet.

Use the accessible-target smoke helper inside a live GNOME 48+ Wayland session:

```sh
./scripts/smoke-insert-accessible.sh text-editor
./scripts/smoke-insert-accessible.sh browser-textarea
```

For the browser smoke, focus a normal textarea or contenteditable field in Firefox before running the helper.

## Loop 4 Fallback-Backed Insertion

Loop 4 keeps the loop-3 text-oriented path first, but declares the full fallback order Pepper X now uses for insertion:

1. semantic `EditableText` insertion
2. AT-SPI string injection
3. clipboard-mediated paste
4. Pepper X-owned `uinput` text injection

The declared loop-4 target classes are:

- `text-editor`
- `browser-textarea`
- `terminal`
- `hostile`

Current boundaries:

- `terminal` targets only use AT-SPI string injection when the focused surface plausibly accepts text
- clipboard mediation preserves and restores clipboard ownership instead of treating paste as destructive
- the `uinput` path is a last fallback, never the default
- the helper is Pepper X-owned and text-only, installed at `/usr/libexec/pepper-x/pepperx-uinput-helper`
- Pepper X still does not claim secure-field support or universal coverage across arbitrary Linux apps

Use the terminal smoke helper inside a live GNOME 48+ Wayland session:

```sh
./scripts/smoke-insert-terminal.sh
```

Run that helper inside the live GNOME session, or export that session's `DBUS_SESSION_BUS_ADDRESS`, `XDG_RUNTIME_DIR`, and `XDG_SESSION_TYPE=wayland` first. As with the accessible-target smokes, SSH into the VM is not an authoritative AT-SPI insertion surface.

## Loop 5 Cleanup

Loop 5 adds a real local cleanup backend on top of the existing ASR and insertion path:

- `llama.cpp` cleanup with `PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf`
- raw ASR transcript archived as `transcript_text`
- cleaned transcript archived separately under `cleanup.cleaned_text`
- cleanup diagnostics that preserve backend/model metadata and `used_ocr`
- optional OCR text treated as bounded supporting context, not a separate mode

Useful loop-5 entrypoints:

```sh
PEPPERX_PARAKEET_MODEL_DIR=/path/to/parakeet \
PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf \
cargo run -p pepper-x-app -- --transcribe-wav-and-cleanup tests/fixtures/loop1-hello.wav
```

```sh
PEPPERX_PARAKEET_MODEL_DIR=/path/to/parakeet \
PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf \
cargo run -p pepper-x-app -- --transcribe-wav-and-cleanup-and-insert-friendly tests/fixtures/loop1-hello.wav
```

For the live cleaned-insertion smoke, focus a GNOME Text Editor document and run:

```sh
PEPPERX_PARAKEET_MODEL_DIR=/path/to/parakeet \
PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf \
./scripts/smoke-insert-cleaned-friendly.sh
```

That helper verifies three things together on a real GNOME Wayland session:

- the archived raw transcript remains distinct from the cleaned transcript
- the cleanup CLI stdout matches the archived cleaned transcript
- the focused Text Editor buffer contains the cleaned transcript after the app insertion path runs

## Loop 6 Archived Reruns

Loop 6 adds archived-run reruns without mutating the original run:

- rerun one archived recording by run ID
- keep the parent run intact
- archive the rerun as a new child linked by `parent_run_id`
- override the cleanup prompt profile now, and optionally override ASR and cleanup models when more than one supported model is installed

The headless rerun entrypoint is:

```sh
PEPPERX_PARAKEET_MODEL_DIR=/path/to/parakeet \
PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf \
cargo run -p pepper-x-app -- --rerun-archived-run <run-id> \
  --cleanup-prompt-profile literal-dictation
```

If additional supported models are installed, you can also pass `--asr-model <model-id>` and `--cleanup-model <model-id>`.

To run the rerun smoke against a caller-owned state root:

```sh
export PEPPERX_STATE_ROOT="$(mktemp -d)"
PEPPERX_PARAKEET_MODEL_DIR=/path/to/parakeet \
PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf \
tests/smoke/test_rerun_pipeline.sh
```

That smoke proves the rerun uses the archived parent WAV, archives a new child run, preserves the parent metadata, and records the rerun prompt override on the child.

## Diagnostics Surface

Pepper X now exposes a dedicated Diagnostics page in the GTK shell. It is a first-pass runtime surface, not a generic log blob.

Current diagnostics coverage:

- selected ASR model, cleanup model, and cleanup prompt profile
- model cache root plus per-model install paths and readiness
- extension connectivity, modifier-capture support, and service version
- latest-run ASR and cleanup timings
- latest-run insertion backend, insertion target, OCR usage, and failure reasons

The history browser remains the place to inspect one archived run or one parent/rerun comparison in detail. The Diagnostics page is the current-session overview.

## Package Install Verification

Pepper X still ships as an unsandboxed native-app beta. Package verification is script-first at this stage:

```sh
python3 -m pytest packaging/tests -q
tests/smoke/test_packaging_install.sh
./scripts/verify-extension-install.sh
```

Those checks cover:

- Debian and RPM runtime dependency metadata
- packaged binary, helper, desktop, and autostart asset coverage
- desktop/autostart metadata consistency
- packaged GNOME extension asset validation through the same extension install checker used for dev installs

## Startup, Upgrade, and Uninstall Verification

Pepper X distinguishes two packaged launch modes:

- interactive launch from the desktop entry opens the normal UI
- session autostart keeps the runtime alive without forcing the window open

Use the release checklist and verification helpers when producing beta artifacts:

```sh
./scripts/verify-upgrade-ubuntu.sh <old.deb> <new.deb>
./scripts/verify-upgrade-fedora.sh <old.rpm> <new.rpm>
./scripts/verify-uninstall-cleanup.sh
```

The release-process checklist lives in:

- `docs/release/pepper-x-v1.md`
