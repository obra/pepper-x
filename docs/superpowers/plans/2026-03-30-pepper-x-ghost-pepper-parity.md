# Pepper X Ghost Pepper Parity Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn Pepper X from a backend-heavy GNOME prototype into a real Ghost Pepper parity product on Linux, while preserving Pepper X's stronger history and diagnostics surfaces.

**Architecture:** Stop treating prerecorded ASR, cleanup, insertion, and reruns as separate product paths. The shipped app must route one shared runtime through the real user journey: packaged install -> first-run onboarding -> model bootstrap -> live hold-to-talk -> cleanup -> insertion -> archive -> history/diagnostics. Ghost Pepper parity is defined by user-visible behavior first; Pepper X-only extras stay, but they must not outrun the core flow again.

**Tech Stack:** Rust, GTK4, libadwaita, PipeWire, GNOME Shell extension (GJS), D-Bus, AT-SPI, `sherpa-onnx`, Parakeet NeMo bundles, `llama.cpp`, local OCR, `.deb`/`.rpm` packaging

---

## Reality Check

Ghost Pepper is not technically deep, but it is product-complete in the places that matter:

- first-run onboarding with setup completion state
- explicit permission and recovery affordances
- automatic model download and readiness UX
- microphone selection and sound-check UI
- live hold-to-talk dictation with visible status feedback
- cleanup controls with prompt editing
- background-first behavior with a useful control surface

Pepper X already has real backend depth:

- live PipeWire capture
- local ASR
- local cleanup
- insertion fallbacks
- archived history
- reruns
- diagnostics

But today those capabilities are mostly exposed as CLI flows, summary text, or lower-level infrastructure. The parity work has to prioritize the shipped product path, not another backend slice.

## Parity Bar

Do not call Pepper X "Ghost Pepper parity" until all of these are true on packaged Ubuntu and Fedora GNOME systems:

1. Fresh install leads to a real first-run setup flow instead of a bare shell window.
2. The setup flow can get a user from zero to a successful live dictation without reading docs.
3. Models auto-download from the GUI with progress, retry, and readiness reporting. ASR readiness blocks completion; cleanup can continue loading in the background as long as its state is visible and honest.
4. Live modifier hold-to-talk runs the full pipeline: record -> transcribe -> optionally clean -> insert -> archive.
5. The user can pick a microphone, see a level meter, and recover from bad input or broken prerequisites.
6. The user can control cleanup from the GUI, including on/off, prompt profile, and freeform prompt editing.
7. The background/panel surface reflects readiness, failure, and recovery state.
8. History, reruns, and diagnostics still work on real live runs, not only prerecorded fixtures.
9. Packaged offline dictation works after model download completes.
10. Package install, upgrade, and uninstall flows preserve runtime correctness and user state.

## Not Parity Blockers

These are useful, but they do not block parity with Ghost Pepper:

- expanding the model catalog far beyond a sane default set
- a dedicated evaluation/lab UI beyond the current history/rerun comparison
- universal insertion guarantees for every hostile target class
- a Sparkle-style self-updater instead of distro-native update flows

## File Structure

**Planning docs to keep current:**
- `docs/superpowers/specs/2026-03-27-pepper-x-design.md`
  - Keep as the long-lived product contract, but stop reading it as "already effectively implemented".
- `docs/superpowers/plans/2026-03-28-pepper-x-iterative-ai-loops.md`
  - Historical context for the backend loops that already landed.
- `docs/superpowers/plans/2026-03-28-pepper-x-subprojects-2-5.md`
  - Historical execution plan for subsystem slices; this parity plan supersedes it as the release-critical roadmap.
- `docs/release/pepper-x-v1.md`
  - Release checklist that must be updated to match the parity bar.

**App shell and product UX:**
- Create: `app/src/app_model.rs`
  - Shared app-wide state for onboarding, runtime status, model readiness, and user-facing recovery actions.
- Create: `app/src/onboarding.rs`
  - First-run setup wizard and retryable setup screens.
- Create: `app/src/settings_view.rs`
  - Real settings form widgets instead of summary text.
- Create: `app/src/diagnostics_view.rs`
  - Actionable diagnostics widgets with retry/open/retest affordances.
- Create: `app/src/overlay.rs`
  - Live recording/transcribing/cleanup/clipboard-fallback overlay surface.
- Modify: `app/src/app.rs`
  - Compose the shared app model, initial routing, and runtime-to-UI wiring.
- Modify: `app/src/background.rs`
  - Background-first actions, panel-affordance hooks, and first-run startup behavior.
- Modify: `app/src/window.rs`
  - Replace summary labels with real pages and stateful widgets.
- Modify: `app/src/settings.rs`
  - Persist onboarding state, launch-at-login, mic, cleanup, prompt editing, and integration choices.

**Shared runtime path:**
- Modify: `app/src/session_runtime.rs`
  - One shared live session pipeline for recording, ASR, cleanup, insertion, archiving, and status updates.
- Modify: `app/src/transcription.rs`
  - Collapse prerecorded/live/rerun logic onto shared steps instead of separate product paths.
- Modify: `app/src/cli.rs`
  - Keep CLI as a dev/test seam, not the primary product surface.
- Modify: `app/src/history_view.rs`
  - Preserve current history/rerun advantages, but make them reflect the real live runtime.
- Modify: `app/src/history_store.rs`
  - Archive richer live-session metadata and setup/runtime diagnostics.

**Runtime crates:**
- Modify: `crates/pepperx-audio/src/devices.rs`
  - Stable microphone inventory for onboarding and settings UIs.
- Modify: `crates/pepperx-audio/src/recording.rs`
  - Live-level sampling hooks and more explicit runtime errors for bad device/input conditions.
- Modify: `crates/pepperx-models/src/download.rs`
  - Bootstrap progress, retryable failures, and GUI-friendly status callbacks.
- Modify: `crates/pepperx-models/src/cache.rs`
  - Better install/readiness/disk-usage metadata for UI and release checks.
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`
  - Live modifier capture status, insertion support matrix, and runtime failure classification.
- Modify: `crates/pepperx-platform-gnome/src/context.rs`
  - Explicit OCR/context readiness and failure reporting for setup and diagnostics.
- Modify: `crates/pepperx-platform-gnome/src/service.rs`
  - Narrow UI-facing status and action contract for the extension.

**GNOME Shell extension:**
- Modify: `gnome-extension/extension.js`
  - Indicator state, richer menu actions, and capability/error display.
- Modify: `gnome-extension/ipc.js`
  - Readiness/error/status polling and retry-safe actions.
- Delete or repurpose: `gnome-extension/keybindings.js`
  - Remove dead scaffolding if it no longer reflects the real modifier-capture path.

**Verification and packaging:**
- Modify: `README.md`
  - Honest user/developer setup and packaging instructions.
- Modify: `packaging/deb/control`
  - Runtime dependencies for the shipped app and helper surfaces.
- Modify: `packaging/rpm/pepper-x.spec`
  - Same for Fedora.
- Modify: `packaging/deb/pepper-x.desktop`
  - First-run/user-visible shell routing.
- Modify: `packaging/deb/pepper-x-autostart.desktop`
  - Background-first startup behavior.
- Create: `tests/smoke/test_first_run_onboarding.sh`
  - Packaged and dev first-run flow smoke.
- Create: `tests/smoke/test_gui_model_bootstrap.sh`
  - GUI-driven model bootstrap smoke.
- Create: `tests/smoke/test_live_dictation_pipeline.sh`
  - Real live dictation end-to-end smoke.
- Create: `tests/smoke/test_packaged_beta_flow.sh`
  - Authoritative packaged Ubuntu/Fedora flow smoke.

## Chunk 1: Reset Pepper X Around the Real User Journey

### Task 1: Add a shared app model and startup routing that reflects product state

**Files:**
- Create: `app/src/app_model.rs`
- Modify: `app/src/app.rs`
- Modify: `app/src/background.rs`
- Modify: `app/src/settings.rs`
- Test: `app/src/app.rs`
- Test: `app/src/settings.rs`

- [ ] **Step 1: Write failing startup-routing tests**

Add tests for:
- first run opens onboarding instead of a bare settings summary
- completed setup starts background-first without forcing the main window open
- broken setup prerequisites surface a recoverable setup state, not only stderr text

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepper-x-app app_shell_ -- --nocapture
cargo test -p pepper-x-app settings_ -- --nocapture
```

Expected: FAIL because the current app has no shared onboarding/setup state.

- [ ] **Step 3: Implement the smallest shared app model**

Implement:
- persisted onboarding/setup completion state
- runtime readiness summary owned by the app instead of ad hoc provider closures
- initial window/background routing based on setup state

- [ ] **Step 4: Re-run the targeted tests**

Run the commands from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/app_model.rs app/src/app.rs app/src/background.rs app/src/settings.rs
git commit -m "Add Pepper X shared app model and startup routing"
```

### Task 2: Replace read-only summary pages with real page scaffolding

**Files:**
- Create: `app/src/settings_view.rs`
- Create: `app/src/diagnostics_view.rs`
- Modify: `app/src/window.rs`
- Test: `app/src/window.rs`

- [ ] **Step 1: Write failing window-page tests**

Add tests for:
- the main window builds real page containers instead of text-only summaries
- settings and diagnostics pages can render stateful rows/cards
- the window can switch between setup, settings, history, and diagnostics states cleanly

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepper-x-app window_ -- --nocapture
```

Expected: FAIL because the window still renders summary labels.

- [ ] **Step 3: Implement the smallest real page scaffold**

Implement:
- settings page as a form container
- diagnostics page as a card/list container
- window routing that can host onboarding separately from the normal shell

- [ ] **Step 4: Re-run the targeted tests**

Run the command from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/settings_view.rs app/src/diagnostics_view.rs app/src/window.rs
git commit -m "Replace Pepper X summary pages with real page scaffolding"
```

## Chunk 2: Build Real First-Run Onboarding and Recovery

### Task 3: Add a Ghost-Pepper-class setup wizard for GNOME

**Files:**
- Create: `app/src/onboarding.rs`
- Modify: `app/src/app_model.rs`
- Modify: `app/src/app.rs`
- Test: `app/src/onboarding.rs`

- [ ] **Step 1: Write failing onboarding tests**

Add tests for:
- welcome -> setup -> try-it -> done progression
- persisted completion state
- reopening setup after a partial or failed first run

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepper-x-app onboarding_ -- --nocapture
```

Expected: FAIL because no onboarding module exists.

- [ ] **Step 3: Implement the smallest real wizard**

Include:
- welcome framing
- setup checklist with progress
- try-it step tied to the real runtime state
- done state that hands back to background operation

- [ ] **Step 4: Re-run the targeted tests**

Run the command from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/onboarding.rs app/src/app_model.rs app/src/app.rs
git commit -m "Add Pepper X onboarding wizard"
```

### Task 4: Add explicit prerequisite probes and recovery actions

**Files:**
- Modify: `app/src/onboarding.rs`
- Modify: `app/src/diagnostics_view.rs`
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`
- Modify: `crates/pepperx-platform-gnome/src/context.rs`
- Modify: `gnome-extension/extension.js`
- Test: `app/src/onboarding.rs`
- Test: `crates/pepperx-platform-gnome/src/context.rs`

- [ ] **Step 1: Write failing prerequisite tests**

Add tests for:
- broken modifier capture exposes a user-facing recovery state
- missing OCR/context support reports a retryable status
- missing extension connectivity does not silently degrade without UI

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepper-x-app onboarding_recovery_ -- --nocapture
cargo test -p pepperx-platform-gnome context_ -- --nocapture
```

Expected: FAIL because failures are currently surfaced as plain diagnostics text or logs.

- [ ] **Step 3: Implement the smallest recovery contract**

Implement:
- probe results the UI can render
- concrete recovery actions like retry, open docs, and recheck
- extension-facing status that distinguishes disconnected, degraded, and ready

- [ ] **Step 4: Re-run the targeted tests**

Run the commands from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/onboarding.rs app/src/diagnostics_view.rs crates/pepperx-platform-gnome/src/atspi.rs crates/pepperx-platform-gnome/src/context.rs gnome-extension/extension.js
git commit -m "Add Pepper X prerequisite recovery flows"
```

## Chunk 3: Turn Model and Audio Infrastructure into Product UX

### Task 5: Add GUI-driven automatic model bootstrap with progress and retry

**Files:**
- Modify: `app/src/app_model.rs`
- Modify: `app/src/onboarding.rs`
- Modify: `app/src/settings_view.rs`
- Modify: `crates/pepperx-models/src/download.rs`
- Modify: `crates/pepperx-models/src/cache.rs`
- Test: `crates/pepperx-models/src/download.rs`
- Test: `app/src/onboarding.rs`

- [ ] **Step 1: Write failing model-bootstrap tests**

Add tests for:
- first-run setup triggers default model bootstrap automatically
- bootstrap progress and failure states are observable by the UI
- a failed download can be retried without restarting the whole app

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepperx-models download_ -- --nocapture
cargo test -p pepper-x-app onboarding_model_ -- --nocapture
```

Expected: FAIL because model bootstrap is still CLI-first.

- [ ] **Step 3: Implement the smallest GUI-first model flow**

Implement:
- default bootstrap plan for one ASR model and one cleanup model
- progress callback/status objects
- retryable failures and readiness confirmation
- onboarding gating that requires ASR readiness, while cleanup can continue bootstrapping in the background

- [ ] **Step 4: Re-run the targeted tests**

Run the commands from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/app_model.rs app/src/onboarding.rs app/src/settings_view.rs crates/pepperx-models/src/download.rs crates/pepperx-models/src/cache.rs
git commit -m "Add Pepper X GUI model bootstrap"
```

### Task 6: Add real microphone controls and live input feedback

**Files:**
- Modify: `app/src/settings_view.rs`
- Modify: `app/src/onboarding.rs`
- Modify: `app/src/settings.rs`
- Modify: `crates/pepperx-audio/src/devices.rs`
- Modify: `crates/pepperx-audio/src/recording.rs`
- Test: `crates/pepperx-audio/src/devices.rs`
- Test: `app/src/settings.rs`

- [ ] **Step 1: Write failing microphone UX tests**

Add tests for:
- selected microphone persistence and round-tripping
- live device inventory available to onboarding and settings
- a level meter or signal-strength sample hook is available without starting a full dictation session

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepperx-audio device_ -- --nocapture
cargo test -p pepper-x-app settings_ -- --nocapture
```

Expected: FAIL because the current runtime has no user-facing level/selection flow.

- [ ] **Step 3: Implement the smallest microphone UI contract**

Implement:
- device picker support for onboarding and settings
- light-weight level sampling for a meter
- explicit bad-input/no-signal errors that the UI can explain

- [ ] **Step 4: Re-run the targeted tests**

Run the commands from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/settings_view.rs app/src/onboarding.rs app/src/settings.rs crates/pepperx-audio/src/devices.rs crates/pepperx-audio/src/recording.rs
git commit -m "Add Pepper X microphone controls and level feedback"
```

## Chunk 4: Make the Live Modifier Path the Real Product Path

### Task 7: Route live modifier hold-to-talk through the full pipeline

**Files:**
- Modify: `app/src/session_runtime.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/app_model.rs`
- Modify: `app/src/history_store.rs`
- Test: `app/src/session_runtime.rs`
- Test: `app/src/transcription.rs`

- [ ] **Step 1: Write failing live-pipeline tests**

Add tests for:
- live modifier capture produces the same cleanup/insertion/archive behavior as prerecorded runs
- live runs archive raw transcript, cleaned transcript, insertion diagnostics, and setup/runtime metadata together
- a live failure leaves enough diagnostics for retry instead of silently dropping the run

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepper-x-app session_runtime_ -- --nocapture
cargo test -p pepper-x-app cleanup_insert_ -- --nocapture
```

Expected: FAIL because the live path is still narrower than the prerecorded cleanup/insertion path.

- [ ] **Step 3: Implement the smallest shared live pipeline**

Implement:
- one runtime path for live and prerecorded inputs
- cleanup and insertion on live stop
- consistent archive generation for live sessions

- [ ] **Step 4: Re-run the targeted tests**

Run the commands from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/session_runtime.rs app/src/transcription.rs app/src/app_model.rs app/src/history_store.rs
git commit -m "Route Pepper X live dictation through the full pipeline"
```

### Task 8: Add visible live-state feedback and fallback messaging

**Files:**
- Create: `app/src/overlay.rs`
- Modify: `app/src/app_model.rs`
- Modify: `app/src/app.rs`
- Modify: `gnome-extension/extension.js`
- Test: `app/src/overlay.rs`

- [ ] **Step 1: Write failing feedback-surface tests**

Add tests for:
- recording/transcribing/cleanup/clipboard-fallback states map to one user-facing overlay model
- panel/indicator state changes when the runtime state changes
- insertion fallback messages are visible without opening the diagnostics page

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepper-x-app overlay_ -- --nocapture
```

Expected: FAIL because no overlay module exists and the indicator is effectively static.

- [ ] **Step 3: Implement the smallest live feedback surface**

Implement:
- overlay state model
- app-owned overlay UI
- extension-visible ready/busy/error state

- [ ] **Step 4: Re-run the targeted tests**

Run the command from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/overlay.rs app/src/app_model.rs app/src/app.rs gnome-extension/extension.js
git commit -m "Add Pepper X live status feedback surfaces"
```

### Task 9: Expand the live insertion contract past GNOME Text Editor

**Files:**
- Modify: `app/src/transcription.rs`
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`
- Modify: `crates/pepperx-platform-gnome/src/context.rs`
- Modify: `tests/smoke/test_live_dictation_pipeline.sh`
- Test: `crates/pepperx-platform-gnome/src/atspi.rs`

- [ ] **Step 1: Write failing insertion-matrix tests**

Add tests for:
- live runs can target at least one friendly editor and one browser textarea class
- terminal fallback selection is archived honestly
- the live path stops hard-coding GNOME Text Editor as the only supported product target

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepperx-platform-gnome accessible_insert_ -- --nocapture
```

Expected: FAIL because the product path is still effectively fixed to the friendly target policy.

- [ ] **Step 3: Implement the smallest honest live support matrix**

Ship and document:
- friendly editor
- browser textarea/contenteditable
- terminal fallback path

Keep hostile/Wine behavior diagnostic-only until proven.

- [ ] **Step 4: Re-run the targeted tests**

Run the command from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/transcription.rs crates/pepperx-platform-gnome/src/atspi.rs crates/pepperx-platform-gnome/src/context.rs tests/smoke/test_live_dictation_pipeline.sh
git commit -m "Expand Pepper X live insertion support matrix"
```

## Chunk 5: Finish the Product Control Surfaces

### Task 10: Add real settings for cleanup, prompt editing, and launch-at-login

**Files:**
- Modify: `app/src/settings_view.rs`
- Modify: `app/src/settings.rs`
- Modify: `app/src/app_model.rs`
- Test: `app/src/settings.rs`
- Test: `app/src/window.rs`

- [ ] **Step 1: Write failing settings-form tests**

Add tests for:
- cleanup on/off toggle
- prompt profile selector
- freeform cleanup prompt editing persistence
- launch-at-login toggle

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepper-x-app settings_ -- --nocapture
cargo test -p pepper-x-app window_ -- --nocapture
```

Expected: FAIL because the current shell has no real settings controls.

- [ ] **Step 3: Implement the smallest full settings surface**

Implement:
- cleanup enabled state
- prompt profile plus custom prompt editing
- launch-at-login control
- explicit save/apply feedback

- [ ] **Step 4: Re-run the targeted tests**

Run the commands from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/settings_view.rs app/src/settings.rs app/src/app_model.rs
git commit -m "Add Pepper X real settings controls"
```

### Task 11: Enrich the extension/panel control surface

**Files:**
- Modify: `gnome-extension/extension.js`
- Modify: `gnome-extension/ipc.js`
- Modify: `crates/pepperx-platform-gnome/src/service.rs`
- Test: `tests/smoke/test_extension_ipc.sh`

- [ ] **Step 1: Write failing indicator-surface checks**

Add checks for:
- status line or icon state changes for ready, busy, and error
- menu actions for settings, history, diagnostics, and retry/recheck where appropriate
- capability polling that distinguishes disconnected from degraded

- [ ] **Step 2: Run the targeted checks and verify they fail**

Run:
```sh
bash tests/smoke/test_extension_ipc.sh
```

Expected: FAIL because the current extension surface is too thin to express parity-level status.

- [ ] **Step 3: Implement the smallest richer panel UX**

Implement:
- better status polling
- richer menu content
- action hooks that match the app-owned recovery flows
- pre-setup minimal menu behavior so unfinished first runs expose setup first, not the full normal shell

- [ ] **Step 4: Re-run the targeted checks**

Run the command from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add gnome-extension/extension.js gnome-extension/ipc.js crates/pepperx-platform-gnome/src/service.rs tests/smoke/test_extension_ipc.sh
git commit -m "Enrich Pepper X extension control surface"
```

## Chunk 6: Preserve Pepper X Advantages Without Losing the Plot

### Task 12: Keep history, reruns, and diagnostics aligned with the real live product

**Files:**
- Modify: `app/src/history_view.rs`
- Modify: `app/src/history_store.rs`
- Modify: `app/src/diagnostics_view.rs`
- Test: `app/src/history_view.rs`
- Test: `app/src/history_store.rs`

- [ ] **Step 1: Write failing live-history tests**

Add tests for:
- live sessions appear in history with enough metadata to debug failures
- reruns preserve the parent/child relationship for live-origin runs
- diagnostics actions can reopen the relevant archived run instead of only showing summary text

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:
```sh
cargo test -p pepper-x-app history_store_ -- --nocapture
cargo test -p pepper-x-app history_view_ -- --nocapture
```

Expected: FAIL because current history/diagnostics still assume a more static archive surface.

- [ ] **Step 3: Implement the smallest live-aligned polish**

Implement:
- tighter linkage between diagnostics and archived runs
- better live-run metadata presentation
- rerun flows that clearly describe what changed

- [ ] **Step 4: Re-run the targeted tests**

Run the commands from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add app/src/history_view.rs app/src/history_store.rs app/src/diagnostics_view.rs
git commit -m "Align Pepper X history and diagnostics with live sessions"
```

## Chunk 7: Package, Validate, and Ship the Real Product Path

### Task 13: Make packaged installs behave like the product, not the repo

**Files:**
- Modify: `README.md`
- Modify: `packaging/deb/control`
- Modify: `packaging/rpm/pepper-x.spec`
- Modify: `packaging/deb/pepper-x.desktop`
- Modify: `packaging/deb/pepper-x-autostart.desktop`
- Create: `tests/smoke/test_first_run_onboarding.sh`
- Create: `tests/smoke/test_gui_model_bootstrap.sh`
- Create: `tests/smoke/test_packaged_beta_flow.sh`
- Test: `packaging/tests/test_metadata.py`

- [ ] **Step 1: Write failing packaged-flow checks**

Add checks for:
- first packaged launch opens onboarding
- autostart launch remains background-first after setup completion
- packaged assets point to the real installed runtime and helper locations

- [ ] **Step 2: Run the targeted checks and verify they fail**

Run:
```sh
python3 -m pytest packaging/tests -q
```

Expected: FAIL because the current packaging metadata does not enforce the parity product flow.

- [ ] **Step 3: Implement the smallest packaged UX alignment**

Implement:
- accurate desktop/autostart behavior
- honest runtime dependency metadata
- first-run packaged smoke coverage

- [ ] **Step 4: Re-run the targeted checks**

Run the command from Step 2.

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add README.md packaging/deb/control packaging/rpm/pepper-x.spec packaging/deb/pepper-x.desktop packaging/deb/pepper-x-autostart.desktop packaging/tests/test_metadata.py tests/smoke/test_first_run_onboarding.sh tests/smoke/test_gui_model_bootstrap.sh tests/smoke/test_packaged_beta_flow.sh
git commit -m "Align Pepper X packaging with the parity product flow"
```

### Task 14: Add the authoritative live Ubuntu/Fedora parity matrix

**Files:**
- Modify: `docs/release/pepper-x-v1.md`
- Modify: `README.md`
- Create: `tests/smoke/test_live_dictation_pipeline.sh`

- [ ] **Step 1: Write the live validation matrix before implementation drift resumes**

Capture explicit required checks for:
- packaged first run
- model bootstrap
- offline dictation after model bootstrap
- modifier-only live dictation
- cleanup on/off
- browser textarea insertion
- terminal fallback
- history/rerun
- diagnostics and recovery
- upgrade and uninstall

- [ ] **Step 2: Add the smoke/helper scaffolding**

Add helpers and checklists for Ubuntu and Fedora GNOME 48+ live sessions.

- [ ] **Step 3: Run the full automated suite**

Run:
```sh
cargo fmt --check
cargo test --workspace
cargo check --workspace
bash tests/smoke/test_extension_ipc.sh
python3 -m pytest packaging/tests -q
```

Expected: PASS before attempting the live distro matrix.

- [ ] **Step 4: Run the authoritative live distro matrix**

Run the packaged and live-session checks on:
- Ubuntu 25.04+ GNOME Wayland
- Fedora 42+ GNOME Wayland

Expected:
- new user can complete onboarding and successfully dictate without reading docs
- packaged upgrades preserve settings/history state
- uninstall removes packaged assets and leaves user data intact

- [ ] **Step 5: Commit**

```bash
git add docs/release/pepper-x-v1.md README.md tests/smoke/test_live_dictation_pipeline.sh
git commit -m "Add Pepper X parity release matrix"
```

## Execution Order

Implement this plan in this order:

1. Chunk 1 and Chunk 2 first. Pepper X needs a product shell before more backend work.
2. Chunk 3 next. Ghost Pepper parity depends on model/bootstrap/audio UX, not hidden CLI seams.
3. Chunk 4 immediately after. The live modifier path is the actual product.
4. Chunk 5 once the live path is real. Settings and panel affordances should control a working product, not placeholders.
5. Chunk 6 only after the core flow works. Keep Pepper X's history/diagnostics edge, but do not let it lead the roadmap again.
6. Chunk 7 last, with real packages and live distro validation as the ship gate.

## Review Notes

- If time pressure forces triage, do not cut onboarding, model bootstrap UX, or the full live dictation path. Those are the actual parity blockers.
- If modifier-only capture remains unreliable in live distro validation, stop and spin a dedicated hotkey fallback track before continuing to polish surface UX.
- If OCR/context capture remains unreliable, ship cleanup with honest degradation rather than lying about support.
