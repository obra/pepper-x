# Pepper X V1 Release Checklist

## Install

- Build the release artifacts for the target distro.
- Install the package on a clean GNOME 48+ Wayland system.
- Confirm the packaged desktop entry launches `pepper-x`.
- Confirm the autostart entry launches the same executable without forcing the window open.
- Open Pepper X manually from the desktop entry and confirm the normal UI still appears.

## Upgrade

- Upgrade from the previous package version with the distro package manager.
- Confirm the app still starts and the existing settings/history state remains readable.
- Confirm the autostart desktop entry still points at the packaged executable after upgrade.
- Run the upgrade verification scripts for the relevant distro artifact pair.

## Uninstall

- Remove the package with the distro package manager.
- Confirm the packaged desktop entry and autostart entry are removed.
- Confirm the `pepper-x` executable is no longer installed from the package.
- Keep user state unless a separate data-removal step is explicitly requested.

## Release

- Run the Rust checks and the packaging verification scripts before tagging a release.
- Confirm packaged launches behave correctly in both modes:
  - interactive launch opens the UI
  - autostart launch keeps the runtime alive without forcing the window open
- Publish the distro artifacts only after the install, upgrade, and uninstall checks pass.
