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
        *) echo "unknown kind '$kind'" >&2; exit 1 ;;
    esac
done
```

Three details that are not incidental:

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
  too — see `tests::install_manifest_transforms_are_known`.
- **`IFS="$(printf '\t')"`, not `IFS=$'\t'`.** The latter is a bashism, and
  `debian/rules` and most `%install` scripts run under `sh`.

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
- Build from **`v0.4.1` or later**. The two earlier tags are both unsuitable,
  for unrelated reasons, and neither announces it:
  - `v0.3.0` predates `.gitattributes`. `export-ignore` is read from the tree
    being archived, so `git archive v0.3.0` still carries
    `extra/dkms/xt-geoip-3.30.tar.gz` — third-party GPL-2 source inside a
    release whose root `LICENSE` is MIT.
  - `v0.4.0` predates the emitter audit of 2026-09-20. Its `xtgeoip-docgen`
    writes `install-manifest.tsv` without validating the fields it
    interpolates, which is the finding that put a forged row — and therefore
    an arbitrary `install` line — into the manifest a recipe consumes. Since
    this file is that recipe's instructions, building from that tag defeats
    the point of reading it.

  `publish = false`, so there is no crates.io tarball either way.
- Roll the tarball with `git archive`, not `tar czf`. `export-ignore` is an
  attribute `git archive` consults; anything that copies files from a working
  tree silently reincludes everything it was added to keep out.

## Status

| Format | State |
|---|---|
| deb | `debian/`, written 2026-09-22 against `v0.4.1`. Not built end to end — no `debhelper` on the authoring machine. |
| pacman | next |
| rpm, xbps, ebuild, apk, nix, SlackBuild | not started |

`debian/README.source` is the one to read before writing another: it is where
what this recipe cost gets written down. Four things it surfaced that the
remaining seven will each have to answer:

1. **The two transforms may already be someone else's job.** Debian's
   `dh_strip` and `dh_compress` do exactly what `strip` and `gzip` name, so
   `debian/rules` performs neither — it rewrites the man page's `dest` to drop
   `.gz` and lets `dh_compress` put it back at `-9n`. The manifest declares an
   *end state*; each format reaches it by its own road. Expect the same of rpm.
2. **Not `dh --buildsystem=cargo`.** It builds against Debian's own crate
   registry, which re-resolves — the exact thing `--locked` is there to stop.
   Every format with a native Rust helper (`dh-cargo`, `%cargo_build`,
   `cargo.eclass`) needs the same question asked and probably answered the same
   way.
3. **`-dbgsym` is empty as configured.** `Cargo.toml` sets no
   `[profile.release]`, so Cargo's release default gives no debug info and
   there is nothing for `dh_strip` to extract. The "do not pre-strip" rule in
   the notes above is still right, but it does not pay for this package until
   somebody turns debug info on. `debian/rules` suppresses the empty package
   and says how to get a real one.
4. **Feeding 307 crates to a network-less build machine.** `cargo vendor`, and
   then a decision about whether the vendored tree goes in the source tarball —
   which is a licensing question, not a build one.
