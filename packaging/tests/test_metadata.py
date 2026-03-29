from configparser import ConfigParser
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
DEB_ROOT = REPO_ROOT / "packaging" / "deb"
RPM_SPEC = REPO_ROOT / "packaging" / "rpm" / "pepper-x.spec"

DESKTOP_FILE = DEB_ROOT / "pepper-x.desktop"
AUTOSTART_FILE = DEB_ROOT / "pepper-x-autostart.desktop"
CONTROL_FILE = DEB_ROOT / "control"
DEBIAN_INSTALL_FILE = DEB_ROOT / "pepper-x.install"

APPLICATION_ID = "com.obra.PepperX"
EXECUTABLE_NAME = "pepper-x"
UINPUT_HELPER_NAME = "pepperx-uinput-helper"
EXTENSION_UUID = "pepperx@obra"
EXTENSION_INSTALL_ROOT = f"/usr/share/gnome-shell/extensions/{EXTENSION_UUID}"
EXTENSION_ASSET_PATHS = [
    "metadata.json",
    "extension.js",
    "ipc.js",
    "keybindings.js",
    "README.md",
]
DESKTOP_INSTALL_PATH = "/usr/share/applications/com.obra.PepperX.desktop"
AUTOSTART_INSTALL_PATH = "/etc/xdg/autostart/pepper-x-autostart.desktop"
UINPUT_HELPER_INSTALL_PATH = f"pepper-x/{UINPUT_HELPER_NAME}"
DEBIAN_RUNTIME_DEPENDENCIES = [
    "${misc:Depends}",
    "${shlibs:Depends}",
    "libadwaita-1-0",
    "libatspi2.0-0",
    "libgtk-4-1",
    "pipewire",
    "tesseract-ocr",
]
RPM_RUNTIME_DEPENDENCIES = [
    "at-spi2-core",
    "gtk4",
    "libadwaita",
    "pipewire",
    "tesseract",
]


def load_desktop_entry(path: Path) -> dict[str, str]:
    parser = ConfigParser(interpolation=None)
    parser.optionxform = str
    parser.read(path)
    return dict(parser["Desktop Entry"])


def test_desktop_file_uses_application_id_and_exec() -> None:
    entry = load_desktop_entry(DESKTOP_FILE)

    assert entry["Exec"] == EXECUTABLE_NAME
    assert entry["Icon"] == APPLICATION_ID
    assert entry["StartupWMClass"] == APPLICATION_ID


def test_autostart_file_uses_same_executable() -> None:
    entry = load_desktop_entry(AUTOSTART_FILE)

    assert entry["Exec"] == EXECUTABLE_NAME
    assert entry["Icon"] == APPLICATION_ID
    assert entry["X-GNOME-Autostart-enabled"] == "false"
    assert entry["Terminal"] == "false"


def test_desktop_and_autostart_files_keep_matching_launch_metadata() -> None:
    desktop = load_desktop_entry(DESKTOP_FILE)
    autostart = load_desktop_entry(AUTOSTART_FILE)

    for field in ("Type", "Version", "Exec", "Icon", "Terminal"):
        assert desktop[field] == autostart[field]


def test_debian_metadata_is_internally_consistent() -> None:
    control = CONTROL_FILE.read_text()

    assert "Source: pepper-x" in control
    assert "Package: pepper-x" in control
    assert "Architecture: amd64" in control
    assert "Description: GNOME-first local Linux dictation shell" in control
    assert "GNOME 48+" in control
    assert "Ubuntu 25.04+" in control
    assert "Fedora 42+" in control
    assert "Depends: " in control

    for dependency in DEBIAN_RUNTIME_DEPENDENCIES:
        assert dependency in control


def test_debian_install_manifest_packages_the_gnome_extension_assets() -> None:
    install_manifest = DEBIAN_INSTALL_FILE.read_text()

    for asset in EXTENSION_ASSET_PATHS:
        assert f"gnome-extension/{asset} {EXTENSION_INSTALL_ROOT}/" in install_manifest


def test_rpm_spec_references_expected_install_paths() -> None:
    spec = RPM_SPEC.read_text()

    assert "Name:           pepper-x" in spec
    assert "BuildArch:      x86_64" in spec
    assert "Requires:       " in spec
    assert f"%{{_bindir}}/{EXECUTABLE_NAME}" in spec
    assert f"%{{_libexecdir}}/{UINPUT_HELPER_INSTALL_PATH}" in spec
    assert DESKTOP_INSTALL_PATH in spec
    assert AUTOSTART_INSTALL_PATH in spec
    assert "GNOME 48+" in spec
    assert "Ubuntu 25.04+" in spec
    assert "Fedora 42+" in spec

    for dependency in RPM_RUNTIME_DEPENDENCIES:
        assert dependency in spec


def test_rpm_spec_installs_the_gnome_extension_assets() -> None:
    spec = RPM_SPEC.read_text()

    for asset in EXTENSION_ASSET_PATHS:
        assert (
            f"install -Dpm0644 gnome-extension/{asset} "
            f"%{{buildroot}}{EXTENSION_INSTALL_ROOT}/{asset}"
        ) in spec
        assert f"{EXTENSION_INSTALL_ROOT}/{asset}" in spec


def test_rpm_spec_installs_the_same_packaging_assets() -> None:
    spec = RPM_SPEC.read_text()

    assert "install -Dpm0644 packaging/deb/pepper-x.desktop" in spec
    assert "install -Dpm0644 packaging/deb/pepper-x-autostart.desktop" in spec
    assert f"%{{_bindir}}/{EXECUTABLE_NAME}" in spec
    assert f"%{{_libexecdir}}/{UINPUT_HELPER_INSTALL_PATH}" in spec
    assert DESKTOP_INSTALL_PATH in spec
    assert AUTOSTART_INSTALL_PATH in spec
