# Pepper X Loop 4 Fallback-Backed Insertion Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend Pepper X from loop-3 semantic accessible insertion into a declared fallback-backed hostile-target path that can handle terminal-style and other weak-accessibility targets honestly.

**Architecture:** Keep the default insertion path text-oriented and app-owned. Loop 4 adds an explicit fallback chain in this order: semantic accessibility, AT-SPI string injection, clipboard-assisted paste, and finally a tiny Pepper X-owned `uinput` helper for hostile targets that still reject every text-oriented path. Diagnostics must record both the selected backend and the attempted fallback chain.

**Tech Stack:** Rust, Cargo workspace, GTK4/libadwaita, AT-SPI/libatspi, GNOME Wayland, Firefox/GTK loop-3 paths, clipboard mediation, `uinput`, a Pepper X-owned helper daemon, terminal smokes

---

## Chunk 1: Fallback Selection

### Task 1: Add failing fallback-selection tests

**Files:**
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`
- Modify: `app/src/transcript_log.rs`

- [ ] **Step 1: Write the failing fallback-selection tests**

Add tests that prove:
- Pepper X prefers semantic insertion when the focused target still supports it
- terminal-style targets fall through semantic insertion to a string-injection candidate
- clipboard mediation is selected only after text-oriented paths fail
- `uinput` remains the last fallback for hostile targets

Record the fallback chain as data:
- chosen backend
- ordered attempted backends
- target class

- [ ] **Step 2: Run the targeted fallback tests**

Run:
```sh
cd pepper-x
cargo test -p pepperx-platform-gnome fallback_insert_ -- --nocapture
cargo test -p pepper-x-app transcript_log -- --nocapture
```

Expected:
- the new fallback-selection assertions fail because loop 3 still stops at semantic insertion only

- [ ] **Step 3: Implement the minimal fallback-selection model**

Implement:
- one tiny backend enum for loop 4
- one ordered fallback-chain representation
- selection logic that keeps semantic insertion first and `uinput` last

Do not implement the backends themselves yet in this task.

- [ ] **Step 4: Re-run the targeted fallback tests**

Run the commands from Step 2.

Expected:
- the fallback-selection tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add crates/pepperx-platform-gnome/src/atspi.rs app/src/transcript_log.rs
git -C pepper-x commit -m "Model Pepper X insertion fallback selection"
```

## Chunk 2: Text-Oriented Fallbacks

### Task 2: Add AT-SPI string injection for terminal-style targets

**Files:**
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`
- Create: `scripts/smoke-insert-terminal.sh`

- [ ] **Step 1: Write the failing terminal live test and smoke helper**

Add:
- one ignored live test for `xterm` or another declared terminal smoke target
- `scripts/smoke-insert-terminal.sh` that drives the exact ignored terminal live test

Keep the scope narrow:
- one declared terminal class
- AT-SPI string injection only
- no clipboard or `uinput` in this task

- [ ] **Step 2: Run the terminal smoke helper to confirm failure**

Run inside a supported GNOME session:
```sh
cd pepper-x
./scripts/smoke-insert-terminal.sh
```

Expected:
- the terminal smoke fails because loop 3 does not have a terminal insertion path

- [ ] **Step 3: Implement the string-injection backend**

Implement:
- AT-SPI string-injection fallback
- target classification for the declared terminal class
- readback or another scriptable verification seam for the terminal smoke

The implementation must:
- leave loop-3 semantic targets unchanged
- refuse targets that do not plausibly accept string injection
- record string injection as the chosen backend when it succeeds

- [ ] **Step 4: Re-run the terminal smoke helper**

Run the command from Step 2.

Expected:
- the declared terminal smoke passes on a real GNOME Wayland session

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add crates/pepperx-platform-gnome/src/atspi.rs scripts/smoke-insert-terminal.sh
git -C pepper-x commit -m "Add Pepper X terminal string injection fallback"
```

### Task 3: Add clipboard-mediated paste before raw input fallback

**Files:**
- Modify: `crates/pepperx-platform-gnome/src/atspi.rs`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/transcript_log.rs`

- [ ] **Step 1: Write the failing clipboard fallback tests**

Add tests that prove:
- Pepper X preserves and restores clipboard state when it uses clipboard mediation
- clipboard fallback is selected only after semantic and string-injection paths fail
- the transcript entry records that clipboard mediation ran

- [ ] **Step 2: Run the targeted clipboard tests**

Run:
```sh
cd pepper-x
cargo test -p pepperx-platform-gnome clipboard_insert_ -- --nocapture
cargo test -p pepper-x-app app_shell -- --nocapture
```

Expected:
- the clipboard fallback assertions fail because Pepper X does not mediate clipboard insertion yet

- [ ] **Step 3: Implement the clipboard fallback**

Implement:
- clipboard capture/restore
- one narrow paste path
- transcript diagnostics for clipboard mediation

Do not implement `uinput` in this task.

- [ ] **Step 4: Re-run the targeted clipboard tests**

Run the commands from Step 2.

Expected:
- the clipboard fallback tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add crates/pepperx-platform-gnome/src/atspi.rs app/src/transcription.rs app/src/transcript_log.rs
git -C pepper-x commit -m "Add Pepper X clipboard insertion fallback"
```

## Chunk 3: Raw Input Fallback

### Task 4: Add the Pepper X-owned `uinput` helper as the last fallback

**Files:**
- Create: `crates/pepperx-uinput-helper/Cargo.toml`
- Create: `crates/pepperx-uinput-helper/src/main.rs`
- Modify: `Cargo.toml`
- Modify: `app/src/transcription.rs`
- Modify: `app/src/transcript_log.rs`
- Modify: `packaging/`

- [ ] **Step 1: Write the failing `uinput` fallback tests**

Add tests that prove:
- Pepper X reaches `uinput` only after every other declared backend fails
- the helper protocol is text-focused, not generic keyboard remapping
- transcript diagnostics record `uinput` as the last fallback

- [ ] **Step 2: Run the targeted `uinput` tests**

Run:
```sh
cd pepper-x
cargo test -p pepper-x-app uinput_insert_ -- --nocapture
```

Expected:
- the `uinput` fallback assertions fail because no helper exists yet

- [ ] **Step 3: Implement the minimal helper**

Implement:
- a tiny Pepper X-owned daemon
- one narrow local IPC contract for text insertion
- virtual-device setup that is persistent like `ydotoold`, but Pepper X-owned

The helper must:
- inject text only
- stay unsandboxed
- remain optional until the fallback path selects it

- [ ] **Step 4: Re-run the targeted `uinput` tests**

Run the command from Step 2.

Expected:
- the helper-backed fallback tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add Cargo.toml crates/pepperx-uinput-helper/Cargo.toml crates/pepperx-uinput-helper/src/main.rs app/src/transcription.rs app/src/transcript_log.rs packaging
git -C pepper-x commit -m "Add Pepper X uinput insertion helper"
```

## Chunk 4: Documentation And Verification

### Task 5: Document the loop-4 surface

**Files:**
- Modify: `README.md`
- Modify: `docs/architecture/text-insertion-strategy.md`

- [ ] **Step 1: Update the docs**

Document:
- the declared fallback order
- which target classes are loop-4 supported
- the fact that `uinput` is last fallback, not default

Do not claim:
- universal support
- secure-field support
- old-GNOME compatibility

- [ ] **Step 2: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add README.md docs/architecture/text-insertion-strategy.md
git -C pepper-x commit -m "Document Pepper X fallback insertion loop"
```

### Task 6: Run the exact loop-4 verification set

**Files:**
- Test: `crates/pepperx-platform-gnome/src/atspi.rs`
- Test: `app/src/transcript_log.rs`
- Test: `scripts/smoke-insert-terminal.sh`

- [ ] **Step 1: Run formatting and targeted tests**

Run:
```sh
cd pepper-x
cargo fmt --check
cargo test -p pepperx-platform-gnome fallback_insert_ -- --nocapture
cargo test -p pepperx-platform-gnome clipboard_insert_ -- --nocapture
cargo test -p pepper-x-app app_shell -- --nocapture
cargo test -p pepper-x-app uinput_insert_ -- --nocapture
```

Expected:
- the loop-4 targeted tests pass

- [ ] **Step 2: Run workspace validation**

Run:
```sh
cd pepper-x
cargo check --workspace
cargo test --workspace
```

Expected:
- the workspace builds and tests cleanly with the fallback helper included

- [ ] **Step 3: Run the real-session fallback smokes**

Run inside a supported GNOME session:
```sh
cd pepper-x
./scripts/smoke-insert-accessible.sh text-editor
./scripts/smoke-insert-accessible.sh browser-textarea
./scripts/smoke-insert-terminal.sh
```

Expected:
- semantic targets still pass
- the declared terminal smoke passes through the loop-4 fallback chain

- [ ] **Step 4: Confirm the tree is ready for loop 5**

```bash
git -C pepper-x status --short
```

Expected:
- the working tree is clean because the earlier commits already landed
