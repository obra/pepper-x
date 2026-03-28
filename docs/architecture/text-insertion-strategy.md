# Pepper X Text Insertion Strategy

## Scope

This document defines the V1 text insertion architecture for Pepper X on GNOME Wayland.

Constraints:

- GNOME-first
- Wayland-only for V1
- unsandboxed
- Ubuntu and Fedora targets
- modern baseline acceptable
- reliability matters more than elegance

This document covers insertion only. Modifier capture is a separate problem.

## Goal

Pepper X must insert final text into the currently focused application with a backend stack that is:

- reliable for common Linux-native apps
- diagnosable when it fails
- explicit about which target classes require fallback behavior
- honest about where privileged input injection is still necessary

## Non-Goals

- universal zero-fallback insertion across every Linux app
- secure-field bypasses
- old GNOME compatibility guarantees
- pretending that accessibility alone covers terminals, Wine, or canvas-heavy apps

## Core Decision

Pepper X should not make `uinput` the primary insertion path.

The default insertion strategy should be:

1. semantic accessibility insertion
2. AT-SPI string injection
3. clipboard-assisted paste
4. privileged `uinput` fallback

This keeps the common case layout-independent and text-aware, while preserving a hard fallback for hostile targets.

## Why `uinput` Is Not First

`uinput` is valuable, but it is the wrong default:

- it needs elevated device access
- it is keycode-oriented rather than text-oriented
- keyboard layout differences make it easier to get wrong
- it is more likely to produce surprising side effects in focused apps
- it adds a long-tail support burden around permissions, virtual devices, and desktop quirks

Pepper X should still have a `uinput` backend, but only as the last fallback for targets that do not expose a useful text-oriented path.

## Backend Stack

### 1. Semantic accessibility insertion

Primary API:

- `AtspiEditableText.insert_text`
- `AtspiEditableText.set_text_contents` only where replacing the full field is the correct behavior

Use this when the focused object:

- is editable
- exposes the `EditableText` interface
- is not read-only
- is not a password or otherwise protected field

Why this is first:

- it works with text instead of keyboard emulation
- it is not layout-dependent
- it is likely to behave correctly for Unicode text
- it is the cleanest fit for GTK, many Qt widgets, office apps, and browser text controls that expose accessibility correctly

### 2. AT-SPI string injection

Primary API:

- `Atspi.generate_keyboard_event(..., ATSPI_KEY_STRING)`

Use this when:

- semantic insertion is unavailable
- the target appears likely to accept IME-style text input
- the target is not known to require raw keycode emulation

This is the preferred bridge for targets that behave more like terminals or input-method consumers than structured editable widgets.

This path needs live verification. It is promising for some terminal-style targets, but Pepper X should not assume universal success without smoke coverage.

### 3. Clipboard-assisted paste

Use this when:

- semantic insertion is unavailable
- string injection is unavailable or rejected
- the focused target is likely to handle paste more reliably than direct typing

Requirements:

- preserve and restore clipboard state when feasible
- avoid destructive clipboard behavior without user visibility
- record that insertion used clipboard mediation

This is a compatibility layer, not the ideal path.

### 4. Privileged `uinput` fallback

Use this when:

- the target does not expose a useful accessibility text interface
- IME-style string injection does not work
- clipboard-assisted paste is unavailable or incorrect
- the target class is known to require raw input emulation

Likely examples:

- some terminals
- some Xwayland apps
- custom-drawn UI toolkits
- some Wine apps

Implementation shape:

- a tiny Pepper X-owned daemon
- persistent virtual device, inspired by `ydotoold`
- narrow local IPC to the main app
- strict responsibility: text injection only

Pepper X should not depend on `ydotool` directly. The useful lesson is the persistent daemon architecture, not the dependency.

## Target Classes

### Best-case targets

Expected primary backend:

- semantic accessibility insertion

Likely app classes:

- GTK4 and libadwaita apps
- GTK3 apps with working AT-SPI exposure
- LibreOffice
- browsers and webviews that expose editable accessibility objects correctly
- mainstream Qt apps that use standard accessible widgets

### Mixed targets

Expected primary backend:

- semantic accessibility first
- string injection or clipboard fallback when accessibility is incomplete

Likely app classes:

- non-trivial Qt apps
- apps with custom widgets layered over otherwise accessible shells
- browser-hosted editors with partial accessibility fidelity

### Hostile targets

Expected primary backend:

- string injection, clipboard paste, or `uinput`

Likely app classes:

- terminal emulators
- Xwayland-era apps with weak accessibility
- custom-rendered editors and canvas-heavy apps
- Wine applications

Pepper X must treat these as explicit fallback targets rather than accidental edge cases.

## Expected Coverage By Target

### GTK and libadwaita

These should be the best-supported targets. Pepper X should expect `EditableText` to be the normal success path.

### Qt

Qt has a real accessibility stack, but coverage depends on whether the app uses standard accessible widgets or custom controls. Pepper X should expect a split result:

- ordinary widgets often succeed with semantic insertion
- custom widgets may require fallback behavior

### Terminals

Terminals are a special class. Pepper X should not assume `EditableText` support. The expected order is:

1. string injection
2. clipboard-assisted paste
3. `uinput`

Terminals need explicit smoke coverage because they are a core dictation target.

### Xwayland and legacy X apps

These should be treated as fallback-first targets. If a legacy app exposes enough accessibility, that is a bonus, not a design assumption.

### Wine

Wine should be treated as hostile by default. Pepper X should assume that semantic accessibility may be incomplete or absent and should be prepared to use `uinput`.

## Selection Algorithm

For each insertion request:

1. Identify the focused accessible object and target metadata.
2. If the target is editable and exposes `EditableText`, try semantic insertion.
3. If semantic insertion is unavailable, try AT-SPI string injection for targets that plausibly accept composed text input.
4. If that fails, try clipboard-assisted paste where the target is likely to honor paste correctly.
5. If all text-oriented paths fail, route to the `uinput` daemon.
6. Record the selected backend, failure chain, and target metadata for diagnostics.

Pepper X should bias toward text-oriented paths before synthetic keyboard emulation.

## Diagnostics

Every insertion attempt should capture:

- target application name
- toolkit or target classification when known
- focused accessibility role and interfaces
- chosen backend
- fallback chain attempted
- success or failure result
- failure reason

This is required both for user-facing diagnostics and for future backend tuning.

## Safety Rules

Pepper X should refuse insertion or require an explicit fallback decision when:

- the focused field is read-only
- the target appears to be a password field
- focus cannot be identified confidently
- the fallback path would create surprising clipboard loss without restoration

The insertion subsystem must prefer a clear failure over corrupting user input.

## Recommended Spikes

Before locking the implementation plan for insertion, Pepper X should spike these in order:

1. semantic insertion with `AtspiEditableText` on GNOME 48+
2. AT-SPI string injection against terminal targets
3. clipboard preservation and restore behavior under Wayland
4. minimal `uinput` daemon for hard fallback targets

The terminal spike set should include:

- Ghostty
- xterm
- GNOME Text Editor
- a Qt text editor
- LibreOffice Writer
- a browser text area
- a Wine target

## Proposed Planning Change

The future insertion implementation plan should explicitly model insertion as a multi-backend subsystem rather than a single mechanism.

The plan should have separate tasks for:

- target inspection and routing
- semantic accessibility insertion
- string injection
- clipboard fallback
- privileged `uinput` fallback
- diagnostics and smoke matrix

That is the minimum honest shape for reliable text insertion on GNOME Wayland.
