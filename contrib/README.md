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

A recipe's install step is then a loop, not a list:

```sh
grep -v '^#' docs/generated/install-manifest.tsv | while IFS=$'\t' read -r kind src transform dest mode; do
    case "$kind" in
        dir)  install -d -m "$mode" "$DESTDIR$dest" ;;
        file) install -D -m "$mode" "$src" "$DESTDIR$dest" ;;
    esac
done
```

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
- Runtime-depends on **`xtables-addons`** (or the distribution's name for the
  `xt_geoip` match). The data files this produces are useless without it.
- Build from the `v0.4.0` tag or later — **not** from `v0.3.0`. The tarball
  exclusions in `.gitattributes` were added after that tag was cut, and
  `export-ignore` is read from the tree being archived, so `git archive v0.3.0`
  still carries `extra/dkms/xt-geoip-3.30.tar.gz`: third-party GPL-2 source
  inside a release whose root `LICENSE` is MIT. `publish = false`, so there is
  no crates.io tarball either way.
- Roll the tarball with `git archive`, not `tar czf`. `export-ignore` is an
  attribute `git archive` consults; anything that copies files from a working
  tree silently reincludes everything it was added to keep out.

## Status

Nothing here yet. `debian/` and `PKGBUILD` first — they are the least
ceremonious of the eight formats and cover the most users, and writing them
will surface whatever the other six also need.
