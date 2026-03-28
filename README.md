# Pepper X

Pepper X is a GNOME-first local Linux dictation utility inspired by Ghost Pepper.

This repo starts from the approved design and implementation plan:

- [Pepper X design](docs/superpowers/specs/2026-03-27-pepper-x-design.md)
- [Pepper X shell and GNOME integration plan](docs/superpowers/plans/2026-03-27-pepper-x-shell-and-gnome-integration.md)

Current direction:

- brand new app with zero shared code from Ghost Pepper
- GNOME-first, Wayland-only for V1
- Rust + GTK4/libadwaita app
- thin GNOME Shell extension for shell-facing integration
- unsandboxed `.deb` and `.rpm` distribution
- fully local runtime, no cloud dependencies

The first execution phase is repo bootstrap, app shell, GNOME extension scaffold, IPC, and modifier-only hold-to-talk signaling.
