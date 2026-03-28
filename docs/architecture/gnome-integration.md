# Pepper X GNOME Integration

## D-Bus contract

- Service name: `com.obra.PepperX.Service`
- Object path: `/com/obra/PepperX`
- Interface: `com.obra.PepperX`

## Supported methods

- `Ping() -> s`
  Returns `"pong"` and marks the extension link as connected.
- `StartRecording(s trigger_source) -> ()`
  Accepts:
  - `modifier-only`
  - `standard-shortcut`
  - `shell-action`
- `StopRecording() -> ()`
- `ShowSettings() -> ()`
- `ShowHistory() -> ()`
- `GetCapabilities() -> (bbs)`
  Returns:
  - `modifier_only_supported`
  - `extension_connected`
  - `version`

## App ownership

Pepper X keeps the session state machine and shell routing in the app. The GNOME-facing service only translates D-Bus requests into:

- session state transitions
- app shell commands

No ASR, cleanup, OCR, insertion, or history behavior crosses this boundary.

## Extension startup expectations

The GNOME Shell extension should:

1. call `Ping` during startup to prove reachability
2. call `GetCapabilities` after a successful ping
3. use `ShowSettings` for a manual shell action
4. use `ShowHistory` for a manual shell action

Modifier-only hold-to-talk now lives in the app on GNOME 48+ through an app-owned `libatspi` device watcher. The extension should not own global hotkey capture.

If the app service is unavailable, the extension should log a clear error and avoid hanging GNOME Shell.
