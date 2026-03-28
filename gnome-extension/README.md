# Pepper X GNOME Shell Extension

Pepper X keeps core product logic in the Rust app and uses this GNOME Shell extension only for shell-facing integration. Modifier-only capture is not assumed to live in the extension; on GNOME 48+ the app may own that path.

## Development checks

From the repository root:

```sh
./scripts/dev-install-extension.sh --check
```

## Local install on GNOME Shell

From the repository root on a GNOME 48+ Wayland session:

```sh
./scripts/dev-install-extension.sh
```

The extension adds a small panel entry with one action:

- `Open Pepper X Settings`

That action reaches the Pepper X app over D-Bus and requests the settings shell window. The extension should stay thin and not assume ownership of modifier-only capture.
