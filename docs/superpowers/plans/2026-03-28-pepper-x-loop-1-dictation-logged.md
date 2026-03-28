# Pepper X Loop 1 Dictation Logged Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the first real AI vertical slice to Pepper X: transcribe a prerecorded WAV with the real local ASR backend, persist an inspectable archive entry, and surface the result through a stable Pepper X entrypoint.

**Architecture:** Add one focused ASR crate plus thin app-side orchestration and transcript logging modules. Use `sherpa-onnx` with a Parakeet NeMo model from the start so the only loop-1 simplification is prerecorded audio instead of live capture. Leave the GNOME/platform and session state surfaces untouched for this loop and make the new behavior observable through a CLI entrypoint and the existing History page.

**Tech Stack:** Rust, Cargo workspace, `sherpa-onnx`, Parakeet NeMo model bundle, GTK4, libadwaita, JSON archive files, shell-based smoke checks

---

## File Structure

**Repository root:** `pepper-x/`

**Create:**
- `crates/pepperx-asr/Cargo.toml`
  - Focused ASR crate manifest.
- `crates/pepperx-asr/src/lib.rs`
  - Public `transcribe_wav` surface and request/result types.
- `crates/pepperx-asr/src/transcriber.rs`
  - Real `sherpa-onnx` WAV transcription path and input/model validation.
- `app/src/cli.rs`
  - Parse and run the non-GUI `--transcribe-wav <path>` entry mode.
- `app/src/transcription.rs`
  - One-shot orchestration from WAV path to ASR result to transcript-log append.
- `app/src/transcript_log.rs`
  - Append-only transcript artifact store for loop 1.
- `tests/fixtures/loop1-hello.wav`
  - Stable prerecorded speech sample for loop-1 smoke coverage.
- `tests/smoke/test_prerecorded_asr.sh`
  - Env-gated end-to-end smoke helper for loop 1.

**Modify:**
- `Cargo.toml`
  - Add `crates/pepperx-asr` to the workspace.
- `app/Cargo.toml`
  - Add the ASR crate and any small supporting dependencies the app shell needs.
- `app/src/main.rs`
  - Declare the new app modules and dispatch between normal GUI startup and `--transcribe-wav <path>`.
- `app/src/app.rs`
  - Wire the app shell to the transcript log for showing recent entries.
- `app/src/window.rs`
  - Replace the history placeholder with a minimal transcript/archive view.
- `README.md`
  - Document the loop-1 model prerequisite, CLI entrypoint, and local smoke commands.

**Test:**
- `crates/pepperx-asr/src/lib.rs`
- `crates/pepperx-asr/src/transcriber.rs`
- `app/src/cli.rs`
- `app/src/transcript_log.rs`
- `app/src/window.rs`

---

## Chunk 1: Runtime Skeleton And Archive Persistence

### Task 1: Add failing tests for transcript-log persistence

**Files:**
- Modify: `app/Cargo.toml`
- Create: `app/src/transcript_log.rs`

- [ ] **Step 1: Write the failing transcript-log tests**

Add tests that prove:
- Pepper X can append a transcript entry to an injected log root
- reloading the log preserves:
  - source WAV path
  - transcript text
  - backend/model identifier
  - elapsed timing data
- recent entries are returned newest-first

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:
```sh
cd pepper-x
cargo test -p pepper-x-app transcript_log -- --nocapture
```

Expected:
- the tests fail because the transcript log module does not exist yet

- [ ] **Step 3: Implement the minimal transcript log**

Implement:
- a small transcript-entry model in `app/src/transcript_log.rs`
- append-only persistence under a runtime-injected root directory
- UTF-8 JSON Lines storage via `serde_json`, not SQLite
- no ASR logic yet

Keep this narrow:
- no insertion code
- no microphone code
- no cleanup logic
- no model bootstrap logic yet

- [ ] **Step 4: Re-run the targeted tests**

Run the command from Step 2.

Expected:
- the transcript-log tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add app/Cargo.toml app/src/transcript_log.rs
git -C pepper-x commit -m "Add Pepper X transcript log"
```

---

## Chunk 2: Real Sherpa-Onnx Transcription

### Task 2: Add failing tests for the real WAV transcription path

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/pepperx-asr/Cargo.toml`
- Create: `crates/pepperx-asr/src/lib.rs`
- Create: `crates/pepperx-asr/src/transcriber.rs`
- Create: `tests/fixtures/loop1-hello.wav`

- [ ] **Step 1: Write the failing transcription tests**

Add tests that prove:
- transcription requests reject missing WAV files cleanly
- transcription requests reject incomplete model directories cleanly
- the ASR crate exposes the chosen backend name as `sherpa-onnx`
- an env-gated, `#[ignore]` real-backend test can transcribe `tests/fixtures/loop1-hello.wav` to a non-empty result

Use a tiny internal seam for testability, but do not introduce a broad backend-abstraction framework.

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:
```sh
cd pepper-x
cargo test -p pepperx-asr transcriber_ -- --nocapture
```

Expected:
- the new tests fail because the real transcription module does not exist yet

- [ ] **Step 3: Implement the real transcription module**

Implement:
- the new `pepperx-asr` crate
- `sherpa-onnx` integration in `crates/pepperx-asr/src/transcriber.rs`
- model-directory validation for the chosen Parakeet bundle
- WAV loading via the backend's supported path
- a real-backend test marked `#[ignore]` so `cargo test --workspace` stays green without local model assets
- request/result structs that capture:
  - WAV path
  - model directory
  - backend name
  - model name
  - elapsed time
  - transcript text or failure

Choose one concrete loop-1 model family and document it in code comments and README:
- preferred: `sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8`
- acceptable fallback if the repository tests need a smaller default: the 110m Parakeet INT8 bundle

- [ ] **Step 4: Re-run the targeted tests**

Run the command from Step 2.

Expected:
- the transcription tests pass

- [ ] **Step 5: Add an end-to-end smoke helper**

Create `tests/smoke/test_prerecorded_asr.sh`.

The smoke script should:
- require `PEPPERX_PARAKEET_MODEL_DIR` or another explicit model-root environment variable
- require `PEPPERX_STATE_ROOT`, with the caller responsible for creating a fresh temporary state root for the run
- use the exact fixture at `tests/fixtures/loop1-hello.wav`
- run the Pepper X CLI entrypoint against that fixed sample WAV
- assert that Pepper X emits a non-empty transcript and writes a transcript-log artifact

- [ ] **Step 6: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add Cargo.toml Cargo.lock crates/pepperx-asr/Cargo.toml crates/pepperx-asr/src/lib.rs crates/pepperx-asr/src/transcriber.rs tests/fixtures/loop1-hello.wav tests/smoke/test_prerecorded_asr.sh
git -C pepper-x commit -m "Add Pepper X sherpa-onnx transcription path"
```

---

## Chunk 3: Stable App Entry Points

### Task 3: Add failing tests for the loop-1 entrypoints

**Files:**
- Modify: `app/Cargo.toml`
- Create: `app/src/cli.rs`
- Create: `app/src/transcription.rs`
- Modify: `app/src/main.rs`
- Modify: `app/src/app.rs`
- Modify: `app/src/window.rs`

- [ ] **Step 1: Write the failing entrypoint and history tests**

Add tests that prove:
- Pepper X exposes a stable `--transcribe-wav <path>` entry mode without starting the GUI
- invalid CLI arguments are rejected with a stable error
- the history page can render recent archive entries loaded from the same injected `PEPPERX_STATE_ROOT` the CLI path writes to
- the history page no longer says transcription arrives later

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:
```sh
cd pepper-x
cargo test -p pepper-x-app cli_mode -- --nocapture
cargo test -p pepper-x-app app_shell -- --nocapture
```

Expected:
- the new CLI/history assertions fail because the app still has only placeholder history content and no dedicated CLI entrypoint

- [ ] **Step 3: Implement the smallest stable entrypoints**

Implement:
- `app/src/cli.rs` with a narrow parser/runner for `--transcribe-wav <path>`
- `app/src/transcription.rs` with one-shot orchestration from WAV path to ASR result to transcript-log append
- module declarations in `app/src/main.rs` for `cli`, `transcription`, and `transcript_log`
- a `--transcribe-wav <path>` mode in `main.rs`
- app wiring that reads recent transcript-log entries from an injected or environment-selected state root
- a minimal history view that shows the latest transcript text and basic metadata

Keep this intentionally shallow:
- no file picker
- no editable transcript UI yet
- no insertion controls yet
- no cleanup controls yet

- [ ] **Step 4: Re-run the targeted tests**

Run the command from Step 2.

Expected:
- the CLI and history tests pass

- [ ] **Step 5: Run the loop-1 smoke**

Run:
```sh
cd pepper-x
export PEPPERX_STATE_ROOT="$(mktemp -d)"
PEPPERX_PARAKEET_MODEL_DIR=/path/to/model PEPPERX_STATE_ROOT="$PEPPERX_STATE_ROOT" tests/smoke/test_prerecorded_asr.sh
```

Expected:
- the model bundle is available locally through the explicit environment variable
- Pepper X transcribes the sample WAV through `sherpa-onnx`
- Pepper X persists a transcript-log entry
- the same `PEPPERX_STATE_ROOT` can be reused for the History-page smoke

- [ ] **Step 6: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add app/Cargo.toml app/src/cli.rs app/src/transcription.rs app/src/main.rs app/src/app.rs app/src/window.rs README.md
git -C pepper-x commit -m "Surface Pepper X loop 1 dictation logs"
```

---

## Chunk 4: Verification

### Task 4: Run the exact loop-1 verification set

**Files:**
- Test: `crates/pepperx-asr/src/lib.rs`
- Test: `crates/pepperx-asr/src/transcriber.rs`
- Test: `app/src/cli.rs`
- Test: `app/src/transcript_log.rs`
- Test: `app/src/window.rs`
- Test: `tests/smoke/test_prerecorded_asr.sh`

- [ ] **Step 1: Run formatting and targeted tests**

Run:
```sh
cd pepper-x
cargo fmt --check
cargo test -p pepperx-asr -- --nocapture
cargo test -p pepper-x-app cli_mode -- --nocapture
cargo test -p pepper-x-app transcript_log -- --nocapture
cargo test -p pepper-x-app app_shell -- --nocapture
```

Expected:
- all targeted tests pass

- [ ] **Step 2: Run workspace validation**

Run:
```sh
cd pepper-x
cargo check --workspace
cargo test --workspace
```

Expected:
- the workspace builds and tests pass with the loop-1 ASR crate included

- [ ] **Step 3: Run the real-backend smoke**

Run:
```sh
cd pepper-x
export PEPPERX_STATE_ROOT="$(mktemp -d)"
PEPPERX_PARAKEET_MODEL_DIR=/path/to/model cargo test -p pepperx-asr transcriber_real_ -- --ignored --nocapture
PEPPERX_PARAKEET_MODEL_DIR=/path/to/model PEPPERX_STATE_ROOT="$PEPPERX_STATE_ROOT" tests/smoke/test_prerecorded_asr.sh
```

Expected:
- Pepper X produces a real transcript from the sample WAV and archives it successfully

- [ ] **Step 4: Run an optional manual app sanity check**

The automated history assertion above is the required proof that the GUI reads the same state root the CLI writes. The manual smoke below is a final sanity check only.

Run:
```sh
cd pepper-x
STATE_ROOT="$(mktemp -d)"
PEPPERX_PARAKEET_MODEL_DIR=/path/to/model PEPPERX_STATE_ROOT="$STATE_ROOT" tests/smoke/test_prerecorded_asr.sh
PEPPERX_STATE_ROOT="$STATE_ROOT" cargo run -p pepper-x-app
```

Expected:
- the app launches
- the history page shows the transcript-log entry created in the earlier smoke run
- no insertion or cleanup controls are implied yet

- [ ] **Step 5: Confirm the loop is ready for the next slice**

```bash
git -C pepper-x status --short
```

Expected:
- the working tree is clean because the implementation commits already landed during the earlier tasks
