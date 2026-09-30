# Nothing about the version is written here: scripts/build-rpm.sh passes
#   --define "lucerna_version ..." --define "lucerna_release ..." --define "lucerna_tarball_version ..."
# derived from the workspace version (docs/PACKAGING.md).

# The binaries are Rust and already stripped of debug info by the release profile.
%global debug_package %{nil}

Name:           lucerna
Version:        %{lucerna_version}
Release:        %{lucerna_release}
Summary:        Animated wallpapers for Linux

License:        MIT
URL:            https://github.com/blindicide/lucerna
Source0:        lucerna-%{lucerna_tarball_version}.tar.gz

# The Rust toolchain comes from rustup (rust-toolchain.toml pins the version), not from a package.
BuildRequires:  gcc
BuildRequires:  pkgconf-pkg-config
BuildRequires:  gtk4-devel
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib
Requires:       mpv

%description
Lucerna plays video and animated-image wallpapers as the desktop
background, using mpv. A small per-user background service keeps the
wallpapers running after the control window is closed, restores them at
login, and pauses them while they are covered by a fullscreen window or
the screen is locked.

The package contains the GTK 4 control application (lucerna), the
background service (lucernad) and a command-line controller with
diagnostics (lucernactl). It supports X11 sessions, with Linux Mint
Cinnamon as the target desktop; Wayland sessions are detected and
reported but not supported.

%prep
%autosetup -n lucerna-%{lucerna_tarball_version}

%build
cargo build --release --locked -p lucerna-ui -p lucerna-daemon -p lucerna-cli

%install
bash scripts/install-files.sh --destdir %{buildroot} --prefix %{_prefix}

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/org.lucerna.Lucerna.desktop
appstream-util validate-relax --nonet %{buildroot}%{_datadir}/metainfo/org.lucerna.Lucerna.metainfo.xml

%files
%license LICENSE
%doc README.md CHANGELOG.md docs/MANUAL-ACCEPTANCE.md docs/TROUBLESHOOTING.md
%{_bindir}/lucerna
%{_bindir}/lucernad
%{_bindir}/lucernactl
%{_mandir}/man1/lucerna.1*
%{_mandir}/man1/lucernad.1*
%{_mandir}/man1/lucernactl.1*
%{_datadir}/applications/org.lucerna.Lucerna.desktop
%{_datadir}/icons/hicolor/scalable/apps/org.lucerna.Lucerna.svg
%{_datadir}/metainfo/org.lucerna.Lucerna.metainfo.xml

# The changelog entry is appended by scripts/build-rpm.sh from the release date in CHANGELOG.md.
%changelog
