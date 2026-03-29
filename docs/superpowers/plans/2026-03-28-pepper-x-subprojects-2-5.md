# Pepper X Subprojects 2-5 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish Pepper X's remaining V1 product work after the shell/GNOME foundation by adding live audio/runtime behavior, cleanup/OCR/corrections, first-class history and reruns, and real packaging/operational polish.

**Architecture:** Keep the app as the product coordinator and reuse the existing loop-1 through loop-5 seams instead of replacing them. Add focused runtime crates only where the boundary is real and testable: live audio capture, model catalog/cache management, OCR acquisition, and archive/history browsing. The GNOME platform crate stays thin and GNOME-facing; model/runtime/history ownership remains in the app and focused Rust crates.

**Tech Stack:** Rust, Cargo workspace, GTK4, libadwaita, GStreamer-backed live audio capture, `sherpa-onnx`, Parakeet NeMo models, `llama.cpp`, GNOME Shell screenshot D-Bus, local OCR, AT-SPI, D-Bus, `.deb`/`.rpm` packaging

---

## File Structure

**Planning docs to keep current:**
- `docs/superpowers/specs/2026-03-27-pepper-x-design.md`
  - Product contract for parity and subsystem boundaries.
- `docs/superpowers/plans/2026-03-28-pepper-x-iterative-ai-loops.md`
  - Existing vertical-slice roadmap that produced the current partial AI stack.
- `docs/superpowers/plans/2026-03-28-pepper-x-subprojects-2-5.md`
  - Master execution plan for the remaining product work.
- `docs/architecture/text-insertion-strategy.md`
  - Long-term insertion backend order and target-class expectations.
- `docs/architecture/gnome-integration.md`
  - GNOME-facing shell/runtime seam.

**New runtime crates:**
- `crates/pepperx-audio/Cargo.toml`
  - Live microphone/device handling dependencies and features.
- `crates/pepperx-audio/src/lib.rs`
  - Public live-capture API exported to the app.
- `crates/pepperx-audio/src/devices.rs`
  - Microphone enumeration and selected-device metadata.
- `crates/pepperx-audio/src/recording.rs`
  - Start/stop recording session core that writes deterministic WAV output for ASR handoff.
- `crates/pepperx-models/Cargo.toml`
  - Shared model catalog/cache/download management for ASR and cleanup.
- `crates/pepperx-models/src/lib.rs`
  - Public catalog/cache API.
- `crates/pepperx-models/src/catalog.rs`
  - Built-in model catalog entries and metadata.
- `crates/pepperx-models/src/cache.rs`
  - Cache root discovery, installed model inventory, readiness status.
- `crates/pepperx-models/src/download.rs`
  - Download/bootstrap support for supported model bundles.
- `crates/pepperx-ocr/Cargo.toml`
  - GNOME screenshot + local OCR dependencies.
- `crates/pepperx-ocr/src/lib.rs`
  - Public OCR/context API.
- `crates/pepperx-ocr/src/gnome_screenshot.rs`
  - Frontmost-window or focused-screen capture via GNOME-friendly D-Bus.
- `crates/pepperx-ocr/src/recognize.rs`
  - OCR extraction and bounded context shaping.
- `crates/pepperx-corrections/Cargo.toml`
  - Persistent deterministic correction store.
- `crates/pepperx-corrections/src/lib.rs`
  - Public correction and learning API.
- `crates/pepperx-corrections/src/store.rs`
  - On-disk preferred transcription and replacement storage.
- `crates/pepperx-corrections/src/learning.rs`
  - Conservative post-paste learning rules.

**App-owned orchestration and UI surfaces:**
- `app/Cargo.toml`
  - Add new runtime crates.
- `app/src/app.rs`
  - App composition root, background startup, runtime wiring.
- `app/src/main.rs`
  - CLI/GUI entrypoint routing.
- `app/src/cli.rs`
  - Headless dev flows for recording, reruns, diagnostics, and model management.
- `app/src/transcription.rs`
  - App-owned orchestration around recording, ASR, cleanup, OCR, insertion, and archival.
- `app/src/transcript_log.rs`
  - Replace loop-era JSONL assumptions with first-class archived-run records.
- `app/src/session_runtime.rs`
  - Recording session state machine and end-to-end runtime coordinator.
- `app/src/history_store.rs`
  - Query/load archived run records for UI and reruns.
- `app/src/history_view.rs`
  - Focused history browser widgets and row/view models.
- `app/src/window.rs`
  - Real settings/history/diagnostics surfaces instead of summary text only.
- `app/src/settings.rs`
  - Live-audio model/default insertion/correction settings.

**GNOME-facing integration surfaces:**
- `crates/pepperx-platform-gnome/src/service.rs`
  - Hold-to-talk IPC routing into the runtime coordinator.
- `crates/pepperx-platform-gnome/src/atspi.rs`
  - Modifier capture and focused-app insertion only.
- `gnome-extension/`
  - Thin extension remains shell-facing only.

**Packaging and operational files:**
- `README.md`
  - Current install/run/verification instructions.
- `packaging/deb/control`
  - Runtime dependencies and package metadata.
- `packaging/rpm/pepper-x.spec`
  - RPM build/install/runtime metadata.
- `packaging/deb/pepper-x.desktop`
  - Desktop launcher.
- `packaging/deb/pepper-x-autostart.desktop`
  - GNOME autostart integration.
- `packaging/tests/test_metadata.py`
  - Packaging metadata validation.
- `docs/release/pepper-x-v1.md`
  - Release checklist and upgrade process.

**Verification surfaces to add:**
- `tests/fixtures/`
  - Stable audio, OCR, and archive fixtures.
- `tests/smoke/test_live_recording.sh`
  - Live recording and ASR smoke.
- `tests/smoke/test_model_management.sh`
  - Model download/readiness smoke.
- `tests/smoke/test_ocr_cleanup.sh`
  - OCR-assisted cleanup smoke.
- `tests/smoke/test_rerun_pipeline.sh`
  - History rerun smoke.
- `tests/smoke/test_packaging_install.sh`
  - Local package install smoke for Fedora and Ubuntu.

## Chunk 1: Shared Runtime Foundations

### Task 1: Add a shared model catalog and cache layer

**Files:**
- Create: `crates/pepperx-models/Cargo.toml`
- Create: `crates/pepperx-models/src/lib.rs`
- Create: `crates/pepperx-models/src/catalog.rs`
- Create: `crates/pepperx-models/src/cache.rs`
- Create: `crates/pepperx-models/src/download.rs`
- Modify: `Cargo.toml`
- Modify: `app/Cargo.toml`
- Test: `crates/pepperx-models/src/lib.rs`

- [ ] **Step 1: Write failing model-catalog tests**

Add tests for:
- listing supported ASR and cleanup models
- deriving the default cache root from XDG state/cache dirs
- reporting readiness for a missing model vs an installed model

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepperx-models catalog_ cache_ -- --nocapture`
Expected: FAIL because the crate and APIs do not exist yet.

- [ ] **Step 3: Implement the smallest shared model catalog**

Create:
- built-in catalog structs for supported Parakeet ASR and cleanup models
- cache-root helpers
- readiness checks that validate required files on disk without downloading yet

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p pepperx-models catalog_ cache_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml app/Cargo.toml crates/pepperx-models
git commit -m "Add Pepper X model catalog and cache layer"
```

### Task 2: Promote transcript logging into a first-class archived-run store

**Files:**
- Modify: `app/src/transcript_log.rs`
- Create: `app/src/history_store.rs`
- Modify: `app/src/transcription.rs`
- Test: `app/src/transcript_log.rs`
- Test: `app/src/history_store.rs`

- [ ] **Step 1: Write failing archive-store tests**

Add tests for:
- writing one archived run as a stable JSON record under a run-specific directory
- loading runs newest-first for the history UI
- preserving raw transcript, cleanup diagnostics, insertion diagnostics, and source media metadata together

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepper-x-app transcript_archive_ history_store_ -- --nocapture`
Expected: FAIL because the archive store still assumes a flat loop-era JSONL file.

- [ ] **Step 3: Implement the smallest archive upgrade**

Keep the existing JSONL reader compatible if already needed for migration, but make new writes create first-class archived run records with stable IDs and dedicated metadata files.

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p pepper-x-app transcript_archive_ history_store_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/transcript_log.rs app/src/history_store.rs app/src/transcription.rs
git commit -m "Promote Pepper X transcript logs into archived runs"
```

## Chunk 2: Subproject 2 Audio and Transcription Runtime

### Task 3: Add live microphone enumeration and selected-device metadata

**Files:**
- Create: `crates/pepperx-audio/Cargo.toml`
- Create: `crates/pepperx-audio/src/lib.rs`
- Create: `crates/pepperx-audio/src/devices.rs`
- Modify: `Cargo.toml`
- Modify: `app/Cargo.toml`
- Modify: `app/src/settings.rs`
- Test: `crates/pepperx-audio/src/devices.rs`
- Test: `app/src/settings.rs`

- [ ] **Step 1: Write failing device-enumeration tests**

Add tests for:
- representing discovered input devices with stable IDs and display names
- persisting the preferred input device in app settings
- returning an empty but valid device list when no microphones are available

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepperx-audio device_ -- --nocapture`
Expected: FAIL because the audio crate does not exist yet.

- [ ] **Step 3: Implement the smallest live-device inventory**

Create the audio crate and a device inventory API that the app can call without starting recording. Keep it Linux-only and microphone-only.

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p pepperx-audio device_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml app/Cargo.toml app/src/settings.rs crates/pepperx-audio
git commit -m "Add Pepper X microphone inventory"
```

### Task 4: Add the recording session core and live WAV handoff

**Files:**
- Create: `crates/pepperx-audio/src/recording.rs`
- Create: `app/src/session_runtime.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/cli.rs`
- Test: `crates/pepperx-audio/src/recording.rs`
- Test: `app/src/session_runtime.rs`

- [ ] **Step 1: Write failing recording-session tests**

Add tests for:
- starting a recording session with a selected device
- stopping a recording session and materializing a WAV artifact
- rejecting duplicate starts and duplicate stops
- handing the recorded WAV path to the existing ASR pipeline

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepperx-audio recording_ -- --nocapture`
Run: `cargo test -p pepper-x-app session_runtime_ -- --nocapture`
Expected: FAIL because live recording is not implemented.

- [ ] **Step 3: Implement the smallest live recording runtime**

Record mono PCM into a deterministic temporary WAV file suitable for the existing `sherpa-onnx` path. Keep the runtime single-session and synchronous at the app boundary.

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p pepperx-audio recording_ -- --nocapture`
Run: `cargo test -p pepper-x-app session_runtime_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/cli.rs app/src/session_runtime.rs app/src/transcription.rs crates/pepperx-audio/src/recording.rs
git commit -m "Add Pepper X live recording runtime"
```

### Task 5: Route hold-to-talk and CLI recording through the runtime coordinator

**Files:**
- Modify: `app/src/app.rs`
- Modify: `app/src/cli.rs`
- Modify: `crates/pepperx-platform-gnome/src/service.rs`
- Modify: `crates/pepperx-session/src/lib.rs`
- Test: `app/src/app.rs`
- Test: `crates/pepperx-platform-gnome/src/service.rs`
- Test: `tests/smoke/test_live_recording.sh`

- [ ] **Step 1: Write failing coordinator tests**

Add tests for:
- `StartRecording` beginning a live session instead of only updating session state
- `StopRecording` producing an archived ASR run
- a headless dev flow such as `pepper-x --record-and-transcribe`

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepper-x-app app_shell_recording_ -- --nocapture`
Run: `cargo test -p pepperx-platform-gnome service_contract_ -- --nocapture`
Expected: FAIL because the GNOME/app seam still stops at session state only.

- [ ] **Step 3: Implement the smallest coordinator wiring**

Keep `pepperx-session` as the state kernel, but route start/stop commands through the app-owned runtime coordinator so a completed recording flows into archived transcription.

- [ ] **Step 4: Add and run the live-recording smoke**

Create `tests/smoke/test_live_recording.sh`.

Run: `tests/smoke/test_live_recording.sh`
Expected: PASS in a real GNOME session with `PEPPERX_PARAKEET_MODEL_DIR` set.

- [ ] **Step 5: Commit**

```bash
git add app/src/app.rs app/src/cli.rs crates/pepperx-platform-gnome/src/service.rs crates/pepperx-session/src/lib.rs tests/smoke/test_live_recording.sh
git commit -m "Connect Pepper X hold-to-talk to live dictation runtime"
```

### Task 6: Add model readiness, inventory, and bootstrap entrypoints

**Files:**
- Modify: `crates/pepperx-models/src/download.rs`
- Modify: `crates/pepperx-asr/src/transcriber.rs`
- Modify: `crates/pepperx-cleanup/src/cleanup.rs`
- Modify: `app/src/cli.rs`
- Modify: `app/src/window.rs`
- Test: `crates/pepperx-models/src/download.rs`
- Test: `app/src/window.rs`
- Test: `tests/smoke/test_model_management.sh`

- [ ] **Step 1: Write failing model-management tests**

Add tests for:
- reporting ASR and cleanup readiness separately
- surfacing cache paths and installed model names in app diagnostics
- refusing a live run when the requested model is not ready

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepperx-models download_ -- --nocapture`
Run: `cargo test -p pepper-x-app model_status_ -- --nocapture`
Expected: FAIL because there is no shared readiness/bootstrap layer yet.

- [ ] **Step 3: Implement the smallest supported bootstrap flow**

Add:
- a headless CLI path to list supported models and cache status
- download/bootstrap for the supported catalog entries
- diagnostics shown in the app history/settings surfaces

- [ ] **Step 4: Re-run the targeted tests and smoke**

Run: `cargo test -p pepperx-models download_ -- --nocapture`
Run: `cargo test -p pepper-x-app model_status_ -- --nocapture`
Run: `tests/smoke/test_model_management.sh`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/pepperx-models crates/pepperx-asr/src/transcriber.rs crates/pepperx-cleanup/src/cleanup.rs app/src/cli.rs app/src/window.rs tests/smoke/test_model_management.sh
git commit -m "Add Pepper X model readiness and bootstrap flows"
```

## Chunk 3: Subproject 3 Cleanup, OCR, and Corrections

### Task 7: Add GNOME-friendly OCR capture and bounded cleanup context

**Files:**
- Create: `crates/pepperx-ocr/Cargo.toml`
- Create: `crates/pepperx-ocr/src/lib.rs`
- Create: `crates/pepperx-ocr/src/gnome_screenshot.rs`
- Create: `crates/pepperx-ocr/src/recognize.rs`
- Modify: `Cargo.toml`
- Modify: `app/Cargo.toml`
- Modify: `app/src/transcription.rs`
- Test: `crates/pepperx-ocr/src/lib.rs`
- Test: `app/src/transcription.rs`
- Test: `tests/smoke/test_ocr_cleanup.sh`

- [ ] **Step 1: Write failing OCR tests**

Add tests for:
- acquiring OCR text from a provided image fixture
- bounding OCR context before it reaches cleanup
- recording `used_ocr` only when OCR text actually contributed

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepperx-ocr ocr_ -- --nocapture`
Run: `cargo test -p pepper-x-app cleanup_ocr_ -- --nocapture`
Expected: FAIL because OCR is currently only a shape in the cleanup request.

- [ ] **Step 3: Implement the smallest OCR path**

Add:
- GNOME screenshot capture for the relevant surface
- local OCR extraction
- bounded OCR text fed into the existing cleanup runtime

- [ ] **Step 4: Re-run the targeted tests and smoke**

Run: `cargo test -p pepperx-ocr ocr_ -- --nocapture`
Run: `cargo test -p pepper-x-app cleanup_ocr_ -- --nocapture`
Run: `tests/smoke/test_ocr_cleanup.sh`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml app/Cargo.toml crates/pepperx-ocr app/src/transcription.rs tests/smoke/test_ocr_cleanup.sh
git commit -m "Add Pepper X OCR-assisted cleanup context"
```

### Task 8: Add deterministic corrections and preferred transcriptions

**Files:**
- Create: `crates/pepperx-corrections/Cargo.toml`
- Create: `crates/pepperx-corrections/src/lib.rs`
- Create: `crates/pepperx-corrections/src/store.rs`
- Modify: `Cargo.toml`
- Modify: `app/Cargo.toml`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/settings.rs`
- Test: `crates/pepperx-corrections/src/lib.rs`
- Test: `app/src/transcription.rs`

- [ ] **Step 1: Write failing corrections tests**

Add tests for:
- applying an exact preferred-transcription override
- applying deterministic replacement rules after cleanup
- persisting and reloading the correction store

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepperx-corrections correction_ -- --nocapture`
Run: `cargo test -p pepper-x-app cleanup_corrections_ -- --nocapture`
Expected: FAIL because there is no correction store yet.

- [ ] **Step 3: Implement the smallest deterministic correction layer**

Apply user-owned corrections after cleanup and before insertion. Keep matching deterministic and auditable.

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p pepperx-corrections correction_ -- --nocapture`
Run: `cargo test -p pepper-x-app cleanup_corrections_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml app/Cargo.toml crates/pepperx-corrections app/src/transcription.rs app/src/settings.rs
git commit -m "Add Pepper X deterministic corrections"
```

### Task 9: Add conservative post-paste learning

**Files:**
- Modify: `crates/pepperx-corrections/src/learning.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/transcript_log.rs`
- Modify: `app/src/window.rs`
- Test: `crates/pepperx-corrections/src/lib.rs`
- Test: `app/src/transcription.rs`

- [ ] **Step 1: Write failing learning tests**

Add tests for:
- learning only from successful insertions
- rejecting low-confidence or destructive learning updates
- recording the learning action in archived diagnostics

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepperx-corrections learning_ -- --nocapture`
Run: `cargo test -p pepper-x-app post_paste_learning_ -- --nocapture`
Expected: FAIL because learning behavior does not exist yet.

- [ ] **Step 3: Implement the smallest conservative learning path**

Keep the first version append-only and auditable. Do not mutate existing correction rules implicitly.

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p pepperx-corrections learning_ -- --nocapture`
Run: `cargo test -p pepper-x-app post_paste_learning_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/pepperx-corrections/src/learning.rs app/src/transcription.rs app/src/transcript_log.rs app/src/window.rs
git commit -m "Add Pepper X conservative post-paste learning"
```

## Chunk 4: Subproject 4 History, Reruns, and Diagnostics

### Task 10: Replace the summary-only History page with a real browser

**Files:**
- Modify: `app/src/history_store.rs`
- Create: `app/src/history_view.rs`
- Modify: `app/src/window.rs`
- Test: `app/src/history_store.rs`
- Test: `app/src/history_view.rs`

- [ ] **Step 1: Write failing history-browser tests**

Add tests for:
- listing archived runs with newest-first ordering
- showing raw transcript, cleaned transcript, model names, insertion backend, and timings
- selecting one run without rebuilding the whole window

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepper-x-app history_store_ history_view_ -- --nocapture`
Expected: FAIL because the History page is still summary text only.

- [ ] **Step 3: Implement the smallest real history browser**

Keep the UI focused:
- list on the left
- selected run details on the right
- explicit raw vs cleaned text
- model/runtime/insertion/OCR diagnostics

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p pepper-x-app history_store_ history_view_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/history_store.rs app/src/history_view.rs app/src/window.rs
git commit -m "Add Pepper X history browser"
```

### Task 11: Add reruns with alternate models and prompts

**Files:**
- Modify: `app/src/cli.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/history_store.rs`
- Modify: `app/src/history_view.rs`
- Test: `app/src/transcription.rs`
- Test: `app/src/history_view.rs`
- Test: `tests/smoke/test_rerun_pipeline.sh`

- [ ] **Step 1: Write failing rerun tests**

Add tests for:
- rerunning an archived recording with a different cleanup model
- rerunning with a modified cleanup prompt
- preserving the original run and storing the rerun as a new archived run linked to its parent

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepper-x-app rerun_ -- --nocapture`
Expected: FAIL because archived runs are not rerunnable yet.

- [ ] **Step 3: Implement the smallest rerun path**

Support:
- headless rerun CLI for one archived run ID
- UI action from the history browser
- parent/child linkage in archive metadata

- [ ] **Step 4: Re-run the targeted tests and smoke**

Run: `cargo test -p pepper-x-app rerun_ -- --nocapture`
Run: `tests/smoke/test_rerun_pipeline.sh`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/cli.rs app/src/transcription.rs app/src/history_store.rs app/src/history_view.rs tests/smoke/test_rerun_pipeline.sh
git commit -m "Add Pepper X archived reruns"
```

### Task 12: Surface first-class runtime diagnostics

**Files:**
- Modify: `app/src/window.rs`
- Modify: `app/src/history_view.rs`
- Modify: `app/src/settings.rs`
- Modify: `README.md`
- Test: `app/src/window.rs`

- [ ] **Step 1: Write failing diagnostics-surface tests**

Add tests for:
- rendering model readiness and cache paths
- rendering insertion backend and OCR usage
- rendering session timings and failure reasons without collapsing them into a single blob

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepper-x-app diagnostics_ -- --nocapture`
Expected: FAIL because diagnostics still live in summary strings only.

- [ ] **Step 3: Implement the smallest diagnostics UI**

Expose:
- model readiness
- cache locations
- latest runtime timings
- insertion path
- OCR usage
- extension connectivity

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p pepper-x-app diagnostics_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/window.rs app/src/history_view.rs app/src/settings.rs README.md
git commit -m "Add Pepper X runtime diagnostics surfaces"
```

## Chunk 5: Subproject 5 Packaging and Operational Polish

### Task 13: Finish package metadata and install assets

**Files:**
- Modify: `packaging/deb/control`
- Modify: `packaging/rpm/pepper-x.spec`
- Modify: `packaging/tests/test_metadata.py`
- Modify: `README.md`
- Test: `packaging/tests/test_metadata.py`

- [ ] **Step 1: Write failing packaging-metadata tests**

Add tests for:
- package dependencies required by live audio, OCR, and model runtimes
- shipping all installed binaries/assets
- matching desktop/autostart metadata across package formats

- [ ] **Step 2: Run the packaging tests and verify they fail**

Run: `python3 -m pytest packaging/tests -q`
Expected: FAIL because the current skeleton only covers the initial shell/runtime pieces.

- [ ] **Step 3: Implement the smallest complete metadata update**

Update both package formats for the real runtime dependencies and installed assets the app now needs.

- [ ] **Step 4: Re-run the packaging tests**

Run: `python3 -m pytest packaging/tests -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add packaging/deb/control packaging/rpm/pepper-x.spec packaging/tests/test_metadata.py README.md
git commit -m "Complete Pepper X package metadata"
```

### Task 14: Add startup behavior, install docs, and release process

**Files:**
- Modify: `app/src/background.rs`
- Modify: `app/src/settings.rs`
- Modify: `README.md`
- Create: `docs/release/pepper-x-v1.md`
- Test: `app/src/background.rs`

- [ ] **Step 1: Write failing startup-behavior tests**

Add tests for:
- launch-at-login toggles reflecting real installed assets
- background startup preserving the app runtime without forcing the window open
- documenting upgrade/install flows for supported distros

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run: `cargo test -p pepper-x-app background_ settings_ -- --nocapture`
Expected: FAIL because startup behavior is still shell-scaffold level.

- [ ] **Step 3: Implement the smallest operational polish**

Make launch-at-login and background behavior honest for packaged installs, then document install, upgrade, and release steps.

- [ ] **Step 4: Re-run the targeted tests**

Run: `cargo test -p pepper-x-app background_ settings_ -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/background.rs app/src/settings.rs README.md docs/release/pepper-x-v1.md
git commit -m "Add Pepper X startup and release docs"
```

### Task 15: Run the full verification matrix on authoritative Linux targets

**Files:**
- Modify as needed: `README.md`
- Modify as needed: `tests/smoke/`

- [ ] **Step 1: Run the full automated suite**

Run:

```bash
cargo fmt --check
cargo test --workspace
cargo check --workspace
bash tests/smoke/test_extension_ipc.sh
./scripts/smoke-hotkey.sh
tests/smoke/test_live_recording.sh
tests/smoke/test_model_management.sh
tests/smoke/test_ocr_cleanup.sh
tests/smoke/test_rerun_pipeline.sh
python3 -m pytest packaging/tests -q
```

Expected: PASS

- [ ] **Step 2: Run the live GNOME validation checklist on Ubuntu**

Validate on GNOME 48+ Wayland:
- modifier-only hold-to-talk with a physical keyboard
- live dictation into a friendly target
- OCR-assisted cleanup
- history browser and rerun flow
- packaged launch-at-login behavior

- [ ] **Step 3: Run the same live checklist on Fedora**

Use the Fedora package/install path, not only a dev checkout.

- [ ] **Step 4: Fix any failures immediately**

Do not wave away distro-specific packaging or runtime differences. Either fix them or narrow the claim in docs and package metadata.

- [ ] **Step 5: Commit the final verification/doc updates**

```bash
git add README.md tests/smoke
git commit -m "Finalize Pepper X verification matrix"
```

Plan complete and saved to `docs/superpowers/plans/2026-03-28-pepper-x-subprojects-2-5.md`. Ready to execute.
