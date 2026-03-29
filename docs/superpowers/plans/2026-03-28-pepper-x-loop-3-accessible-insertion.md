# Pepper X Loop 3 Common Accessible Insertion Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend loop 2 so Pepper X can insert a prerecorded-WAV transcript into common accessible targets beyond GNOME Text Editor while keeping the insertion path text-oriented and honest.

**Architecture:** Reuse the loop-2 transcript pipeline, keep the app first, and expand only the GNOME platform insertion selector. Loop 3 broadens semantic AT-SPI insertion from one allowlisted app to a small declared class of common accessible targets, records the chosen target class, and refuses everything else without inventing clipboard or `uinput` fallback behavior yet.

**Tech Stack:** Rust, Cargo workspace, GTK4/libadwaita, AT-SPI/libatspi, GNOME Wayland, Firefox or Chromium textareas for browser smoke, JSON Lines transcript archive

---

## Chunk 1: Multi-Target Selection

### Task 1: Add failing classification tests for common accessible targets

**Files:**
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`

- [ ] **Step 1: Write the failing classification tests**

Add narrow pure tests that prove:
- GNOME Text Editor still selects the semantic AT-SPI insertion backend
- a browser textarea target class can select the same semantic backend without an app-specific allowlist
- an ambiguous target without the required editable/caret surface still fails fast
- unsupported targets stay rejected

Use a tiny selector seam only:
- target application identity key derived from runtime metadata
- target class enum with only the loop-3 classes you are actually shipping
- no fallback chain yet

- [ ] **Step 2: Run the targeted selector tests**

Run:
```sh
cd pepper-x
cargo test -p pepperx-platform-gnome accessible_insert_selection -- --nocapture
```

Expected:
- the new selector tests fail because loop 2 still only accepts GNOME Text Editor

- [ ] **Step 3: Implement the minimal selector expansion**

Implement:
- one small target-class enum for loop 3
- classification from focused-target metadata into:
  - GNOME Text Editor
  - browser textarea
  - unsupported
- semantic insertion selection for the supported classes only

Do not add:
- clipboard fallback
- `uinput`
- generic app heuristics beyond the declared classes

- [ ] **Step 4: Re-run the targeted selector tests**

Run the command from Step 2.

Expected:
- the loop-3 selector tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add crates/pepperx-platform-gnome/src/atspi.rs
git -C pepper-x commit -m "Expand Pepper X accessible target selection"
```

### Task 2: Add a real-session browser textarea smoke

**Files:**
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`
- Create: `scripts/smoke-insert-accessible.sh`

- [ ] **Step 1: Write the failing live smoke helper**

Create `scripts/smoke-insert-accessible.sh`.

The helper should:
- require a live GNOME 48+ Wayland session
- require a focused supported target before running
- support exactly two declared smoke modes:
  - `text-editor`
  - `browser-textarea`
- run an exact ignored test for the requested target class

Keep the helper honest:
- do not fake a browser target
- do not route through the loop-4 fallback path
- do not use clipboard or `uinput`

- [ ] **Step 2: Run the live smoke helper to confirm failure**

Run inside a live supported GNOME session:
```sh
cd pepper-x
./scripts/smoke-insert-accessible.sh browser-textarea
```

Expected:
- the browser smoke fails because loop 2 still only supports GNOME Text Editor

- [ ] **Step 3: Implement browser textarea insertion through the semantic backend**

Implement in `crates/pepperx-platform-gnome/src/atspi.rs`:
- focused-target classification for browser textareas
- reuse of the existing semantic insertion and readback path
- one ignored live test for browser textarea insertion

The implementation must:
- keep GNOME Text Editor working
- share as much of the insertion path as possible with loop 2
- reject unsupported browser surfaces instead of pretending they are editable

- [ ] **Step 4: Re-run the live smoke helper**

Run:
```sh
cd pepper-x
./scripts/smoke-insert-accessible.sh text-editor
./scripts/smoke-insert-accessible.sh browser-textarea
```

Expected:
- both declared loop-3 live smokes pass in a real GNOME Wayland session

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add crates/pepperx-platform-gnome/src/atspi.rs scripts/smoke-insert-accessible.sh
git -C pepper-x commit -m "Add Pepper X browser textarea insertion smoke"
```

## Chunk 2: App Diagnostics

### Task 3: Record the target class on loop-3 transcript entries

**Files:**
- Modify: `app/src/transcript_log.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/window.rs`

- [ ] **Step 1: Write the failing loop-3 diagnostics tests**

Add tests that prove:
- Pepper X records the supported target class on a successful loop-3 insertion
- Pepper X preserves the existing loop-2 diagnostics for GNOME Text Editor
- the History summary can show the latest target class without turning into a full diagnostics browser

- [ ] **Step 2: Run the targeted app tests**

Run:
```sh
cd pepper-x
cargo test -p pepper-x-app transcript_log -- --nocapture
cargo test -p pepper-x-app app_shell -- --nocapture
```

Expected:
- the new target-class assertions fail because loop 2 does not record or render target classes yet

- [ ] **Step 3: Implement the smallest diagnostics update**

Implement:
- one optional target-class field on the insertion diagnostics object
- loop-3 orchestration updates that set the target class from the selected insertion outcome
- one small History summary addition for the latest target class

Do not add:
- a new diagnostics store
- a history details page
- a fallback-chain UI

- [ ] **Step 4: Re-run the targeted app tests**

Run the commands from Step 2.

Expected:
- the transcript-log and History tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add app/src/transcript_log.rs app/src/transcription.rs app/src/window.rs
git -C pepper-x commit -m "Record Pepper X accessible target classes"
```

### Task 4: Document the loop-3 surface

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the README**

Document:
- the declared loop-3 target classes
- the new browser textarea smoke helper
- the fact that loop 3 is still semantic insertion only

Do not document:
- clipboard promises
- terminal support
- Wine support
- `uinput`

- [ ] **Step 2: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add README.md
git -C pepper-x commit -m "Document Pepper X accessible insertion loop"
```

## Chunk 3: Verification

### Task 5: Run the exact loop-3 verification set

**Files:**
- Test: `crates/pepperx-platform-gnome/src/atspi.rs`
- Test: `app/src/transcript_log.rs`
- Test: `app/src/window.rs`
- Test: `scripts/smoke-insert-accessible.sh`

- [ ] **Step 1: Run formatting and targeted tests**

Run:
```sh
cd pepper-x
cargo fmt --check
cargo test -p pepperx-platform-gnome accessible_insert_ -- --nocapture
cargo test -p pepper-x-app transcript_log -- --nocapture
cargo test -p pepper-x-app app_shell -- --nocapture
```

Expected:
- the loop-3 targeted tests pass

- [ ] **Step 2: Run workspace validation**

Run:
```sh
cd pepper-x
cargo check --workspace
cargo test --workspace
```

Expected:
- the workspace still builds and tests cleanly

- [ ] **Step 3: Run the real-session smokes**

Run inside a supported GNOME session:
```sh
cd pepper-x
./scripts/smoke-insert-accessible.sh text-editor
./scripts/smoke-insert-accessible.sh browser-textarea
```

Expected:
- the declared loop-3 target-class smokes pass on a real GNOME Wayland session

- [ ] **Step 4: Confirm the tree is ready for loop 4**

```bash
git -C pepper-x status --short
```

Expected:
- the working tree is clean because the earlier commits already landed
