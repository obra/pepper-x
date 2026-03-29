from configparser import ConfigParser
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
DEB_ROOT = REPO_ROOT / "packaging" / "deb"
RPM_SPEC = REPO_ROOT / "packaging" / "rpm" / "pepper-x.spec"
README_FILE = REPO_ROOT / "README.md"

DESKTOP_FILE = DEB_ROOT / "pepper-x.desktop"
AUTOSTART_FILE = DEB_ROOT / "pepper-x-autostart.desktop"
CONTROL_FILE = DEB_ROOT / "control"

APPLICATION_ID = "com.obra.PepperX"
EXECUTABLE_NAME = "pepper-x"
UINPUT_HELPER_NAME = "pepperx-uinput-helper"
DESKTOP_INSTALL_PATH = "/usr/share/applications/com.obra.PepperX.desktop"
AUTOSTART_INSTALL_PATH = "/etc/xdg/autostart/pepper-x-autostart.desktop"
UINPUT_HELPER_INSTALL_PATH = f"pepper-x/{UINPUT_HELPER_NAME}"


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
    assert entry["X-GNOME-Autostart-enabled"] == "false"


def test_debian_metadata_is_internally_consistent() -> None:
    control = CONTROL_FILE.read_text()

    assert "Source: pepper-x" in control
    assert "Package: pepper-x" in control
    assert "Architecture: amd64" in control
    assert "Description: GNOME-first local Linux dictation shell" in control
    assert "GNOME 48+" in control
    assert "Ubuntu 25.04+" in control
    assert "Fedora 42+" in control


def test_rpm_spec_references_expected_install_paths() -> None:
    spec = RPM_SPEC.read_text()

    assert "Name:           pepper-x" in spec
    assert "BuildArch:      x86_64" in spec
    assert f"%{{_bindir}}/{EXECUTABLE_NAME}" in spec
    assert f"%{{_libexecdir}}/{UINPUT_HELPER_INSTALL_PATH}" in spec
    assert DESKTOP_INSTALL_PATH in spec
    assert AUTOSTART_INSTALL_PATH in spec
    assert "GNOME 48+" in spec
    assert "Ubuntu 25.04+" in spec
    assert "Fedora 42+" in spec


def test_readme_documents_gnome_48_packaging_floor() -> None:
    readme = README_FILE.read_text()

    assert "GNOME 48+ baseline" in readme
    assert "Ubuntu 25.04+" in readme
    assert "Fedora 42+" in readme
