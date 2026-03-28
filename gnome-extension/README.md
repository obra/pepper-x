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

On a brand-new install, GNOME Shell may need one session restart before it recognizes the unpacked extension directory. After that first restart, rerunning the script is enough to copy updates and re-enable the extension.

The extension adds a small panel entry with two actions:

- `Open Pepper X Settings`
- `Open Pepper X History`

Those actions reach the Pepper X app over D-Bus and request the shell windows. The extension should stay thin and not assume ownership of modifier-only capture.
