# Pepper X

Pepper X is a GNOME-first local Linux dictation utility inspired by Ghost Pepper.

This repo starts from the approved design and implementation plan:

- [Pepper X design](docs/superpowers/specs/2026-03-27-pepper-x-design.md)
- [Pepper X shell and GNOME integration plan](docs/superpowers/plans/2026-03-27-pepper-x-shell-and-gnome-integration.md)

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

### Fedora

Install the toolchain and native development packages:

```sh
sudo dnf install \
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
  libgirepository1.0-dev \
  libglib2.0-dev \
  libgtk-4-dev \
  pkg-config
```

## Bootstrap checks

Run the current workspace checks from the repository root:

```sh
cargo check --workspace
bash tests/smoke/test_extension_ipc.sh
```

The Cargo check should pass after Task 1. The IPC smoke test is expected to fail until the D-Bus service lands in the later IPC tasks.
