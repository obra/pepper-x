# Pepper X

Pepper X is a GNOME-first local Linux dictation utility inspired by Ghost Pepper.

This repo starts from the approved design and implementation plan:

- [Pepper X design](docs/superpowers/specs/2026-03-27-pepper-x-design.md)
- [Pepper X shell and GNOME integration plan](docs/superpowers/plans/2026-03-27-pepper-x-shell-and-gnome-integration.md)
- [Pepper X GNOME 48 recovery plan](docs/superpowers/plans/2026-03-28-pepper-x-gnome48-recovery.md)

Current direction:

- brand new app with zero shared code from Ghost Pepper
- GNOME 48+ baseline, Wayland-only for V1
- Rust + GTK4/libadwaita app
- app-first ownership of product logic
- thin GNOME Shell extension for shell-facing integration
- unsandboxed `.deb` and `.rpm` distribution
- fully local runtime, no cloud dependencies

The first execution phase is repo bootstrap, app shell, GNOME extension scaffold, IPC, and modifier-only hold-to-talk signaling. Modifier-only capture is not assumed to be extension-only.

## Local prerequisites

Pepper X V1 targets GNOME 48+ on Wayland. The practical distro floor for this path is Ubuntu 25.04+ and Fedora 42+. The workspace is unsandboxed and currently bootstraps the Rust app shell plus GNOME integration scaffolding.

The current GTK/libadwaita dependency set requires Rust 1.92 or newer.

### Fedora

Install the toolchain and native development packages:

```sh
sudo dnf install \
  at-spi2-core-devel \
  cargo \
  gcc \
  glib2-devel \
  gobject-introspection-devel \
  gtk4-devel \
  libadwaita-devel \
  pkgconf-pkg-config
```

### Ubuntu

Install the toolchain and native development packages:

```sh
sudo apt install \
  build-essential \
  cargo \
  libadwaita-1-dev \
  libatspi2.0-dev \
  libgirepository1.0-dev \
  libglib2.0-dev \
  libgtk-4-dev \
  pkg-config
```

## Verification

Run the current automated checks from the repository root:

```sh
cargo fmt --check
cargo test --workspace
cargo check --workspace
bash tests/smoke/test_extension_ipc.sh
./scripts/smoke-hotkey.sh
python3 -m pytest packaging/tests -q
```

For live GNOME 48+ modifier capture validation on a real Wayland session, run:

```sh
./scripts/gnome48-smoke-hotkey.sh
```

Run that helper inside the live GNOME session, or export that session's `DBUS_SESSION_BUS_ADDRESS` first. It only proves the live-session prerequisites. The final press/release check still needs a physical keyboard on the live GNOME session because QEMU `send-key`, VNC, and noVNC injection were not authoritative for the app-owned AT-SPI watcher path.
