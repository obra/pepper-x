# Pepper X Modifier-Only Hotkey Smoke

Run this checklist from a GNOME 48+ Wayland session with the Pepper X app installed. The extension may still provide shell-facing integration, but modifier-only capture is not assumed to be extension-only.

Before the manual key test, run this helper from inside the live GNOME session, or export that session's `DBUS_SESSION_BUS_ADDRESS` first:

```sh
./scripts/gnome48-smoke-hotkey.sh
```

- Verify modifier-only press triggers StartRecording.
- Verify release triggers StopRecording.
- Verify repeated use stays stable across multiple attempts.
- Verify `GetCapabilities` still reports the correct modifier-only capability state when the extension is absent.
- Verify first install of the extension requires only a single GNOME session restart, not repeated reinstalls.
- Do not treat QEMU `send-key`, VNC, or noVNC input injection as authoritative for this path; use a physical keyboard on the live GNOME session.
- Confirm the app log shows:
  - `[Pepper X] modifier-only start`
  - `[Pepper X] modifier-only stop`
