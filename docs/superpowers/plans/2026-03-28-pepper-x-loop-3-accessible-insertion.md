# Pepper X Loop 3 Common Accessible Insertion Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish loop 3 by carrying the already-landed accessible insertion target classes through Pepper X's app diagnostics and by validating the declared GNOME Text Editor and browser textarea surface.

**Architecture:** Loop 3 already has the platform-side selector and live smoke helper in place. The remaining work is app-owned: record the selected target class on transcript entries, surface it in the History summary, document the new target classes honestly, and rerun the exact loop-3 checks without broadening into clipboard or `uinput` fallback behavior.

**Tech Stack:** Rust, Cargo workspace, GTK4/libadwaita, AT-SPI/libatspi, GNOME Wayland, Firefox browser textareas, JSON Lines transcript archive

---

## Chunk 1: App Diagnostics

### Task 1: Record the target class on loop-3 transcript entries

**Files:**
- Modify: `app/src/transcript_log.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/window.rs`

- [ ] **Step 1: Write the failing loop-3 diagnostics tests**

Add tests that prove:
- Pepper X records the supported target class on a successful loop-3 insertion
- Pepper X preserves the existing loop-2 diagnostics for GNOME Text Editor
- the History summary can show the latest target class without turning into a diagnostics browser

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

## Chunk 2: Documentation

### Task 2: Document the loop-3 surface

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

### Task 3: Run the exact loop-3 verification set

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
