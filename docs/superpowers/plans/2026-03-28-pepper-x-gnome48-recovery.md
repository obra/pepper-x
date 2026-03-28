# Pepper X GNOME 48 Recovery Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish Pepper X subproject 1 on a GNOME 48+ baseline with a working GTK/libadwaita app shell, a thin GNOME Shell extension, a narrow D-Bus contract, and a proven modifier-only hold-to-talk path that does not depend on unsupported GNOME 46 extension behavior.

**Architecture:** Carry forward the existing app-first Rust shell, session state, D-Bus service, and packaging skeleton. Move modifier-only capture out of the unproven extension-only path and into an app-owned GNOME 48+ accessibility/device backend built on `libatspi`. Keep the extension thin by limiting it to shell-facing status and app actions, and treat live modifier verification as a real-session requirement rather than something VNC or QEMU key injection can prove.

**Tech Stack:** Rust, Cargo workspace, GTK4, libadwaita, D-Bus, libatspi, GObject Introspection, GNOME Shell extension (GJS), shell-based smoke tests, Debian/RPM metadata

---

## File Structure

**Repository root:** `pepper-x/`

**Modify:**
- `docs/superpowers/specs/2026-03-27-pepper-x-design.md`
  - Update the agreed GNOME baseline and insertion assumptions.
- `docs/architecture/gnome-integration.md`
  - Clarify the app ↔ extension contract and hotkey ownership after the GNOME 48+ spike.
- `docs/architecture/text-insertion-strategy.md`
  - Reference future insertion work without broadening subproject 1 scope.
- `README.md`
  - Document the GNOME 48+ baseline and development/test prerequisites.
- `app/Cargo.toml`
  - Keep app-side wiring aligned with the GNOME modifier backend and IPC contract.
- `app/src/app.rs`
  - Wire the modifier-only capture backend into the app lifecycle.
- `crates/pepperx-ipc/src/lib.rs`
  - Keep the service name and shell capability defaults aligned with the live contract.
- `crates/pepperx-platform-gnome/Cargo.toml`
  - Link the GNOME accessibility/device backend against the required native libraries.
- `crates/pepperx-platform-gnome/src/lib.rs`
  - Export the GNOME-side capture/bootstrap surface.
- `crates/pepperx-platform-gnome/src/service.rs`
  - Keep the D-Bus service narrow and current.
- `gnome-extension/extension.js`
  - Limit the extension to shell-facing actions and startup/status behavior if the app owns hotkey capture.
- `gnome-extension/ipc.js`
  - Keep the D-Bus client aligned with the current app contract.
- `gnome-extension/keybindings.js`
  - Either remove modifier ownership entirely or leave only a documented fallback path.
- `gnome-extension/README.md`
  - Update development expectations for GNOME 48+.
- `scripts/dev-install-extension.sh`
  - Make first-install and reload expectations explicit.
- `scripts/smoke-hotkey.sh`
  - Verify the current modifier-only ownership path and required code markers.
- `tests/smoke/test_extension_ipc.sh`
  - Keep the D-Bus smoke aligned with the real service name and capabilities.
- `tests/smoke/test_modifier_only_hotkey.md`
  - Replace the old extension-only manual checklist with the GNOME 48+ workflow.

**Create:**
- `crates/pepperx-platform-gnome/src/atspi.rs`
  - GNOME 48+ accessibility/device monitoring bridge for modifier-only capture.
- `scripts/gnome48-smoke-hotkey.sh`
  - Real-session smoke helper for the GNOME 48+ modifier-only path.

**Test:**
- `app/src/app.rs`
- `crates/pepperx-platform-gnome/src/atspi.rs`
- `crates/pepperx-platform-gnome/src/service.rs`
- `tests/smoke/test_extension_ipc.sh`

---

## Chunk 1: Planning Recovery

### Task 1: Update the docs to reflect the GNOME 48+ baseline

**Files:**
- Modify: `docs/superpowers/specs/2026-03-27-pepper-x-design.md`
- Modify: `docs/architecture/gnome-integration.md`
- Modify: `docs/architecture/text-insertion-strategy.md`
- Modify: `README.md`
- Modify: `gnome-extension/README.md`
- Modify: `tests/smoke/test_modifier_only_hotkey.md`

- [ ] **Step 1: Write the failing documentation expectations**

Add explicit documentation assertions for:
- GNOME 48+ as the hotkey-test baseline
- Ubuntu 25.04+ and Fedora 42+ as the practical distro floor for this path
- Rust 1.92+ as the minimum toolchain for the selected GTK/libadwaita stack
- app-first ownership of product logic
- modifier-only capture no longer assumed to be an extension-only capability

- [ ] **Step 2: Review the docs to verify the expectations are currently missing**

Run:
```sh
cd pepper-x
rg -n "GNOME 48|Ubuntu 25.04|Fedora 42|Atspi|modifier-only capture" docs README.md gnome-extension/README.md tests/smoke/test_modifier_only_hotkey.md
```

Expected:
- important GNOME 48+ assumptions are missing or incomplete

- [ ] **Step 3: Update the docs minimally**

Make the smallest honest edits that:
- preserve the original history
- document the revised baseline
- point future insertion work to `docs/architecture/text-insertion-strategy.md`

- [ ] **Step 4: Re-run the documentation scan**

Run the command from Step 2.

Expected:
- the revised GNOME 48+ assumptions are now present

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add docs/superpowers/specs/2026-03-27-pepper-x-design.md docs/architecture/gnome-integration.md docs/architecture/text-insertion-strategy.md README.md gnome-extension/README.md tests/smoke/test_modifier_only_hotkey.md docs/superpowers/plans/2026-03-28-pepper-x-gnome48-recovery.md docs/superpowers/plans/2026-03-27-pepper-x-shell-and-gnome-integration.md
git -C pepper-x commit -m "Revise Pepper X GNOME 48 planning"
```

---

## Chunk 2: GNOME 48+ Modifier Capture Spike

### Task 2: Add failing tests for the app-owned GNOME 48+ modifier backend

**Files:**
- Modify: `app/Cargo.toml`
- Modify: `app/src/app.rs`
- Modify: `app/src/background.rs`
- Modify: `app/src/settings.rs`
- Create: `crates/pepperx-platform-gnome/src/atspi.rs`
- Modify: `crates/pepperx-platform-gnome/src/lib.rs`
- Test: `crates/pepperx-session/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Add tests that prove:
- the GNOME platform layer can describe whether modifier capture is available
- duplicate start/stop transitions are still rejected by the session state machine
- the app and IPC layers agree on the live capability state they report

- [ ] **Step 2: Run the targeted tests to verify failure**

Run:
```sh
cd pepper-x
cargo test -p pepper-x-app app_shell -- --nocapture
cargo test -p pepperx-session -- --nocapture
```

Expected:
- the new availability/capability assertions fail because the GNOME 48+ bridge does not exist yet

- [ ] **Step 3: Implement the smallest GNOME 48+ bridge**

Implement:
- a focused `atspi.rs` module that translates the chosen modifier sequence into app callbacks
- app lifecycle wiring that starts the bridge only when the runtime environment supports it
- capability plumbing that surfaces modifier support cleanly

Keep this tight:
- no insertion work
- no privileged helper yet
- no broad platform abstraction layer

- [ ] **Step 4: Re-run the targeted tests**

Run the commands from Step 2.

Expected:
- the availability/capability tests pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add app/Cargo.toml app/src/app.rs app/src/background.rs app/src/settings.rs crates/pepperx-platform-gnome/src/lib.rs crates/pepperx-platform-gnome/src/atspi.rs crates/pepperx-session/src/lib.rs
git -C pepper-x commit -m "Add GNOME 48 modifier capture bridge"
```

---

## Chunk 3: Thin Extension Recovery

### Task 3: Re-scope the GNOME extension around shell actions and reachability

**Files:**
- Modify: `docs/architecture/gnome-integration.md`
- Modify: `gnome-extension/extension.js`
- Modify: `gnome-extension/ipc.js`
- Modify: `gnome-extension/keybindings.js`
- Modify: `gnome-extension/README.md`
- Modify: `scripts/dev-install-extension.sh`
- Modify: `scripts/smoke-hotkey.sh`
- Modify: `tests/smoke/test_extension_ipc.sh`

- [ ] **Step 1: Write the failing extension checks**

Update the existing shell smoke scripts so they fail until the extension clearly reflects its reduced responsibility:
- startup ping still required
- settings/history shell actions still required
- stale modifier-only ownership markers removed or replaced

- [ ] **Step 2: Run the shell checks to verify failure**

Run:
```sh
cd pepper-x
bash tests/smoke/test_extension_ipc.sh
./scripts/smoke-hotkey.sh
```

Expected:
- at least one check fails because the extension still reflects the old ownership model

- [ ] **Step 3: Implement the minimal extension recovery**

Implement:
- an extension entrypoint that only owns shell-facing actions and startup reachability
- D-Bus capability queries that report the current app-owned hotkey capability state accurately
- either an empty or explicitly unsupported keybinding module if the app now owns modifier capture

- [ ] **Step 4: Re-run the shell checks**

Run the commands from Step 2.

Expected:
- the shell checks pass

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add docs/architecture/gnome-integration.md gnome-extension/extension.js gnome-extension/ipc.js gnome-extension/keybindings.js gnome-extension/README.md scripts/dev-install-extension.sh scripts/smoke-hotkey.sh tests/smoke/test_extension_ipc.sh
git -C pepper-x commit -m "Keep Pepper X extension thin on GNOME 48"
```

---

## Chunk 4: Real GNOME 48+ Verification

### Task 4: Prove modifier-only hold-to-talk on a live GNOME 48+ session

**Files:**
- Create: `scripts/gnome48-smoke-hotkey.sh`
- Modify: `README.md`
- Modify: `tests/smoke/test_modifier_only_hotkey.md`

- [ ] **Step 1: Write the failing live-session checklist and helper**

Document and script the exact expectations for:
- app startup
- extension startup
- modifier-only press starts recording
- modifier release stops recording
- repeated use stays stable
- the helper proves only live-session prerequisites, not the final keypress itself

- [ ] **Step 2: Run the automated local checks**

Run:
```sh
cd pepper-x
cargo fmt --check
cargo test --workspace
cargo check --workspace
bash tests/smoke/test_extension_ipc.sh
./scripts/smoke-hotkey.sh
python3 -m pytest packaging/tests -q
```

Expected:
- all automated checks pass before the live GNOME session run

- [ ] **Step 3: Run the live GNOME 48+ smoke**

Run the helper and checklist on a real GNOME 48+ Wayland session on:
- Ubuntu 25.04+ or newer
- Fedora 42+ or newer

Expected:
- the app and extension start cleanly
- the helper reports live-session capability readiness
- modifier-only hold-to-talk works end to end when driven from a physical keyboard on the live GNOME session
- the current capability state is visible in diagnostics

Notes:
- Do not treat QEMU `send-key`, VNC, or noVNC injection as authoritative for this path.
- If guest-local `uinput` injection is later proven to hit the same monitor path, it may be added as an automation aid, but it is not the baseline assumption for this plan.

- [ ] **Step 4: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add README.md tests/smoke/test_modifier_only_hotkey.md scripts/gnome48-smoke-hotkey.sh
git -C pepper-x commit -m "Verify Pepper X GNOME 48 hotkey path"
```

---

## Chunk 5: Subproject 1 Closeout

### Task 5: Final verification and packaging pass

**Files:**
- Modify: `packaging/deb/control`
- Modify: `packaging/rpm/pepper-x.spec`
- Modify: `packaging/tests/test_metadata.py`
- Modify: `README.md`

- [ ] **Step 1: Write the failing metadata expectations**

Add checks that the packaging metadata and README reflect:
- GNOME 48+ baseline
- Ubuntu 25.04+ or newer
- Fedora 42+ or newer

- [ ] **Step 2: Run the packaging checks to verify failure**

Run:
```sh
cd pepper-x
python3 -m pytest packaging/tests -q
```

Expected:
- packaging or README metadata fails the updated baseline expectations

- [ ] **Step 3: Implement the minimal metadata fix**

Update:
- package metadata
- README prerequisites, including Rust 1.92+ and the native `libatspi` development packages needed for the GNOME 48+ backend
- packaging tests

- [ ] **Step 4: Re-run the full automated verification**

Run:
```sh
cd pepper-x
cargo fmt --check
cargo test --workspace
cargo check --workspace
bash tests/smoke/test_extension_ipc.sh
./scripts/smoke-hotkey.sh
python3 -m pytest packaging/tests -q
git status --short
```

Expected:
- all automated checks pass
- the tree is either clean or only contains the intentional live-smoke artifacts

- [ ] **Step 5: Commit**

```bash
git -C pepper-x status --short
git -C pepper-x add packaging/deb/control packaging/rpm/pepper-x.spec packaging/tests/test_metadata.py README.md
git -C pepper-x commit -m "Finalize Pepper X GNOME 48 baseline"
```
