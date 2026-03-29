Name:           pepper-x
Version:        0.1.0
Release:        1%{?dist}
Summary:        GNOME-first local Linux dictation shell
License:        Proprietary
URL:            https://github.com/obra/pepper-x
Source0:        %{name}-%{version}.tar.gz

BuildArch:      x86_64
Requires:       at-spi2-core
Requires:       gtk4
Requires:       libadwaita
Requires:       pipewire
Requires:       tesseract

%description
Pepper X is an unsandboxed GTK4/libadwaita desktop shell for local dictation on GNOME Wayland.
Pepper X V1 targets GNOME 48+ on Wayland, with a practical floor of Ubuntu 25.04+ and Fedora 42+.

%install
install -Dpm0755 target/release/pepper-x %{buildroot}%{_bindir}/pepper-x
install -Dpm0755 target/release/pepperx-uinput-helper %{buildroot}%{_libexecdir}/pepper-x/pepperx-uinput-helper
install -Dpm0644 packaging/deb/pepper-x.desktop %{buildroot}/usr/share/applications/com.obra.PepperX.desktop
install -Dpm0644 packaging/deb/pepper-x-autostart.desktop %{buildroot}/etc/xdg/autostart/pepper-x-autostart.desktop

%files
%{_bindir}/pepper-x
%{_libexecdir}/pepper-x/pepperx-uinput-helper
/usr/share/applications/com.obra.PepperX.desktop
/etc/xdg/autostart/pepper-x-autostart.desktop
