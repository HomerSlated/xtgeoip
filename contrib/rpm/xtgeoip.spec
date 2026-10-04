# xtgeoip.spec — build xtgeoip as an RPM.
#
# Written 2026-10-02 from Fedora's cargo-rpm-macros 28.5 and
# redhat-rpm-config, rpm's own brp-compress and macros.in, and Fedora's and
# openSUSE's package archives. Built 2026-10-04 on Fedora Copr (build
# 11073816 of hazensparkle/xtgeoip) for fedora-44-x86_64 and
# opensuse-tumbleweed-x86_64, as it stood at commit e5f35a4; only comments
# and the changelog wording have changed since. That build took the default
# path, with network access. NOT BUILT: `--with vendor`. NOT DONE: an
# install, an upgrade, rpmlint. Read contrib/README.md's status table before
# relying on it.
#
# Distribution-neutral on purpose: Fedora, openSUSE and RHEL each have their
# own Rust macros, and the one this file would otherwise use is unsafe here.
# Fedora's %%cargo_prep runs `rm -f Cargo.lock` unless it is given a vendor
# directory, then builds against Fedora's packaged crates, and %%cargo_build
# never passes --locked. That is the re-resolve Debian's dh-cargo was rejected
# for (packaging.md §5): six credential-path crates are exact-pinned because an
# encrypted blob in /etc/xtgeoip.conf outlives any build, and a fresh resolve
# changes its format without any test noticing. So this calls cargo directly,
# as debian/rules does. A Fedora-proper package would also have to declare
# every vendored crate as bundled(crate()); this file does not.
#
# Unlike the PKGBUILD, nothing here pins a checksum: rpm keeps those in the
# packager's `sources` file, outside the tarball. So Version tracks Cargo.toml.
#
# The install step reads docs/generated/install-manifest.tsv, the one
# declaration of what the package ships (docs/design/packaging.md §6.3).

# Build offline from a vendor tarball the packager rolls, rather than over the
# network: `rpmbuild --with vendor`. Roll it from the unpacked release with
#   cargo vendor --locked vendor
#   tar --sort=name --owner=0 --group=0 --numeric-owner \
#       -cJf xtgeoip-VERSION-vendor.tar.xz vendor
# Upstream ships no vendor tarball (packaging.md §4): it adds nothing the
# lockfile cannot reproduce.
%bcond_with vendor

Name:           xtgeoip
Version:        0.4.3
Release:        1%{?dist}
Summary:        GeoLite2 database builder for the xt_geoip iptables match
License:        MIT
URL:            https://github.com/HomerSlated/xtgeoip

%global relurl %{url}/releases/download/v%{version}
Source0:        %{relurl}/%{name}-%{version}.tar.gz
# Renamed per version so that one release's checksum file cannot stand in for
# another's in a shared SOURCES directory.
Source1:        %{relurl}/SHA256SUMS#/%{name}-%{version}-SHA256SUMS
Source2:        %{relurl}/SHA256SUMS.asc#/%{name}-%{version}-SHA256SUMS.asc
# The release key, 01282FB9C23478CF97A8D9041727776DA3AF9DF9. It is not on a
# keyserver. Fedora wants the keyring committed to the package SCM; check its
# fingerprint against a second source before committing it.
%global keyfpr 01282FB9C23478CF97A8D9041727776DA3AF9DF9
Source3:        %{url}/raw/v%{version}/docs/release_public.asc#/%{name}-keyring-%{keyfpr}.asc
%if %{with vendor}
Source10:       %{name}-%{version}-vendor.tar.xz
%endif

%{?rust_arches:ExclusiveArch: %{rust_arches}}

# edition 2024 sets the floor; upstream declares no tested MSRV.
BuildRequires:  cargo
BuildRequires:  rust >= 1.85
# aws-lc-sys compiles C. Not cmake: it takes its pregenerated-source cc path.
BuildRequires:  gcc
%if 0%{?fedora}
BuildRequires:  openpgpverify
%endif

# Recommends rather than Requires: xtgeoip writes the database files whether or
# not the match is installed, and xtables-addons, which reads them, is not in
# Fedora proper. It is in RPM Fusion and in openSUSE, under that name in both.
Recommends:     xtables-addons
# openSUSE's xtables-geoip ships prebuilt DB-IP data into /usr/share/xt_geoip,
# which is xtgeoip's default output_dir, and owns that directory. Installed
# together, each would overwrite the other's database. No package has this
# name on Fedora or RHEL, so there it constrains nothing.
Conflicts:      xtables-geoip

%description
xtgeoip downloads MaxMind's GeoLite2 country CSV database and converts it into
the binary per-country IP range files that the xt_geoip netfilter match reads,
letting iptables and nftables rules select traffic by country.

%prep
# Verification first, before anything from the tarball runs. The signature
# covers SHA256SUMS, not the tarball; the sha256sum line ties the two. Only
# Fedora has openpgpverify, so elsewhere the check is consistency, not origin.
%if 0%{?fedora}
%openpgpverify -k3 -s2 -d1
%endif
(cd "$(dirname '%{SOURCE0}')" && sha256sum -c --strict '%{SOURCE1}')
%setup -q
%if %{with vendor}
%setup -q -T -D -a 10
%endif

%build
%{?set_build_flags}
# Keep cargo's registry and config inside the build tree, out of ~/.cargo.
export CARGO_HOME="$PWD/.cargo-home"
%if %{with vendor}
mkdir -p "$CARGO_HOME"
cat > "$CARGO_HOME/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$PWD/vendor"
EOF
%endif
# Cargo's release profile carries no DWARF, which leaves find-debuginfo
# nothing to extract (Debian met the same: an empty -dbgsym). Level 2 matches
# Fedora's own %%rustflags_debuginfo; rpm strips the binary afterwards.
export CARGO_PROFILE_RELEASE_DEBUG=2
cargo build %{?_smp_mflags} --release --locked %{?with_vendor:--offline}

%check
export CARGO_HOME="$PWD/.cargo-home"
# The hermetic suite: no root, no network. xtgeoip-tests, the integration
# suite, needs root, openssl(1) and a passphrase typed at a terminal.
cargo test %{?_smp_mflags} --locked %{?with_vendor:--offline}

%install
# One pass over the manifest installs every row and writes the %%files list,
# so neither can drift from the other. Two transforms are rpm's own:
#   strip  brp-strip and find-debuginfo strip /usr/bin/xtgeoip.
#   gzip   brp-compress compresses man pages (gzip -9 -n by default), so the
#          man page installs without .gz and is listed with a trailing glob,
#          which also covers a distribution that compresses differently.
# Both `case`s on kind and transform end in `*)` so that a value added to the
# manifest later fails the build instead of vanishing from the package.
manifest=docs/generated/install-manifest.tsv
filelist=%{name}.files
docdirs=
: > "$filelist"
sed -e '/^#/d' -e '/^$/d' "$manifest" > manifest.rows
while IFS="$(printf '\t')" read -r kind src transform dest mode; do
	# xtgeoip-docgen checks dest's shape, not its characters, and a %%files
	# line expands globs and splits on whitespace. Refuse anything that is
	# not plainly literal there.
	case "$dest" in
	*[!A-Za-z0-9._/+-]*)
		echo "$manifest: '$dest' has a character %%files would interpret" >&2
		exit 1 ;;
	esac
	glob=
	case "$transform" in
	none|-) ;;
	strip) ;;
	gzip) dest="${dest%%.gz}"; glob='*' ;;
	*) echo "$manifest: unknown transform '$transform'" >&2; exit 1 ;;
	esac
	config=
	case "$dest" in
	/etc/*) config='%%config(noreplace) ' ;;
	esac
	case "$kind" in
	dir)
		install -d -m "$mode" -- "%{buildroot}$dest" || exit 1
		echo "%%dir %%attr($mode,root,root) $dest" >> "$filelist" ;;
	file)
		install -D -m "$mode" -- "$src" "%{buildroot}$dest" || exit 1
		echo "$config%%attr($mode,root,root) $dest$glob" >> "$filelist"
		# The package must own the doc directory it creates.
		case "$dest" in
		/usr/share/doc/*/*)
			d="${dest%%/*}"
			case " $docdirs " in
			*" $d "*) ;;
			*) docdirs="$docdirs $d"
			   echo "%%dir $d" >> "$filelist" ;;
			esac ;;
		esac ;;
	*) echo "$manifest: unknown kind '$kind'" >&2; exit 1 ;;
	esac
done < manifest.rows

%files -f %{name}.files
# Not in the manifest: where a licence goes is per-format. rpm installs this to
# %%{_datadir}/licenses/%%{name}; Debian reads debian/copyright instead.
%license LICENSE

%changelog
* Fri Oct 02 2026 xtgeoip packaging <packaging@example.invalid> - 0.4.3-1
- Initial packaging, from contrib/rpm in the upstream tree.
