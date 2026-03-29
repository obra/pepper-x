# Pepper X Loop 5 Cleanup Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a real local cleanup path to Pepper X that preserves raw ASR output, records cleaned output separately, and can reuse the existing insertion pipeline without hiding insertion failures.

**Architecture:** Keep app-owned orchestration in `app/src/transcription.rs`, but isolate the real `llama.cpp` runtime behind a focused `pepperx-cleanup` crate so prompt assembly and model invocation stay out of the GTK shell. Store cleanup diagnostics alongside the existing transcript and insertion diagnostics, with raw transcript text remaining the top-level archived source of truth. Treat OCR as optional supporting text in the cleanup request shape, not a parallel product flow.

**Tech Stack:** Rust, Cargo workspace, GTK4/libadwaita, existing `sherpa-onnx` ASR path, real local `llama.cpp` cleanup backend, JSONL transcript log, existing GNOME insertion pipeline

---

## Chunk 1: Cleanup Archive And CLI Surface

### Task 1: Add failing cleanup archive and CLI tests

**Files:**
- Modify: `app/src/cli.rs`
- Modify: `app/src/main.rs`
- Modify: `app/src/transcript_log.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/window.rs`

- [ ] **Step 1: Write the failing cleanup tests**

Add tests that prove:
- Pepper X parses `--transcribe-wav-and-cleanup <path>`
- Pepper X can archive cleanup diagnostics while keeping `transcript_text` as the raw ASR result
- the CLI prints cleaned text when cleanup succeeded, without rewriting the raw archived transcript
- the History summary shows both the raw transcript and the cleaned transcript when cleanup data exists

- [ ] **Step 2: Run the targeted cleanup tests**

Run:
```sh
cd pepper-x
cargo test -p pepper-x-app cleanup_ -- --nocapture
```

Expected:
- the new cleanup assertions fail because Pepper X only archives raw transcripts today

- [ ] **Step 3: Implement the minimal cleanup archive model**

Implement:
- one optional cleanup diagnostics block on `TranscriptEntry`
- one new CLI mode for `--transcribe-wav-and-cleanup`
- one display helper that prefers cleaned text for CLI output when present
- one History summary update that surfaces raw-versus-cleaned text honestly

Do not invoke the real cleanup backend in this task yet. This task is only about the app-owned archive and UI surface.

- [ ] **Step 4: Re-run the targeted cleanup tests**

Run the command from Step 2.

Expected:
- the cleanup archive and CLI surface tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add app/src/cli.rs app/src/main.rs app/src/transcript_log.rs app/src/transcription.rs app/src/window.rs
git -C pepper-x commit -m "Model Pepper X cleanup archive surface"
```

## Chunk 2: Real `llama.cpp` Cleanup Runtime

### Task 2: Add the real cleanup backend and prerecorded cleanup smoke

**Files:**
- Modify: `Cargo.toml`
- Modify: `app/Cargo.toml`
- Create: `crates/pepperx-cleanup/Cargo.toml`
- Create: `crates/pepperx-cleanup/src/lib.rs`
- Create: `crates/pepperx-cleanup/src/cleanup.rs`
- Modify: `app/src/transcription.rs`
- Create: `tests/smoke/test_cleanup_pipeline.sh`

- [ ] **Step 1: Write the failing cleanup-runtime tests**

Add tests that prove:
- Pepper X rejects a missing cleanup model path
- the cleanup request preserves the raw ASR transcript and returns cleaned text separately
- the cleanup prompt stays deterministic for the same transcript input
- a cleanup backend failure falls back to the raw transcript path while recording cleanup failure diagnostics
- the cleanup smoke path writes cleaned output to stdout while the JSONL archive still keeps the raw transcript text

Keep one ignored real-backend test for the actual `llama.cpp` invocation.

- [ ] **Step 2: Run the targeted cleanup-runtime tests**

Run:
```sh
cd pepper-x
cargo test -p pepperx-cleanup cleanup_ -- --nocapture
cargo test -p pepper-x-app cleanup_ -- --nocapture
```

Expected:
- the cleanup-runtime assertions fail because no cleanup crate exists yet

- [ ] **Step 3: Implement the real cleanup backend**

Implement:
- a focused `pepperx-cleanup` crate that wraps the real `llama.cpp` runtime
- a cleanup request/response API with:
  - raw transcript text
  - optional OCR context text
  - cleanup backend/model metadata
  - cleaned text and elapsed time
  - explicit success/failure diagnostics
- app-owned orchestration for `transcribe_wav_and_cleanup_to_log`
- `PEPPERX_CLEANUP_MODEL_PATH` as the model locator for the real cleanup backend

If cleanup is unavailable or fails after ASR succeeds, continue with the raw transcript path and archive the cleanup failure instead of aborting the entire dictation run.

Keep the first cleanup prompt narrow:
- punctuation and capitalization
- obvious transcript cleanup
- no speculative rewrites or stylistic rewriting

- [ ] **Step 4: Re-run the targeted cleanup-runtime tests**

Run the commands from Step 2.

Expected:
- the cleanup crate tests pass
- the app cleanup tests pass

- [ ] **Step 5: Run the real-backend cleanup checks**

Run:
```sh
cd pepper-x
PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf cargo test -p pepperx-cleanup cleanup_real_ -- --ignored --nocapture
PEPPERX_PARAKEET_MODEL_DIR=/path/to/parakeet PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf tests/smoke/test_cleanup_pipeline.sh
```

Expected:
- the ignored real-backend cleanup test passes
- the prerecorded cleanup smoke passes and records both raw and cleaned transcript artifacts

- [ ] **Step 6: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add Cargo.toml app/Cargo.toml crates/pepperx-cleanup/Cargo.toml crates/pepperx-cleanup/src/lib.rs crates/pepperx-cleanup/src/cleanup.rs app/src/transcription.rs tests/smoke/test_cleanup_pipeline.sh
git -C pepper-x commit -m "Add Pepper X llama cleanup runtime"
```

## Chunk 3: Reuse Cleanup In The Existing Insertion Path

### Task 3: Route cleaned text through the friendly insertion path

**Files:**
- Modify: `app/src/cli.rs`
- Modify: `app/src/main.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/transcript_log.rs`
- Modify: `app/src/window.rs`
- Create: `scripts/smoke-insert-cleaned-friendly.sh`

- [ ] **Step 1: Write the failing cleanup-plus-insertion tests**

Add tests that prove:
- Pepper X parses `--transcribe-wav-and-cleanup-and-insert-friendly <path>`
- friendly insertion receives the cleaned text, not the raw ASR transcript
- the archive still stores the raw transcript separately from the cleaned transcript
- insertion diagnostics remain independent from cleanup diagnostics
- a friendly insertion failure still returns the insertion error even when cleanup succeeded

- [ ] **Step 2: Run the targeted cleanup-plus-insertion tests**

Run:
```sh
cd pepper-x
cargo test -p pepper-x-app cleanup_insert_ -- --nocapture
```

Expected:
- the new assertions fail because Pepper X can only insert raw transcripts today

- [ ] **Step 3: Implement cleanup-backed insertion reuse**

Implement:
- one app-owned orchestration path that runs ASR, cleanup, then the existing friendly insertion seam
- one CLI mode for `--transcribe-wav-and-cleanup-and-insert-friendly`
- one dedicated smoke helper that reuses the same GNOME Text Editor target from loop 2

Do not fork or duplicate the insertion backend logic. Reuse the loop-2/3/4 insertion pipeline.
If the insertion step fails, surface that insertion failure while preserving the cleanup diagnostics on the archived transcript entry.

- [ ] **Step 4: Re-run the targeted cleanup-plus-insertion tests**

Run the command from Step 2.

Expected:
- the cleanup-plus-insertion tests pass

- [ ] **Step 5: Run the live cleaned-insertion smoke**

Run inside a supported GNOME session:
```sh
cd pepper-x
PEPPERX_PARAKEET_MODEL_DIR=/path/to/parakeet PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf ./scripts/smoke-insert-cleaned-friendly.sh
```

Expected:
- the friendly target receives the cleaned transcript text
- the archive keeps both raw and cleaned transcript fields

- [ ] **Step 6: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add app/src/cli.rs app/src/main.rs app/src/transcription.rs app/src/transcript_log.rs app/src/window.rs scripts/smoke-insert-cleaned-friendly.sh
git -C pepper-x commit -m "Route Pepper X cleanup output into friendly insertion"
```

## Chunk 4: Optional OCR Context And Final Verification

### Task 4: Add the OCR-text seam without broadening into a separate OCR product

**Files:**
- Modify: `crates/pepperx-cleanup/src/cleanup.rs`
- Modify: `crates/pepperx-cleanup/src/lib.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/transcript_log.rs`

- [ ] **Step 1: Write the failing OCR-context tests**

Add tests that prove:
- cleanup prompt assembly omits OCR text when no OCR context is present
- cleanup prompt assembly includes bounded OCR text when OCR context is provided
- cleanup diagnostics record whether OCR context contributed to the cleanup request

- [ ] **Step 2: Run the targeted OCR-context tests**

Run:
```sh
cd pepper-x
cargo test -p pepperx-cleanup cleanup_ocr_ -- --nocapture
cargo test -p pepper-x-app cleanup_ocr_ -- --nocapture
```

Expected:
- the OCR-context assertions fail because cleanup currently ignores OCR input entirely

- [ ] **Step 3: Implement the optional OCR-text seam**

Implement:
- one optional OCR text field on the cleanup request
- prompt assembly that treats OCR text as bounded supporting context, not as the primary source
- archive diagnostics that record whether OCR context was used

Do not implement full live screen capture in this loop. The loop-5 cleanup path only needs the OCR text seam so later GNOME capture work can plug into it.

- [ ] **Step 4: Re-run the targeted OCR-context tests**

Run the commands from Step 2.

Expected:
- the OCR-context tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add crates/pepperx-cleanup/src/cleanup.rs crates/pepperx-cleanup/src/lib.rs app/src/transcription.rs app/src/transcript_log.rs
git -C pepper-x commit -m "Add Pepper X cleanup OCR context seam"
```

### Task 5: Document and verify the full loop-5 surface

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/plans/2026-03-28-pepper-x-iterative-ai-loops.md`

- [ ] **Step 1: Update the docs**

Document:
- the real cleanup backend requirement
- the raw-versus-cleaned archive shape
- the fact that OCR is optional supporting context, not a separate mode
- the friendly insertion path now consuming cleaned text when the cleanup CLI path is used

- [ ] **Step 2: Run the exact loop-5 verification set**

Run:
```sh
cd pepper-x
cargo fmt --check
cargo test -p pepperx-cleanup cleanup_ -- --nocapture
cargo test -p pepper-x-app cleanup_ -- --nocapture
cargo test -p pepper-x-app cleanup_insert_ -- --nocapture
cargo check --workspace
cargo test --workspace
PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf cargo test -p pepperx-cleanup cleanup_real_ -- --ignored --nocapture
PEPPERX_PARAKEET_MODEL_DIR=/path/to/parakeet PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf tests/smoke/test_cleanup_pipeline.sh
PEPPERX_PARAKEET_MODEL_DIR=/path/to/parakeet PEPPERX_CLEANUP_MODEL_PATH=/path/to/model.gguf ./scripts/smoke-insert-cleaned-friendly.sh
```

Expected:
- the workspace checks pass
- the cleanup runtime checks pass
- the prerecorded cleanup smoke passes
- the cleaned friendly-insertion smoke passes on a real GNOME Wayland session

- [ ] **Step 3: Confirm the tree is ready for the next loop**

```bash
git -C pepper-x status --short
```

Expected:
- the working tree is clean because the loop-5 commits already landed
