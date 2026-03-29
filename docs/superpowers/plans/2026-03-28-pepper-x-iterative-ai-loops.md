# Pepper X Iterative AI Loops Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver Pepper X's AI stack in small, testable loops that move from logged local dictation to reliable insertion and only then into cleanup.

**Architecture:** Keep Pepper X app-first and avoid inventing a coordinator crate before the seams justify it. The real ASR backend starts in loop 1 and stays in place for later live microphone capture, so the only simplification in the first loop is the input source: prerecorded WAV instead of live audio. Raw transcription, insertion, cleanup, and diagnostics stay separate so each loop has a clear success condition and failures remain attributable to one subsystem at a time.

**Tech Stack:** Rust, Cargo workspace, GTK4, libadwaita, `sherpa-onnx`, Parakeet NeMo models, AT-SPI, D-Bus, clipboard mediation, `uinput` fallback, `llama.cpp`, local OCR

---

## File Structure

**Planning docs to keep current:**
- `docs/superpowers/plans/2026-03-28-pepper-x-iterative-ai-loops.md`
  - Cross-loop roadmap, guardrails, and acceptance criteria.
- `docs/superpowers/plans/2026-03-28-pepper-x-loop-1-dictation-logged.md`
  - Executable plan for the current loop.
- `docs/architecture/text-insertion-strategy.md`
  - Long-term insertion backend order and target-class expectations.

**Primary code areas for loop 1:**
- `Cargo.toml`
  - Workspace membership as focused AI crates are added.
- `app/src/main.rs`
  - Entry-mode selection for GUI, development diagnostics, and later rerun paths.
- `app/src/app.rs`
  - App composition root; owns wiring, not model or insertion internals.
- `app/src/window.rs`
  - Minimal user-visible history and diagnostics surfaces.
- `app/src/cli.rs`
  - Non-GUI loop entrypoints such as prerecorded-WAV transcription.
- `app/src/transcription.rs`
  - App-owned orchestration around ASR, transcript logging, and later insertion.
- `app/src/transcript_log.rs`
  - Append-only raw transcript archive before later history/rerun work expands it.
- `crates/pepperx-asr/src/`
  - Focused ASR subsystem with the real `sherpa-onnx` backend.
- `tests/`
  - Fast targeted tests plus thin end-to-end smoke coverage.

**Later-loop surfaces, only when their loop plans are active:**
- `crates/pepperx-platform-gnome/src/`
  - GNOME-specific capture and insertion coordination.
- helper/service code for `uinput`
  - Hostile-target fallback insertion only if text-oriented paths prove insufficient.
- cleanup/OCR modules
  - `llama.cpp`, OCR context, and later diagnostics surfaces.

---

## Chunk 1: Loop Boundaries

### Task 1: Lock the sequence and stop scope drift

**Files:**
- Modify: `docs/superpowers/plans/2026-03-28-pepper-x-iterative-ai-loops.md`
- Reference: `docs/superpowers/specs/2026-03-27-pepper-x-design.md`
- Reference: `docs/architecture/text-insertion-strategy.md`

- [ ] **Step 1: Keep loop 1 about logged dictation only**

Loop 1 exits only when Pepper X can take a prerecorded WAV, run the real local ASR backend, persist a raw transcript artifact, and surface that result through a stable Pepper X entrypoint.

- [ ] **Step 2: Keep loop 2 about one friendly insertion target**

Loop 2 exits only when Pepper X can take a raw transcript from the loop-1 pipeline and insert it into GNOME Text Editor on a real GNOME Wayland session.

- [ ] **Step 3: Keep loop 3 about common accessible targets**

Loop 3 exits only when Pepper X can reuse the same insertion pipeline for common GTK, browser, and standard accessible app targets through text-oriented backends.

- [ ] **Step 4: Keep loop 4 about broad insertion with explicit fallbacks**

Loop 4 exits only when Pepper X can classify hostile targets and use fallback paths honestly, including clipboard mediation and a Pepper X-owned `uinput` daemon if still required.

- [ ] **Step 5: Keep cleanup after raw insertion is stable**

Loop 5 exits only when Pepper X can take a raw transcript plus optional OCR/corrections context through the cleanup path without blurring responsibility for insertion failures.

---

## Chunk 2: Execution Roadmap

### Task 2: Define loop 1 as the first executable vertical slice

**Files:**
- Create: `docs/superpowers/plans/2026-03-28-pepper-x-loop-1-dictation-logged.md`
- Create later: `crates/pepperx-asr/Cargo.toml`
- Create later: `crates/pepperx-asr/src/lib.rs`
- Create later: `crates/pepperx-asr/src/transcriber.rs`
- Modify later: `Cargo.toml`
- Modify later: `app/Cargo.toml`
- Create later: `app/src/cli.rs`
- Create later: `app/src/transcription.rs`
- Create later: `app/src/transcript_log.rs`
- Modify later: `app/src/main.rs`
- Modify later: `app/src/app.rs`
- Modify later: `app/src/window.rs`
- Create later: `tests/fixtures/loop1-hello.wav`
- Create later: `tests/smoke/test_prerecorded_asr.sh`

- [ ] **Step 1: Start with failing loop-1 checks**

The loop-1 plan must begin with failing tests for:
- transcript-log persistence
- ASR input/model validation
- CLI parsing for `--transcribe-wav <path>`
- app history rendering
- an env-gated real-backend smoke using a fixed WAV fixture

- [ ] **Step 2: Use the real ASR backend immediately**

Loop 1 should use `sherpa-onnx` with a Parakeet NeMo model, not a fake transcriber and not a temporary backend Jesse will have to rip out later.

- [ ] **Step 3: Simplify input source only**

Use prerecorded WAV input for loop 1. Do not add live microphone capture, VAD, device selection, insertion, or cleanup yet.

- [ ] **Step 4: Persist artifacts from the first successful transcript**

Log enough information to make the result inspectable:
- source WAV path
- transcript text
- backend/model metadata
- timing information
- failure reason if transcription fails

- [ ] **Step 5: Surface the archive through stable entrypoints**

Loop 1 should expose the raw transcript through:
- a headless CLI/dev path
- the existing History page in the app shell

### Task 3: Define loop 2 as friendly-app insertion

**Files:**
- Modify later: `app/src/transcription.rs`
- Modify later: `app/src/transcript_log.rs`
- Modify later: `app/src/window.rs`
- Modify later: `crates/pepperx-platform-gnome/src/`
- Modify later: `tests/smoke/`

- [ ] **Step 1: Start with a failing friendly-target insertion check**

The loop-2 plan must begin with:
- a targeted insertion-selection test such as `cargo test -p pepper-x-app insertion_friendly_ -- --nocapture`
- a real-session smoke such as `./scripts/smoke-insert-friendly.sh`

Expected before implementation:
- the targeted test or the GNOME Text Editor smoke fails because no insertion path exists yet

- [ ] **Step 2: Reuse the loop-1 transcript path**

Do not fork the transcription pipeline for insertion. Loop 2 should consume the same raw transcript object loop 1 persists.

- [ ] **Step 3: Target GNOME Text Editor first**

Use one friendly, GNOME-native insertion target to verify the insertion seam before broadening target coverage.

- [ ] **Step 4: Preserve insertion diagnostics**

Record the chosen insertion backend, target metadata, and success/failure result as minimal optional fields on the transcript-log entry for the single friendly-target loop. Revisit a separate diagnostics store only if loop 3 or loop 4 proves the schema is getting noisy.

- [ ] **Step 5: End with the same checks green**

Loop 2 is not complete until the targeted insertion test and `./scripts/smoke-insert-friendly.sh` both pass on a real GNOME Wayland session.

### Task 4: Define loop 3 as common accessible insertion

**Files:**
- Modify later: `docs/architecture/text-insertion-strategy.md`
- Modify later: `app/src/transcription.rs`
- Modify later: `app/src/transcript_log.rs`
- Modify later: `crates/pepperx-platform-gnome/src/`
- Modify later: `tests/smoke/`

- [ ] **Step 1: Start with failing target-class checks**

The loop-3 plan must begin with:
- targeted tests such as `cargo test -p pepper-x-app insertion_accessible_ -- --nocapture`
- smoke coverage for a GTK target and a browser textarea target

Expected before implementation:
- at least one target-class smoke fails because Pepper X still only handles the friendly loop-2 target

- [ ] **Step 2: Keep text-oriented backends first**

Expand to semantic accessibility insertion and AT-SPI string injection for common accessible targets before adding hostile-target fallbacks.

- [ ] **Step 3: Add target classification**

Track enough target metadata to distinguish GTK/libadwaita, browser text controls, ordinary Qt widgets, and ambiguous/custom targets.

- [ ] **Step 4: Keep the result honest**

Success criteria are "works on the supported target classes we claimed," not "works everywhere."

- [ ] **Step 5: End with the target-class checks green**

Loop 3 is not complete until the targeted insertion tests and the declared target-class smokes pass.

### Task 5: Define loop 4 as fallback-backed broad insertion

**Files:**
- Modify later: `app/src/transcription.rs`
- Modify later: `app/src/transcript_log.rs`
- Create later: `crates/pepperx-platform-gnome/src/uinput.rs` or an equivalent focused helper
- Create later: `packaging/` install assets for the helper if needed
- Modify later: `tests/smoke/`

- [ ] **Step 1: Start with failing hostile-target checks**

The loop-4 plan must begin with:
- targeted fallback-selection tests such as `cargo test -p pepper-x-app insertion_fallback_ -- --nocapture`
- hostile-target smoke coverage such as `./scripts/smoke-insert-hostile.sh`

Expected before implementation:
- hostile targets still fail or stop at the last text-oriented backend

- [ ] **Step 2: Keep `uinput` as the last fallback**

Do not make raw keyboard emulation the default insertion path.

- [ ] **Step 3: Make the fallback explicit**

If Pepper X needs a privileged helper, keep it tiny, Pepper X-owned, and responsible for text injection only.

- [ ] **Step 4: Treat hostile targets as a named class**

Terminals, custom-rendered apps, Xwayland-era apps, and Wine are fallback-heavy targets and need their own smoke coverage.

- [ ] **Step 5: End with the hostile-target checks green**

Loop 4 is not complete until the fallback-selection tests and hostile-target smoke coverage pass for the target classes we claim.

### Task 6: Define loop 5 as cleanup after insertion

**Files:**
- Create later: cleanup modules under `app/src/` or a focused cleanup crate if the coordination surface justifies extraction
- Modify later: `app/src/transcription.rs`
- Modify later: `app/src/transcript_log.rs`
- Create later: OCR/context modules under `crates/pepperx-platform-gnome/src/` or a focused runtime crate
- Modify later: `app/src/window.rs`
- Modify later: `docs/architecture/text-insertion-strategy.md`

- [ ] **Step 1: Start with failing cleanup checks**

The loop-5 plan must begin with:
- targeted cleanup tests such as `cargo test -p pepper-x-app cleanup_ -- --nocapture`
- at least one smoke that compares raw transcript output to cleaned output without changing the insertion target

Expected before implementation:
- cleanup paths are missing or fail deterministically because the runtime still surfaces raw transcripts only

- [ ] **Step 2: Keep raw transcript and cleaned transcript separate**

Pepper X must retain the uncleaned ASR output so cleanup mistakes are diagnosable and reversible.

- [ ] **Step 3: Add cleanup only after insertion works**

Cleanup should improve inserted text, not mask insertion bugs or ASR bugs.

- [ ] **Step 4: Introduce OCR as supporting context only**

OCR is there to improve cleanup confidence, not to become a separate feature track.

- [ ] **Step 5: End with raw-versus-cleaned checks green**

Loop 5 is not complete until the cleanup tests and the raw-versus-cleaned smoke checks pass while preserving raw transcript artifacts.

Current loop-5 surface:
- prerecorded WAV cleanup via `--transcribe-wav-and-cleanup`
- cleaned friendly insertion via `--transcribe-wav-and-cleanup-and-insert-friendly`
- raw transcript archived separately from `cleanup.cleaned_text`
- optional OCR text carried only as bounded cleanup-supporting context
- insertion failures surfaced after the archive is written, not hidden by cleanup success

---

## Chunk 3: Cross-Loop Guardrails

### Task 7: Keep the architecture honest while moving fast

**Files:**
- Modify: `docs/superpowers/plans/2026-03-28-pepper-x-iterative-ai-loops.md`

- [ ] **Step 1: Keep GNOME/platform code thin**

The app owns product/runtime logic. GNOME integration code should only handle GNOME-facing seams.

- [ ] **Step 2: Keep TDD real**

Every loop needs a failing test before production code, including any real-session smoke that proves a desktop seam.

- [ ] **Step 3: Commit after each finished slice**

Each loop should land as a small set of reviewable commits, not one giant catch-all diff.

- [ ] **Step 4: Preserve future ARM room without promising it yet**

Avoid needless `x86_64` assumptions in the AI runtime code, but treat official packaging and support claims as a separate decision from loop execution.

- [ ] **Step 5: Keep the manual GNOME validation list short and real**

Only require live-session manual checks for seams automation cannot honestly prove.
