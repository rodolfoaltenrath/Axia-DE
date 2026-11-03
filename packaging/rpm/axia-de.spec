Name:           axia-de
Version:        0.8.7
Release:        3%{?dist}
Summary:        Wayland desktop based on LordOS
License:        MIT AND BSD-2-Clause AND CC0-1.0 AND OFL-1.1 AND CC-BY-4.0 AND CC-BY-3.0-US AND GPL-2.0-or-later
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  gcc
BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  make
BuildRequires:  python3
BuildRequires:  xz
BuildRequires:  binutils
BuildRequires:  pkgconfig(wayland-server)
BuildRequires:  pkgconfig(wayland-client)
BuildRequires:  pkgconfig(wayland-protocols)
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  pkgconfig(libseat)
BuildRequires:  pkgconfig(openssl)
BuildRequires:  pkgconfig(freerdp3)
BuildRequires:  pkgconfig(freerdp-client3)
BuildRequires:  pkgconfig(winpr3)
BuildRequires:  libxcrypt-devel
BuildRequires:  pkgconfig(libavcodec)
BuildRequires:  pkgconfig(libavformat)
BuildRequires:  pkgconfig(libavutil)
BuildRequires:  pkgconfig(libswscale)
BuildRequires:  pkgconfig(libswresample)
BuildRequires:  pkgconfig(libva)
BuildRequires:  pkgconfig(libva-drm)
BuildRequires:  pkgconfig(poppler-glib)
BuildRequires:  pkgconfig(libarchive)
BuildRequires:  pkgconfig(cairo-pdf)
BuildRequires:  pkgconfig(cairo-ft)
BuildRequires:  pkgconfig(freetype2)
BuildRequires:  pkgconfig(fontconfig)
BuildRequires:  systemd-rpm-macros
Requires:       dbus-daemon
Requires:       xkeyboard-config
Requires:       libX11-common
Requires:       tzdata
Requires:       dnf5
Requires:       xdg-user-dirs
Requires:       curl
Recommends:     pipewire-pulseaudio
Recommends:     xorg-x11-server-Xwayland
Recommends:     polkit
Recommends:     mate-polkit
# Lord DE was this desktop's former name.
Obsoletes:      lord-de < 0.8.7-2
Provides:       lord-de = %{version}-%{release}

%description
Axia DE is a Wayland desktop based on LordOS: its compositor, React desktop
and native applications. Host networking, packages and login are managed by
the distribution. This experimental package adds a Wayland session entry.
It does not replace the display manager or enable system services.

%prep
%autosetup

%build
# dist.sh includes the frontend built from package-lock.json. No network access
# or Node.js runtime is needed to compile or run this local RPM.
%set_build_flags
%make_build -C server GPU=0
test -x server/build/axia-media
test -x server/build/axia-doc

%install
python3 tools/desktop/install.py --prefix %{_prefix} --libexecdir %{_libexecdir} --destdir %{buildroot}
install -Dpm 644 packaging/systemd/axia-update.service %{buildroot}%{_unitdir}/axia-update.service
install -Dpm 644 packaging/systemd/axia-update.timer %{buildroot}%{_unitdir}/axia-update.timer

%check
%make_build -C server GPU=0 test-platform
%make_build -C server GPU=0 build/tp-test
server/build/tp-test
python3 tools/desktop/check.py

# Automatic updates are opt-in; do not run presets which could enable this timer.
%preun
%systemd_preun axia-update.timer

%postun
%systemd_postun axia-update.timer

%posttrans
# Systems that chose the lord-de session (the remix ISO sets it for GDM and every account) keep
# logging into the same desktop under its new name.
if [ -f /etc/gdm/custom.conf ]; then
    sed -i 's/^DefaultSession=lord-de\.desktop$/DefaultSession=axia.desktop/' /etc/gdm/custom.conf
fi
for f in /var/lib/AccountsService/users/*; do
    [ -f "$f" ] && sed -i 's/^\(X\?Session\)=lord-de$/\1=axia/' "$f"
done
if [ -x /usr/bin/systemctl ]; then
    /usr/bin/systemctl daemon-reload >/dev/null 2>&1 || :
fi
exit 0

%files
%license LICENSE
%{_bindir}/axia-session
%{_bindir}/axia-fetch
%{_bindir}/axia-sysreport
%{_libexecdir}/axia/
%{_datadir}/wayland-sessions/axia.desktop
%{_datadir}/doc/axia-de/
%{_datadir}/licenses/axia-de/third_party/
%{_datadir}/licenses/axia-de/OFL-Noto.txt
%{_unitdir}/axia-update.service
%{_unitdir}/axia-update.timer

%changelog
* Mon Nov 02 2026 Axia DE contributors - 0.8.7-3
- Prepare Copr snapshot builds and optional automatic package updates.
- Always include document/media helpers and the Fedora session wrapper.
* Mon Nov 02 2026 Axia DE contributors - 0.8.7-2
- Rename to Axia DE; remove the Debian distribution layer.
* Mon Nov 02 2026 Lord DE contributors - 0.8.7-1
- Integrate LordOS 0.8.7 while preserving the independent desktop profile.
* Fri Oct 23 2026 Lord DE contributors - 0.8.3-1
- Extract an independent desktop profile with a Fedora session package.
