# xtgeoip-0.4.4.ebuild — build xtgeoip as a Gentoo package.
#
# Written 2026-10-02 from cargo.eclass (gentoo.git master), the devmanual and
# the net-firewall/xtables-addons ebuild. Built 2026-10-04 in a gentoo/stage3
# container by .github/workflows/packaging.yml (run 37240800966, at commit
# 9cd7980): `ebuild ... manifest`, then `clean test install`, `qmerge` and
# `pkgcheck scan`, with rust-bin 1.97.1. That build was of v0.4.3, under
# that name. Since then: comments, and on 2026-10-06 the rename to 0.4.4;
# CRATES is unchanged, because the two lockfiles differ only in xtgeoip's
# own version. NOT BUILT at 0.4.4, the first release whose manifest has the
# `license` row that the arm in src_install answers. `ebuild` resolves no
# dependencies, so RDEPEND was not exercised. Read contrib/README.md's
# status table before relying on it.
#
# Not in ::gentoo or any overlay. Copy it into an overlay as
# net-firewall/xtgeoip/ and run `ebuild xtgeoip-0.4.4.ebuild manifest` there;
# no Manifest is kept here, because it would restate every crate's hash and
# the tarball's.
#
# The version is in the file name and the crates below are this release's
# Cargo.lock, so this file pins the latest *published* release, not the tree
# it sits in: it ships inside the tarball it builds. After each release a
# follow-up commit renames it and regenerates CRATES (pycargoebuild does the
# same).
#
# Portage builds with no network, so every crate is a distfile. CRATES is
# generated from Cargo.lock, never edited by hand: 306 registry crates, no git
# crates, at v0.4.4.

EAPI=8

CRATES="
	adler2@2.0.1
	aead@0.6.1
	aes@0.9.3
	ahash@0.8.12
	android_system_properties@0.1.6
	annotate-snippets@0.12.16
	anstream@1.0.0
	anstyle-parse@1.0.0
	anstyle-query@1.1.5
	anstyle-wincon@3.0.11
	anstyle@1.0.14
	anyhow@1.0.104
	argon2@0.5.3
	arraydeque@0.5.1
	arrayvec@0.7.8
	async-compression@0.4.44
	atomic-waker@1.1.2
	autocfg@1.5.1
	aws-lc-rs@1.18.1
	aws-lc-sys@0.45.0
	base64@0.22.1
	base64ct@1.8.3
	bitflags@2.13.1
	blake2@0.10.6
	blake3@1.8.7
	block-buffer@0.10.4
	block-buffer@0.12.1
	bumpalo@3.20.3
	bytes@1.12.1
	bzip2@0.6.1
	cc@1.4.4
	cfg-if@1.0.4
	cfg_aliases@0.2.2
	chacha20@0.10.2
	chacha20poly1305@0.11.0
	chrono@0.4.45
	cipher@0.5.2
	clap@4.6.6
	clap_builder@4.6.6
	clap_complete@4.6.11
	clap_derive@4.6.4
	clap_lex@1.1.0
	cmake@0.1.58
	cmov@0.5.4
	colorchoice@1.0.5
	combine@4.6.8
	compression-codecs@0.4.39
	compression-core@0.4.33
	const-oid@0.10.2
	constant_time_eq@0.4.2
	core-foundation-sys@0.8.7
	core-foundation@0.10.1
	core-foundation@0.9.4
	cpubits@0.1.1
	cpufeatures@0.2.17
	cpufeatures@0.3.1
	crc32fast@1.5.1
	crossbeam-deque@0.8.7
	crossbeam-epoch@0.9.20
	crossbeam-utils@0.8.22
	crypto-common@0.1.7
	crypto-common@0.2.2
	csv-core@0.1.13
	csv@1.4.0
	ctutils@0.4.2
	deflate64@0.1.12
	deranged@0.5.8
	digest@0.10.7
	digest@0.11.3
	displaydoc@0.2.7
	dunce@1.0.5
	either@1.18.0
	encoding_rs@0.8.35
	encoding_rs_io@0.1.8
	equivalent@1.0.2
	errno@0.3.14
	fastrand@2.5.0
	fern@0.7.1
	filetime@0.2.29
	find-msvc-tools@0.1.11
	flate2@1.1.10
	fnv@1.0.7
	form_urlencoded@1.2.2
	fs_extra@1.3.0
	futures-channel@0.3.34
	futures-core@0.3.34
	futures-io@0.3.34
	futures-sink@0.3.34
	futures-task@0.3.34
	futures-util@0.3.34
	generic-array@0.14.7
	getrandom@0.2.17
	getrandom@0.3.4
	getrandom@0.4.3
	granit-parser@0.0.7
	h2@0.4.19
	hashbrown@0.17.1
	heck@0.5.0
	hmac@0.13.0
	hostname@0.4.2
	http-body-util@0.1.5
	http-body@1.1.0
	http@1.5.0
	httparse@1.10.1
	hybrid-array@0.4.14
	hyper-rustls@0.27.9
	hyper-util@0.1.20
	hyper@1.11.1
	iana-time-zone-haiku@0.1.2
	iana-time-zone@0.1.65
	icu_collections@2.3.0
	icu_locale_core@2.3.0
	icu_normalizer@2.3.0
	icu_normalizer_data@2.3.0
	icu_properties@2.3.0
	icu_properties_data@2.3.0
	icu_provider@2.3.1
	idna@1.1.0
	idna_adapter@1.2.2
	indexmap@2.14.1
	inout@0.2.2
	ipnet@2.12.1
	ipnetwork@0.21.1
	is_terminal_polyfill@1.70.2
	itoa@1.0.18
	jni-macros@0.22.4
	jni-sys-macros@0.4.1
	jni-sys@0.4.1
	jni@0.22.4
	jobserver@0.1.35
	js-sys@0.3.104
	libbz2-rs-sys@0.2.5
	libc@0.2.189
	linux-raw-sys@0.12.1
	litemap@0.8.3
	log@0.4.34
	lru-slab@0.1.2
	lzma-rust2@0.16.5
	memchr@2.8.3
	memmap2@0.9.11
	mime@0.3.17
	miniz_oxide@0.9.1
	mio@1.2.3
	nohash-hasher@0.2.0
	num-conv@0.2.2
	num-traits@0.2.19
	num_threads@0.1.7
	once_cell@1.21.4
	once_cell_polyfill@1.70.2
	openssl-probe@0.2.1
	password-hash@0.5.0
	pbkdf2@0.13.0
	percent-encoding@2.3.2
	pin-project-lite@0.2.17
	pkg-config@0.3.34
	poly1305@0.9.1
	potential_utf@0.1.6
	powerfmt@0.2.0
	ppmd-rust@1.4.1
	proc-macro2@1.0.107
	quinn-proto@0.11.17
	quinn-udp@0.5.15
	quinn@0.11.11
	quote@1.0.47
	r-efi@5.3.0
	r-efi@6.0.0
	rand@0.10.2
	rand_core@0.10.1
	rand_core@0.6.4
	rand_pcg@0.10.2
	rayon-core@1.13.0
	rayon@1.12.0
	reqwest@0.13.4
	ring@0.17.14
	rpassword@7.5.4
	rtoolbox@0.0.6
	rustc-hash@2.1.3
	rustc_version@0.4.1
	rustix@1.1.4
	rustls-native-certs@0.8.4
	rustls-pki-types@1.15.1
	rustls-platform-verifier-android@0.1.1
	rustls-platform-verifier@0.7.0
	rustls-webpki@0.103.15
	rustls@0.23.45
	rustversion@1.0.23
	ryu@1.0.23
	same-file@1.0.6
	schannel@0.1.29
	secrecy@0.10.3
	security-framework-sys@2.17.0
	security-framework@3.7.0
	semver@1.0.28
	serde-saphyr@0.0.29
	serde@1.0.229
	serde_core@1.0.229
	serde_derive@1.0.229
	serde_json@1.0.151
	serde_spanned@1.1.1
	sha1@0.11.0
	sha2@0.10.9
	sha2@0.11.0
	shlex@2.0.1
	simd-adler32@0.3.10
	simd_cesu8@1.2.0
	simdutf8@0.1.5
	slab@0.4.12
	smallvec@1.15.2
	socket2@0.6.5
	stable_deref_trait@1.2.1
	strsim@0.11.1
	subtle@2.6.1
	syn@2.0.119
	syn@3.0.4
	sync_wrapper@1.0.2
	synstructure@0.13.2
	syslog@7.0.0
	system-configuration-sys@0.6.0
	system-configuration@0.7.0
	tar@0.4.46
	tempfile@3.27.0
	thiserror-impl@2.0.20
	thiserror@2.0.20
	time-core@0.1.9
	time-macros@0.2.32
	time@0.3.55
	tinystr@0.8.4
	tinyvec@1.13.0
	tinyvec_macros@0.1.1
	tokio-rustls@0.26.4
	tokio-util@0.7.19
	tokio@1.53.1
	toml@1.1.5+spec-1.1.0
	toml_datetime@1.1.1+spec-1.1.0
	toml_edit@0.25.13+spec-1.1.0
	toml_parser@1.1.3+spec-1.1.0
	toml_writer@1.1.2+spec-1.1.0
	tower-http@0.6.11
	tower-layer@0.3.3
	tower-service@0.3.3
	tower@0.5.3
	tracing-core@0.1.36
	tracing@0.1.44
	try-lock@0.2.5
	typed-path@0.12.3
	typenum@1.20.1
	unicode-ident@1.0.24
	unicode-width@0.2.2
	universal-hash@0.6.1
	untrusted@0.9.0
	url@2.5.8
	utf8_iter@1.0.4
	utf8parse@0.2.2
	version_check@0.9.5
	walkdir@2.5.0
	want@0.3.1
	wasi@0.11.1+wasi-snapshot-preview1
	wasip2@1.0.4+wasi-0.2.12
	wasm-bindgen-futures@0.4.77
	wasm-bindgen-macro-support@0.2.127
	wasm-bindgen-macro@0.2.127
	wasm-bindgen-shared@0.2.127
	wasm-bindgen@0.2.127
	web-sys@0.3.104
	web-time@1.1.0
	webpki-root-certs@1.0.9
	winapi-util@0.1.11
	windows-core@0.62.2
	windows-implement@0.60.2
	windows-interface@0.59.3
	windows-link@0.2.1
	windows-registry@0.6.1
	windows-result@0.4.1
	windows-strings@0.5.1
	windows-sys@0.52.0
	windows-sys@0.61.2
	windows-targets@0.52.6
	windows_aarch64_gnullvm@0.52.6
	windows_aarch64_msvc@0.52.6
	windows_i686_gnu@0.52.6
	windows_i686_gnullvm@0.52.6
	windows_i686_msvc@0.52.6
	windows_x86_64_gnu@0.52.6
	windows_x86_64_gnullvm@0.52.6
	windows_x86_64_msvc@0.52.6
	winnow@1.0.4
	wit-bindgen@0.57.1
	writeable@0.6.4
	xattr@1.6.1
	yoke-derive@0.8.2
	yoke@0.8.3
	zerocopy-derive@0.8.56
	zerocopy@0.8.56
	zerofrom-derive@0.1.7
	zerofrom@0.1.8
	zeroize@1.9.0
	zerotrie@0.2.5
	zerovec-derive@0.11.6
	zerovec@0.11.8
	zip@8.6.0
	zlib-rs@0.6.7
	zmij@1.0.23
	zopfli@0.8.3
	zstd-safe@7.2.4
	zstd-sys@2.0.16+zstd.1.5.7
	zstd@0.13.3
"

# The floor is the highest `rust-version` any locked crate declares, which at
# v0.4.4 is 1.89 (aes 0.9.3, through zip). Not edition 2024's 1.85: cargo
# refuses to build a crate with an older compiler than it declares. Upstream
# declares the same number as `rust-version` in Cargo.toml from v0.4.4, and a
# test there fails if this line disagrees. Recompute after a lockfile change:
#   cargo metadata --format-version 1 --locked | jq -r \
#     '[.packages[].rust_version | select(.)] | max_by(split(".") | map(tonumber))'
RUST_MIN_VER="1.89.0"

inherit cargo

DESCRIPTION="GeoLite2 database builder for the xt_geoip iptables match"
HOMEPAGE="https://github.com/HomerSlated/xtgeoip"
SRC_URI="
	https://github.com/HomerSlated/xtgeoip/releases/download/v${PV}/${P}.tar.gz
	${CARGO_CRATE_URIS}
"

# Only the crate's own licence. Every crate above is linked in statically, and
# Gentoo expects their licences here too; pycargoebuild generates that list,
# and it has not been run.
LICENSE="MIT"
SLOT="0"
KEYWORDS="~amd64"

# Unlike Arch and Void, Gentoo can say exactly what is needed: the geoip match,
# whose library reads the files this package writes. xtables-addons builds its
# kernel module by default (USE=modules).
RDEPEND="net-firewall/xtables-addons[xtables_addons_geoip]"

# Arch's -flto=auto broke the final link through aws-lc-sys. cargo.eclass
# already runs filter-lto in cargo_env ("Rust extensions are incompatible with
# C/C++ LTO compiler"), so nothing to do here.

src_compile() {
	# --locked is load-bearing: six credential-path crates are exact-pinned,
	# because an encrypted blob in /etc/xtgeoip.conf outlives any build, and a
	# fresh resolve would change its format without any test noticing.
	# cargo_src_compile does not pass it, but it passes its arguments on.
	cargo_src_compile --locked
}

src_test() {
	# The hermetic suite: no root, no network. xtgeoip-tests, the integration
	# suite, needs root, openssl(1) and a passphrase typed at a terminal.
	cargo_src_test --locked
}

# Not cargo_src_install: that is `cargo install --path ./`, which would install
# every binary target, the two development tools included, and ignore the
# manifest. Every file and directory comes from
# docs/generated/install-manifest.tsv instead (packaging.md §6.3). Rewrites
# local to this recipe:
#   strip   nothing to do: Portage strips ELF files itself.
#   gzip    the .gz is dropped: Portage compresses /usr/share/man itself
#           (docompress), with whatever PORTAGE_COMPRESS names.
#   target/release   becomes $(cargo_target_dir), which accounts for the
#           eclass building with --target when it needs to.
#   /usr/share/doc/xtgeoip   becomes /usr/share/doc/${PF}, Gentoo's layout.
#   dir     keepdir, because the handling of empty directories is undefined
#           for package managers (devmanual); it leaves a .keep file.
# Both `case`s on transform and kind end in `*)` so that a value added to the
# manifest later fails the build instead of vanishing from the package.
src_install() {
	local manifest=docs/generated/install-manifest.tsv
	local kind src transform dest mode

	while IFS=$'\t' read -r kind src transform dest mode; do
		case "${transform}" in
			none|-) ;;
			strip) ;;
			gzip) dest="${dest%.gz}" ;;
			*) die "${manifest}: unknown transform '${transform}'" ;;
		esac
		case "${src}" in
			target/release/*) src="$(cargo_target_dir)/${src#target/release/}" ;;
		esac
		case "${dest}" in
			/usr/share/doc/${PN}/*) dest="/usr/share/doc/${PF}/${dest#/usr/share/doc/${PN}/}" ;;
		esac
		case "${kind}" in
			file)
				insinto "${dest%/*}"
				insopts -m"${mode}"
				newins "${src}" "${dest##*/}"
				;;
			dir)
				keepdir "${dest}"
				fperms "${mode}" "${dest}"
				;;
			# LICENSE= above names it, and the text is in the repository's
			# licenses/ directory. The devmanual: "There is no need for
			# dodoc COPYING!"
			license) ;;
			*) die "${manifest}: unknown kind '${kind}'" ;;
		esac
	done < <(sed -e '/^#/d' -e '/^$/d' "${manifest}" || die)
}
