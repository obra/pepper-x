# Pepper X Loop 2 Friendly-App Insertion Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the loop-1 transcript path so Pepper X can insert a freshly transcribed prerecorded WAV into the currently focused GNOME Text Editor document and record the insertion result.

**Architecture:** Reuse the loop-1 transcription/archive pipeline and add one GNOME-specific semantic insertion backend in the platform crate. Keep the scope to GNOME Text Editor plus `EditableText.insert_text`, fail fast for every other target, and persist only minimal insertion diagnostics needed to make the result inspectable.

**Tech Stack:** Rust, Cargo workspace, GTK4/libadwaita app shell, AT-SPI/libatspi, GNOME Text Editor, JSON Lines transcript archive, shell-based real-session smoke checks

---

## File Structure

**Repository root:** `pepper-x/`

**Create:**
- `scripts/smoke-insert-friendly.sh`
  - Real-session GNOME Text Editor smoke for the loop-2 insertion path.

**Modify:**
- `app/src/cli.rs`
  - Add a narrow dev entrypoint for `--transcribe-wav-and-insert-friendly <path>`.
- `app/src/main.rs`
  - Dispatch the new friendly-insert startup mode.
- `app/src/transcription.rs`
  - Reuse the loop-1 transcript path, then invoke the friendly-target insertion backend and persist the insertion result.
- `app/src/transcript_log.rs`
  - Extend transcript entries with minimal insertion diagnostics for loop 2.
- `app/src/window.rs`
  - Surface the latest friendly-target insertion outcome in the History summary without building a broader diagnostics UI.
- `crates/pepperx-platform-gnome/src/lib.rs`
  - Export the friendly-target insertion surface.
- `crates/pepperx-platform-gnome/src/atspi.rs`
  - Add focused-object lookup, GNOME Text Editor validation, and `EditableText.insert_text` insertion.
- `README.md`
  - Document the loop-2 dev entrypoint and the real-session smoke helper.

**Test:**
- `app/src/cli.rs`
- `app/src/transcript_log.rs`
- `app/src/window.rs`
- `crates/pepperx-platform-gnome/src/atspi.rs`

---

## Chunk 1: Friendly Target Backend

### Task 1: Add failing tests for friendly-target validation

**Files:**
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`

- [ ] **Step 1: Write the failing focused-target tests**

Add tests that prove:
- Pepper X rejects a focused target whose application is not GNOME Text Editor
- Pepper X rejects a focused target that is not editable
- Pepper X reports a stable backend name and failure reason for the friendly-target path

Use a tiny pure seam for testability:
- a focused-target struct with just the fields needed for loop 2
- a pure validator or selector that decides whether loop-2 insertion may proceed

Keep this seam narrow:
- no generic target taxonomy yet
- no clipboard fallback
- no string injection fallback
- no `uinput`

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:
```sh
cd pepper-x
cargo test -p pepperx-platform-gnome friendly_insert_ -- --nocapture
```

Expected:
- the tests fail because the loop-2 friendly-target validation path does not exist yet

- [ ] **Step 3: Implement the minimal friendly-target validation**

Implement:
- one backend identifier for the semantic friendly-target path
- one focused-target model with only the fields loop 2 needs
- fail-fast checks for:
  - non-GNOME Text Editor app
  - non-editable target
  - missing caret/edit surface

Do not:
- generalize beyond GNOME Text Editor
- add fallback ordering
- add cross-toolkit heuristics

- [ ] **Step 4: Re-run the targeted tests**

Run the command from Step 2.

Expected:
- the friendly-target validation tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add crates/pepperx-platform-gnome/src/atspi.rs
git -C pepper-x commit -m "Add Pepper X friendly target validation"
```

### Task 2: Add the real GNOME Text Editor insertion path

**Files:**
- Modify: `crates/pepperx-platform-gnome/src/lib.rs`
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`
- Create: `scripts/smoke-insert-friendly.sh`

- [ ] **Step 1: Write the failing real-session smoke**

Create `scripts/smoke-insert-friendly.sh`.

The smoke helper should:
- require a live GNOME 48+ Wayland session
- require GNOME Text Editor to be installed
- require the caller to have an active focused Text Editor document
- fail before implementation because Pepper X has no friendly-target insertion path yet

Keep the script honest:
- no fake target
- no nested fallback path
- no modifier-capture dependency

- [ ] **Step 2: Run the real-session smoke to confirm failure**

Run inside a live supported GNOME session:
```sh
cd pepper-x
./scripts/smoke-insert-friendly.sh
```

Expected:
- the helper fails because the insertion backend is not implemented yet

- [ ] **Step 3: Implement GNOME Text Editor insertion**

Implement in `crates/pepperx-platform-gnome/src/atspi.rs`:
- focused accessible-object lookup for the current target
- validation that the focused object belongs to GNOME Text Editor
- `EditableText.insert_text` insertion at the caret
- a readback or other scriptable verification seam that lets the smoke helper prove the insertion actually happened

The implementation must:
- insert text rather than replace the whole field
- preserve the friendly-target backend name in the result
- return a stable error when the focused target is not acceptable

Do not:
- add string injection
- add clipboard paste
- add `uinput`

- [ ] **Step 4: Re-run the real-session smoke**

Run the command from Step 2.

Expected:
- the helper passes on a real GNOME 48+ Wayland session with GNOME Text Editor focused

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add crates/pepperx-platform-gnome/src/lib.rs crates/pepperx-platform-gnome/src/atspi.rs scripts/smoke-insert-friendly.sh
git -C pepper-x commit -m "Add Pepper X GNOME Text Editor insertion backend"
```

---

## Chunk 2: Transcript Pipeline And Diagnostics

### Task 3: Add failing tests for the loop-2 entrypoint and archive diagnostics

**Files:**
- Modify: `app/src/cli.rs`
- Modify: `app/src/main.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/transcript_log.rs`
- Modify: `app/src/window.rs`

- [ ] **Step 1: Write the failing loop-2 tests**

Add tests that prove:
- Pepper X parses a stable `--transcribe-wav-and-insert-friendly <path>` mode
- loop-2 orchestration reuses the loop-1 transcript path and records insertion diagnostics on the transcript entry
- the History summary can show the latest insertion result without expanding into a full diagnostics UI

Record only the minimum insertion diagnostics needed for loop 2:
- insertion backend name
- target application name
- success or failure result
- failure reason when insertion fails

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:
```sh
cd pepper-x
cargo test -p pepper-x-app cli_mode -- --nocapture
cargo test -p pepper-x-app transcript_log -- --nocapture
cargo test -p pepper-x-app app_shell -- --nocapture
```

Expected:
- the new loop-2 assertions fail because Pepper X does not yet expose the friendly insert mode or archive the insertion outcome

- [ ] **Step 3: Implement the smallest loop-2 orchestration**

Implement:
- the new CLI/dev entrypoint
- one orchestration function that:
  - transcribes the WAV through the loop-1 path
  - invokes the friendly-target insertion backend
  - appends the insertion diagnostics to the same transcript entry
- a minimal History summary update for the latest insertion result

Keep this intentionally narrow:
- no selection among multiple insertion backends
- no generalized insertion target matrix
- no cleanup logic
- no history browser beyond the existing summary

- [ ] **Step 4: Re-run the targeted tests**

Run the command from Step 2.

Expected:
- the loop-2 CLI, archive, and History tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add app/src/cli.rs app/src/main.rs app/src/transcription.rs app/src/transcript_log.rs app/src/window.rs
git -C pepper-x commit -m "Wire Pepper X friendly insertion flow"
```

### Task 4: Document the loop-2 surface

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the README**

Document:
- the new loop-2 dev entrypoint
- the requirement to run the smoke inside a live GNOME 48+ Wayland session
- the GNOME Text Editor-only scope for this loop

Do not document:
- broad accessible-app support
- fallback insertion promises
- cleanup behavior

- [ ] **Step 2: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add README.md
git -C pepper-x commit -m "Document Pepper X friendly insertion loop"
```

---

## Chunk 3: Verification

### Task 5: Run the exact loop-2 verification set

**Files:**
- Test: `crates/pepperx-platform-gnome/src/atspi.rs`
- Test: `app/src/cli.rs`
- Test: `app/src/transcript_log.rs`
- Test: `app/src/window.rs`
- Test: `scripts/smoke-insert-friendly.sh`

- [ ] **Step 1: Run formatting and targeted tests**

Run:
```sh
cd pepper-x
cargo fmt --check
cargo test -p pepperx-platform-gnome friendly_insert_ -- --nocapture
cargo test -p pepper-x-app cli_mode -- --nocapture
cargo test -p pepper-x-app transcript_log -- --nocapture
cargo test -p pepper-x-app app_shell -- --nocapture
```

Expected:
- the targeted loop-2 tests pass

- [ ] **Step 2: Run workspace validation**

Run:
```sh
cd pepper-x
cargo check --workspace
cargo test --workspace
```

Expected:
- the workspace builds and tests pass with the loop-2 insertion backend included

- [ ] **Step 3: Run the real-session friendly-target smoke**

Run inside a supported GNOME session:
```sh
cd pepper-x
./scripts/smoke-insert-friendly.sh
```

Expected:
- GNOME Text Editor receives the transcribed fixture text
- Pepper X records the insertion diagnostics on the matching transcript entry

- [ ] **Step 4: Confirm the tree is ready for the next loop**

```bash
git -C pepper-x status --short
```

Expected:
- the working tree is clean because the earlier commits already landed
