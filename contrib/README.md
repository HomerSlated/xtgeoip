# contrib/ — packaging recipes

Recipes live here. Nothing in this directory is built or installed by this
project; it is what downstream packagers copy or adapt.

Rationale: `docs/design/packaging.md` §6.3. A recipe that lives only in a
distribution's own tree and is never built here drifts from the install set
without anything reporting it.

## Read the manifest; do not restate the file list

The install set is declared once, in `docs/spec/install.yaml`, and
`xtgeoip-docgen` emits `docs/generated/install-manifest.tsv` from it. A recipe
should consume that manifest rather than repeating paths and modes. Nine files
restated across eight recipe formats is 72 statements that nothing verifies.

The manifest is tab-separated, one entry per line, comments begin with `#`:

```
kind    source                                 transform  dest                          mode
file    target/release/xtgeoip                 strip      /usr/bin/xtgeoip              0755
license LICENSE                                -          -                             -
dir     -                                      -          /var/lib/xt_geoip             0755
```

A recipe's install step is then a loop, not a list. `contrib/debian/rules` is
the worked example; this is its shape:

```sh
sed -e '/^#/d' -e '/^$/d' docs/generated/install-manifest.tsv \
| while IFS="$(printf '\t')" read -r kind src transform dest mode; do
    case "$transform" in
        none|-) ;;
        strip)  ;;                      # your packaging system, or strip(1)
        gzip)   dest="${dest%.gz}" ;;   # ditto — see the next section
        *) echo "unknown transform '$transform'" >&2; exit 1 ;;
    esac
    case "$kind" in
        dir)  install -d -m "$mode" -- "$DESTDIR$dest" ;;
        file) install -D -m "$mode" -- "$src" "$DESTDIR$dest" ;;
        license) ;;                     # your format's rule — see below
        *) echo "unknown kind '$kind'" >&2; exit 1 ;;
    esac
done
```

Four details that are not incidental:

- **`--` before the operands.** GNU `install` permutes options among operands,
  so a `source` beginning with `-` is read as a flag: `install -D -m 0755
  "-t/etc/cron.d" "$staged"` exits 0 having copied the file *outside* the
  staging root (measured on coreutils 9.4). `xtgeoip-docgen` now rejects such a
  `source` before it reaches the manifest, so this is defence in depth — but a
  recipe should not depend on the generator to be safe.
- **A `*)` arm on both `case`s.** Without one, a `kind` or `transform` added to
  `install.yaml` later is skipped in silence and the package is quietly missing
  a file. That is the failure this whole convention exists to prevent, arriving
  through the recipe instead of around it. The build fails on an unknown value
  too — see `tests::install_manifest_transforms_are_known` — and
  `tests::every_recipe_has_an_arm_for_every_kind` fails if a recipe here
  lacks an arm for a kind the manifest uses.
- **`IFS="$(printf '\t')"`, not `IFS=$'\t'`.** The latter is a bashism, and
  `debian/rules` and most `%install` scripts run under `sh`.
- **Quote every expansion of `src` and `dest`.** `xtgeoip-docgen` checks their
  *shape* (a relative `source` that climbs no `..` and does not start with
  `-`; an absolute `dest` that climbs no `..`; no control characters in
  either) but not their *characters*. Spaces, glob characters, `$(...)`,
  backslashes and `~` all pass, and are inert today only because the loop
  above quotes `"$src"` and `"$DESTDIR$dest"`. That is a property of the
  recipe, not of the manifest. A recipe that word-splits a field, passes it
  through `eval`, or writes it into a context with its own expansion rules —
  an unquoted Makefile line, or an rpm `%files` list, which treats `*` as a
  glob — has to supply that safety itself. Both recipes here quote; the PKGBUILD
  uses bash, so it reads with `IFS=$'\t'`, which is fine there.

## The two transforms are yours to perform

`transform` is not decoration. Two of the nine entries do not exist on disk in
the form they ship:

- `strip` — the release binary must be stripped at package time. It is
  deliberately *not* stripped by `Cargo.toml`, because Debian and Fedora
  extract `-dbgsym` / `-debuginfo` from what they remove, and pre-stripping
  silently produces an empty debug package.
- `gzip` — the man page installs compressed.

A loop that ignores the `transform` column will install an unstripped binary
and an uncompressed man page. Both work; both will be flagged in review.

## The licence has no path

A `license` row names a file and nothing else; its `transform`, `dest` and
`mode` are all `-`. No two formats agree on where a licence goes, or on
whether it is a file at all, so the manifest does not pretend to know:

| Format | Where the licence comes from |
|---|---|
| deb | `debian/copyright`, installed by `dh_installdocs` |
| rpm | `%license LICENSE` → `/usr/share/licenses/xtgeoip/` |
| pacman | `install` into `/usr/share/licenses/xtgeoip/` |
| xbps | `vlicense LICENSE` |
| ebuild | `LICENSE="MIT"`; no file is installed |

The row exists so that your recipe has to answer the question. MIT requires
the text to accompany every copy, and your loop's `*)` arm will stop the build
until it has a `license)` arm. In all five recipes here that arm does nothing
and the format's own mechanism, outside the loop, installs the file. Until
v0.4.4 the licence was an ordinary `file` row at `/usr/share/doc/xtgeoip/`,
and every package built from it carried the text twice or was flagged for it.

## What not to package

- **`/etc/xtgeoip.conf`.** Not a packaged file, and this is load-bearing —
  `conf --set-credentials` rewrites it in place and leaves it 0600 with
  ciphertext in it. As a dpkg conffile or an rpm `%config` every upgrade would
  diff it against the shipped original and either prompt or relocate live
  credentials. It is created on demand by `conf --show`, `--edit` and
  `--set-credentials`. See §2.
- **`extra/dkms/`.** A vendored copy of xtables-addons' `xt_geoip` module —
  third-party, GPL-2, and already packaged by every distribution here.
  *Depend* on `xtables-addons`; do not ship a second copy of it.
- **`xtgeoip-tests` and `xtgeoip-docgen`.** Development tools with no meaning
  on an installed system.

## Build notes

- `cargo build --release --locked`. The `--locked` matters: six credential-path
  crates are exact-pinned on purpose, and a packager who resolves fresh undoes
  that without noticing.
- Build-depends on a **C compiler** — `aws-lc-sys` is compiled for the TLS
  backend. **Not cmake**: it takes its pregenerated-source `cc` path, and the
  release build succeeds on a machine with no cmake installed.
  On Debian this needs no `Build-Depends` line at all: gcc arrives with
  `build-essential`, which Policy makes implicit and then forbids listing. Each
  of the other seven formats has to decide this for itself.
- Runtime-depends on **`xtables-addons`** (or the distribution's name for the
  `xt_geoip` match). The data files this produces are useless without it.
  That name splits in the Debian family, which is worth knowing before writing
  the other recipes: `xtables-addons-common` carries the userspace match
  extension and is a hard dependency, while the kernel module is a separate
  `xtables-addons-dkms` (or `-source`) and is a `Recommends`. Verified against
  the archive on 2026-09-22, not assumed.
- Build from **`v0.4.2` or later**, the first tag that carries a recipe.
  Nothing older exists: `v0.3.0`, `v0.4.0` and `v0.4.1` were deleted on
  2026-10-02, each unsuitable for a recipe. A clone made before then may
  still hold them, because `git fetch` does not remove tags deleted upstream.
  Do not build from any of them.

  From `v0.4.3` each GitHub release carries `xtgeoip-<version>.tar.gz`, which
  is `git archive` of the tag, and a `SHA256SUMS` signed by the release key
  `01282FB9C23478CF97A8D9041727776DA3AF9DF9` (public half in
  `docs/release_public.asc`).

  `publish = false`, so there is no crates.io tarball either way.
- Roll the tarball with `git archive`, not `tar czf`. `export-ignore` is an
  attribute `git archive` consults; anything that copies files from a working
  tree silently reincludes everything it was added to keep out.

## Status

| Format | State |
|---|---|
| deb | `debian/`, written 2026-09-22, first tagged in `v0.4.2`. Built end to end on 2026-09-29, both with the network and offline from a vendored component tarball. |
| pacman | `arch/PKGBUILD`, written 2026-10-02. **Not built**: no Arch system was available. Its shell logic was exercised outside makepkg against the v0.4.3 release (signature, checksum, manifest loop, both `*)` arms), which is not a build. Pins v0.4.3. |
| rpm | `rpm/xtgeoip.spec`, written 2026-10-02 and built 2026-10-04 on Fedora Copr (build 11073816 of `hazensparkle/xtgeoip`) for `fedora-44-x86_64` and `opensuse-tumbleweed-x86_64`, both with rustc 1.98.1 and GCC 16. On each: `cargo build --release --locked` with network access, 292 tests passed, and the package holds the manifest's 11 rows at their modes plus the licence, with the logrotate file `%config(noreplace)`, the man page gzipped by `brp-compress`, `Conflicts: xtables-geoip`, `Recommends: xtables-addons`, and a 20 MB debuginfo package. On Fedora `%openpgpverify` checked the release key's signature (1 of 1 valid); on Tumbleweed only the checksum ran, as designed. **Not built**: `--with vendor`. Not installed, and `rpmlint` not run. Distribution-neutral; `Version` tracks `Cargo.toml`. |
| xbps | `void/srcpkgs/xtgeoip/template`, written and built 2026-10-02 on Void x86_64 (glibc) by a delegated session: `xbps-src -Q pkg` passed with 292 tests, `xlint` clean, the binary stripped, the manifest's 9 files plus the licence. Not installed, so the `make_dirs` trigger was not exercised. Pins v0.4.3. |
| ebuild | `gentoo/net-firewall/xtgeoip/xtgeoip-0.4.3.ebuild`, written 2026-10-02. **Not built**: no Gentoo system was available. Its `src_install` was run with Portage's install helpers stubbed against the v0.4.3 release (every manifest row, both `*)` arms), which is not a build. No Manifest is kept; `LICENSE` lists only MIT, not the crates' licences. Pins v0.4.3. |
| apk, nix, SlackBuild | not started |

Every recipe gained a `license)` arm on 2026-10-04, after each build recorded
above. The builds do not cover it: they read v0.4.3's manifest, which has no
`license` row, so the arm was never reached.

`debian/README.source` is the one to read before writing another: it is where
what this recipe cost gets written down. Four things it surfaced that every
other format has to answer:

1. **The two transforms may already be someone else's job.** Debian's
   `dh_strip` and `dh_compress` do exactly what `strip` and `gzip` name, so
   `debian/rules` performs neither — it rewrites the man page's `dest` to drop
   `.gz` and lets `dh_compress` put it back at `-9n`. The manifest declares an
   *end state*; each format reaches it by its own road. rpm's `brp-strip` and
   `brp-compress` and xbps-src's strip hook are the same again, with one
   exception: Void ships man pages uncompressed and gunzips any it finds, so
   there the `gzip` end state is overridden, not reached.
2. **Not `dh --buildsystem=cargo`.** It builds against Debian's own crate
   registry, which re-resolves — the exact thing `--locked` is there to stop.
   Every format with a native Rust helper needs the same question asked, and
   the answers so far differ. Fedora's `%cargo_prep` is worse than `dh-cargo`:
   without a vendor directory it runs `rm -f Cargo.lock` before building
   against Fedora's packaged crates, and `%cargo_build` never passes
   `--locked`, so the spec calls cargo itself. openSUSE's
   [Rust packaging page](https://en.opensuse.org/openSUSE:Packaging_Rust_Software)
   (read 2026-10-04) reaches the same verdict from the other side: a spec
   meant for Fedora too should avoid Fedora's macros, and `cargo_prep`
   "especially is known to break vendoring". openSUSE's own route has the
   fault in a different place. Its vendor tarball comes from the
   `cargo_vendor` source service, which defaults to `update=true` and
   `respect-lockfile=false` and ships the resulting `Cargo.lock` inside the
   tarball; that page's example sets `update` to `true` and offers `false`
   only as a cure for dependency conflicts. A build there needs both
   parameters reversed, or a tarball rolled by hand with `cargo vendor
   --locked`, which is what the spec's `--with vendor` expects. Gentoo's
   `cargo_src_compile` does not pass `--locked` either, but it passes its
   arguments on, so the ebuild calls `cargo_src_compile --locked`; it builds
   from a vendor directory of exactly the crates listed in `CRATES`,
   generated from `Cargo.lock`. Void's `build_style=cargo` is
   the exception that is safe: it runs `cargo auditable build --release
   --locked` against crates.io, so the template keeps it and replaces only
   its `do_install`, which would install every binary target.
3. **`-dbgsym` is empty as configured.** `Cargo.toml` sets no
   `[profile.release]`, so Cargo's release default gives no debug info and
   there is nothing for `dh_strip` to extract. The "do not pre-strip" rule in
   the notes above is still right, but it does not pay for this package until
   somebody turns debug info on. `debian/rules` suppresses the empty package
   and says how to get a real one.
4. **Feeding 306 crates to a network-less build machine.** `cargo vendor`
   into the source package, which makes it a licensing question as well as a
   build one: every vendored crate then needs accounting for. The build half
   has a trap in the Debian case, and possibly others: vendored crates ship
   `Cargo.toml.orig`, and anything that tidies `*.orig` away (as `dh_clean`
   does by default) breaks their checksums.

The PKGBUILD added three more, none of which Debian raised:

5. **A distribution's default C flags can break the link.** Arch builds with
   `-flto=auto`, which reaches `aws-lc-sys` through `CFLAGS` and leaves objects
   `rust-lld` cannot resolve; the PKGBUILD sets `!lto`. The detail that
   matters is fat versus slim objects: Fedora's `-flto=auto -ffat-lto-objects`
   keeps machine code beside the LTO bytecode, and the same experiment linked
   (GCC 13, 2026-10-02), as did the real build on Fedora 44 (GCC 16,
   2026-10-04). Copr's Tumbleweed chroot exported no `-flto`, which says
   nothing about the openSUSE Build Service's own flags. Void's defaults
   enable no LTO at all, and Gentoo's
   `cargo.eclass` runs `filter-lto` on every Rust build for this reason.
6. **The runtime dependency may not be one package.** On Arch, xtables-addons
   is AUR-only, as two conflicting packages with no shared `provides`, so it
   is an `optdepends` naming both. "Depend on xtables-addons" is a goal; each
   format has to find out what it can actually express. Gentoo can say
   exactly what is needed: `net-firewall/xtables-addons[xtables_addons_geoip]`.
7. **A recipe inside the tarball cannot pin the tarball.** Formats that carry
   source checksums (PKGBUILD, APKBUILD, ebuild Manifest, SlackBuild .info)
   pin the latest published release and are moved forward after each one.
   Debian does not have the problem: `debian/` states no hash, and the one in
   the `.dsc` is written by `dpkg-source` at build time, and rpm keeps its
   checksums in the packager's `sources` file, so the spec's `Version` can
   track `Cargo.toml`.

The rpm spec and the Void template added four more:

8. **Another package may own the output directory.** openSUSE's
   `xtables-geoip` ships prebuilt DB-IP data into `/usr/share/xt_geoip`,
   xtgeoip's default `output_dir`, and owns it; each would overwrite the
   other's database. The spec declares `Conflicts: xtables-geoip`. Check each
   distribution for a data package of its own.
9. **An empty directory may not survive packaging.** xbps-src deletes empty
   directories from a package, so `/var/lib/xt_geoip` comes from `make_dirs`
   at install time instead, and the template fails the build if the manifest
   and `make_dirs` disagree. That line is the one restatement in any recipe
   here, and it is guarded. Gentoo leaves empty directories undefined too;
   the ebuild turns every `dir` row into `keepdir`.
10. **The distribution may link a -sys crate against its own library.**
    Void's Rust helper exports `ZSTD_SYS_USE_PKG_CONFIG=1`, so the package
    links the system `libzstd` and needs `pkg-config` and `libzstd-devel` to
    build. The Debian package links no libzstd (its dependencies are libc,
    libgcc and xtables-addons-common), so there the bundled copy is used.
11. **The binary may not be where the manifest says.** Void's cargo style
    always passes `--target`, so the release binary is under
    `target/<triple>/release/`, and the template rewrites that one source
    path. Gentoo's eclass does the same when it needs to and says where with
    `cargo_target_dir`. Gentoo also puts docs under `/usr/share/doc/${PF}`,
    so the ebuild rewrites that prefix as well.
