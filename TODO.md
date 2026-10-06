# TODO

Open work only. Everything closed up to 2026-09-05 is archived in
[`DONE.md`](DONE.md) (full history and reasoning) and
[`DONE_tldr.md`](DONE_tldr.md) (the summary that accompanied it). Tickets that
close here move to the end of `DONE.md`, under *Closed after the archive*;
there is no `TODO_tldr.md` any more.

Opened 2026-09-06, brought up to date 2026-10-05. What is here now: the open
packaging work (three recipes unwritten, every written one built), one wording
correction waiting for its file to change, the informational findings from
three guardian audits, one policy question, and the three lists that are normative
rather than historical.

---

## INVARIANTS

Carried forward from `DONE.md` unchanged. These are cited as normative by the
`optimisation-advisor` and `bug-hunter` agent definitions — constraint 5 in
particular — so they must live in the current file, not the archive.

Any refactoring, optimisation, or cleanup must be evaluated in this order of
precedence. A change that violates a higher-priority constraint must not be
made, regardless of other benefits:

1. **No hard errors** — no segfaults, panics, or undefined behaviour
2. **No soft errors** — the function must still work correctly
3. **Not unsafe** — no potential memory leaks or unsound code
4. **Not insecure** — does not introduce or worsen any vulnerability
5. **Doesn't undermine optimisation or parallelism** — existing parallelism
   (Rayon, parallel writes, mmap) must be preserved or improved; never traded
   away for readability
6. **Consistent methods** — follows the established patterns in the codebase
7. **Consistent style** — formatting, naming, structure match the rest
8. **All other factors** — helpers, readability, DRY, etc.

This applies globally. Every item in this file must be assessed against these
constraints before implementation begins.

---

## OPEN

### Guardian re-signing

**The queue is empty as of 2026-10-05T12:01Z**, across 38 rows, and all
seventeen signed source files verify GOOD. The last run
(`guardian_report_20261005_112412.md`) re-signed `src/bin/xtgeoip-docgen.rs`
after the licence row of 2026-10-04, which changed its validator and its
emitter and went a day without a queue row: the stale signature was found
while preparing v0.4.4, not by the queue. It raised nothing at MEDIUM or
above, one LOW in `contrib/README.md`, fixed with that release, and six
informational notes, which are below under *GUARDIAN FINDINGS —
`xtgeoip-docgen.rs`, 2026-10-05*. A change to a signed file gets its queue row
in the commit that makes it.

One wording correction is owed to `src/conf.rs`, deliberately **not** made:
`set_credentials`'s comment says that once the config is 0600 "only root can
read it at all". At 0600 it is the *owner* who keeps read access — root only
because the shipped config is root-owned; under `--config PATH` created by a
non-root user, that user is the one who can still read it. Nothing weakens, and
the auditor recorded it as no-action (CVSS 0.0). Correcting it means a third
re-sign round for a wording nit, so it waits for the next change that touches
`conf.rs` — but a future rewording must not build on "root" where "the owner"
is meant.

### Packaging and deployment

Early, but no longer unexamined: `docs/design/packaging.md` (2026-09-13)
measures the install set and settles the shape. Nine files and two
directories, 8.87 MiB once the shell completions landed on 2026-09-19; eight
recipes cover the top 20 distributions and are the same package eight times
over; deb and `PKGBUILD` first.

**The decision in it worth knowing without reading it**: `/etc/xtgeoip.conf`
must *not* be a packaged file. `conf --set-credentials` rewrites it in place
and leaves it 0600 with ciphertext in it, so as a dpkg conffile or an rpm
`%config` every upgrade would diff it against the shipped original and either
prompt or relocate live credentials. The example ships under
`/usr/share/xt_geoip/`; `conf --show` (and `--edit`, `--set-credentials`)
create the real one on demand via `ensure_system_config_exists`, while
`conf --default` only *prints* the example and creates nothing.

**§6 settled 2026-09-19.** Four decisions:

1. **No binary tarball**, and therefore no musl build — a program that cannot
   run without a distribution-supplied kernel module is served by recipes, not
   by a static binary. The probe was still worth running: it proved **cmake is
   not a build dependency** (`aws-lc-sys` takes its pregenerated-source `cc`
   path), which simplifies all eight recipes.
2. **Shell completions** via `clap_complete` in docgen, behind a contradiction
   test pinning `Cli::command()` to the spec's declared surface.
3. **Recipes in `contrib/`**, with the install set derived from one declaration
   rather than restated eight times.
4. **`extra/ export-ignore`**, keeping the vendored GPL-2 xtables-addons
   tarball out of the published MIT source artefact.

**Built so far**: decisions 2 and 3 in full. Decision 2's safety net first —
`cli::contradiction::clap_surface_matches_the_spec`, which pins clap's argument
surface to `flags` ∪ `global_options` ∪ `subcommand_options` in both
directions, each direction mutation-confirmed. It was worth building on its own
account: the globals were pinned only to the man page and `flags` only to the
guards, so nothing asserted the union, and an argument added to `cli.rs` and
forgotten in the spec was invisible to every existing check.

Then the generator itself: `generate_completions` in docgen emits bash, zsh and
fish into `docs/generated/completions/` (19,901 bytes), guarded by CI's
existing `docgen-check` and verified idempotent. One measured surprise —
`clap_complete` does **not** respect `hide`, so all three shells offer
`fetch`'s four rejected flags. Left alone: `hide` earns its place in the error
path, and an advisory artefact is not worth changing it for.

Then decision 3: `docs/spec/install.yaml` declares the nine files and two
directories once, docgen emits `docs/generated/install-manifest.tsv` from it,
and `contrib/README.md` documents how a recipe consumes it. Each entry carries
a `producer` and a `transform`, because two of the nine are not files on disk
in the form they ship — the man page is gzipped and the binary stripped at
package time. `tests::install_set_sources_exist` pins it, mutation-confirmed
on both claims.

**Written 2026-09-22**: `contrib/debian/` — `control`, `rules`, `changelog`,
`copyright`, `source/format` and a `README.source` for whoever builds it. Its
install step is a shell loop over `install-manifest.tsv` inside
`override_dh_auto_install`, so there is no `debian/install` file and nothing to
keep in step. Two new pins came with it, both mutation-confirmed:
`tests::install_manifest_transforms_are_known` (the recipe branches on the
`transform` column, so the column's vocabulary is now asserted) and
`tests::debian_changelog_version_matches_crate` (the one restatement a Debian
changelog forces).

Three things it settled that were open, and one it corrected:

- **Plain `dh`, not `dh-cargo`** — dh-cargo re-resolves against Debian's crate
  registry, which is what `--locked` exists to prevent, and would need all 307
  lockfile crates packaged in Debian at the pinned versions. §5's table said
  `dh` + `dh-cargo` and has been corrected.
- **The `transform` column is Debian's job, not the recipe's** — `dh_strip` and
  `dh_compress` are exactly `strip` and `gzip`, so `rules` performs neither and
  only rewrites the man page's `dest` to drop `.gz`.
- **No `-dbgsym`** — `Cargo.toml` sets no `[profile.release]`, so Cargo's
  release default leaves no debug info for `dh_strip` to extract. §3's
  do-not-pre-strip rule is still right and does not yet pay; `rules` suppresses
  the empty package and records how to get a real one.
- **The C compiler needs no `Build-Depends` line on Debian.** §3 predicted
  `aws-lc-sys` would be the interesting build dependency. It is not: gcc comes
  from `build-essential`, which Policy makes implicit and then forbids listing.

**Built end to end 2026-09-29**, twice: with the network, and offline from an
`orig-vendor` component tarball with `CARGO_HTTP_PROXY` pointed at a closed
port. Every manifest row lands with its declared mode, the binary is stripped
with no `-dbgsym`, the man page's gzip header carries no mtime (the `-9n`
claim, now checked from output), `/etc/logrotate.d/xtgeoip` is a conffile, and
`debian/rules clean` restores the pristine tree. Building found three faults
that inspection had not:

- `README.source` told the packager to `cp -r contrib/debian` out of the
  `v0.4.1` tarball, which has no recipe in it — the recipe postdates the tag.
- Its offline instructions vendored into the unpacked tree, which `dpkg-source`
  rejects as "unrepresentable changes to source". Now a component tarball.
- `dh_clean` deletes `*.orig` tree-wide, including the 306 `Cargo.toml.orig`
  files the vendored checksums list, so the offline path could never have
  worked. `rules` now runs `dh_clean -X./vendor/`.

**lintian run 2026-09-30** (2.117, `debian` and `ubuntu` profiles,
`-EvIL +pedantic`). The predicted tag is real: `extra-license-file`, at info
level. Two were fixed: `copyright-without-copyright-notice` (`LICENSE` and
`debian/copyright` now carry 2026), and `Standards-Version`, raised from 4.7.0
to 4.7.4 after the upgrading checklist. `Priority: optional` was dropped per
4.7.3 and then restored: dpkg < 1.22.12 (Ubuntu 24.04 has 1.22.6) supplies no
default, so the `.deb` lost the field entirely. The local lintian called 4.7.0
*newer* than current only because it knows Policy up to 4.6.2; sid's 2.141 has
not been run. Every remaining tag is expected and is
explained in `README.source`, including one verified false positive
(`override_dh_auto_test` under `nocheck`: `dh` omits the call itself at compat
13).

**Fixed 2026-09-30 — the unit suite was not hermetic under a proxy.** Found by
that build: with `http_proxy` or `ALL_PROXY` set and no `NO_PROXY`, nine
`fetch::tests` failed after 14 s each with `client error (Connect)`, because
reqwest honours the environment for the 127.0.0.1 stub too. `build_client` now
adds `no_proxy()` under `#[cfg(test)]` only, so the shipped binary still
honours an operator's proxy. With all six proxy variables pointed at a closed
port, `fetch::tests` went from 9 failed in 98.8 s to 55 passed in 0.95 s. It
touched the signed `src/fetch.rs`, re-audited and re-signed 2026-10-01. The
integration suite drives the release binary and is not covered by this.

**Still open:**

- **Three recipes unwritten**: apk, nix, SlackBuild.
  `contrib/README.md` lists the eleven findings the first five produced.
- **The ebuild is built and merged, in a container.** `packaging.yml` run
  37240800966 (2026-10-04); `contrib/README.md`'s status table says what it
  proved. The Manifest it generated is in `private/gh-runs/37240800966/`, and
  is not kept in the tree. Owed before it could go to an overlay:
  - the crates' licences in `LICENSE` (pycargoebuild);
  - a `metadata.xml`, which is all `pkgcheck scan` asked for;
  - Portage's QA notice asking for a crate tarball in place of 306 `CRATES`,
    which somebody would have to host (upstream ships none: packaging.md §4);
  - `RDEPEND`, which `ebuild` does not resolve.
- **Nothing builds on the declared compiler floor.** `Cargo.toml` declares
  `rust-version = "1.89"` since 2026-10-04, measured that day by hand (1.89.0
  builds and passes every test; 1.88.0 is refused), and
  `tests::recipes_state_the_declared_compiler_floor` ties `debian/control`,
  the rpm spec and the ebuild to it. What keeps the declaration true
  afterwards is indirect: clippy's `incompatible_msrv` for this crate's own
  code, and the resolver for dependencies, which prefers versions that build
  on 1.89 at `cargo update`. Neither is a build. A CI job running
  `cargo +1.89.0 check --locked` would be, at the cost of a second toolchain
  in CI. Also: the resolver can now hold a dependency back from a release
  that needs a newer compiler, a security fix included; raising
  `rust-version` to take it is a decision to make then.
- **`contrib/rpm/xtgeoip.spec` is built but not installed.** Fedora Copr
  build 11073816 (`hazensparkle/xtgeoip`, 2026-10-04) succeeded for
  `fedora-44-x86_64` and `opensuse-tumbleweed-x86_64`; `contrib/README.md`'s
  status table says what that proved. Still owed:
  - `--with vendor`, the offline path. Copr built with network access.
  - An install and an upgrade: `%config(noreplace)` on the logrotate file,
    and `Conflicts: xtables-geoip` against the real package on openSUSE.
  - `rpmlint`, and whether the three completion directories are owned by a
    package xtgeoip does not require.
  - openSUSE's LTO. Copr's Tumbleweed chroot exported no `-flto` in
    `CFLAGS`; the openSUSE Build Service's own configuration may, and slim
    LTO objects are what broke the Arch link. Not tested.
  - A decision the Tumbleweed package makes visible: openSUSE files docs
    under `/usr/share/doc/packages/`, and the package has them in
    `/usr/share/doc/xtgeoip`, as the manifest says.
  The logs are kept in `private/copr/11073816/`. Copr deletes a build 14 days
  after a newer one of the same package replaces it.
- **`LICENSE` is a doc file in every package built from v0.4.3**, at
  `/usr/share/doc/xtgeoip/LICENSE`: a second copy in the rpm, pacman and xbps
  packages, a lintian tag on Debian, and on Gentoo the only copy, where
  Gentoo wants none. Fixed in the tree on 2026-10-04 and effective from v0.4.4: the licence is a
  `license` row with no path (packaging.md §6.3), and every recipe has an arm
  for it. Seen on Debian on 2026-10-05, on a build of v0.4.4's tree: one
  licence text in the package, and lintian without `extra-license-file`. Owed
  for the other four, which all name v0.4.4 since the pin bump of 2026-10-06:
  a rebuild of each, to see one copy, and on Gentoo none. `packaging.yml` now
  fails on the wrong count for Arch and Gentoo; for rpm and xbps it has to be
  read from the package listing.
- **`contrib/void/` is built, installed and run.** Built on Void x86_64 glibc
  on 2026-10-02 by a delegated session, installed there the next day, and
  run; `contrib/README.md`'s status table has the detail, which is that
  session's report and was not seen from here. Still not exercised: the
  `make_dirs` trigger (not confirmed either way), `conf_files` handling on
  upgrade, and musl.
- **`contrib/arch/PKGBUILD` is built and installed, in a container.**
  `packaging.yml` run 37234035970 (2026-10-04) passed every step;
  `contrib/README.md`'s status table says what it proved. `!lto` is settled:
  run 37234046715 built without it and failed at the link. Still owed: an
  upgrade, for `backup=`. The logs and packages of all three runs are in
  `private/gh-runs/`.
- **After every release**, the PKGBUILD's `pkgver` and `sha256sums` and the Void
  template's `version` and `checksum` move to the new tarball in a follow-up
  commit, and the ebuild is renamed with its `CRATES` regenerated from the new
  `Cargo.lock`. Renamed, not copied: with two ebuilds in the directory,
  `recipes_state_the_declared_compiler_floor` checks whichever `read_dir`
  returns first (I-2 below). They cannot move before it: each file ships
  inside the tarball it pins. The rpm spec's `Version` and `%changelog` move *with* the release
  instead, and nothing checks them against `Cargo.toml` yet, as
  `debian_changelog_version_matches_crate` does for Debian.
- **sid's lintian 2.141 has not been run.** `Standards-Version: 4.7.4` rests on
  the upgrading checklist and a lintian that knows Policy only to 4.6.2.
- **README's usage block has no drift check.** Since 2026-10-02 it is the
  verbatim output of `xtgeoip --help` and `xtgeoip conf -h`, and nothing
  compares it with the program: the next change to the help text in
  `src/cli.rs` leaves it behind. A docgen test could diff the two.

Recipes build from a git tag, since `publish = false`. `v0.4.3` is the oldest
tag on GitHub: the first with a published release (source tarball plus
`SHA256SUMS` signed by the release key) and the first whose tarball leaves out
`.github/`, `.gitignore` and `.gitattributes`. `v0.4.4` is the first whose
manifest has the `license` row and whose `Cargo.toml` declares `rust-version`.
Annotations at `private/TAG_MSG_v0.4.2`, `private/TAG_MSG_v0.4.3` and
`private/TAG_MSG_v0.4.4`.
Four names are gone and must never be reused: a clone made before a deletion
still holds the old tag, and `git fetch` would not replace it. `v0.3.0`,
`v0.4.0` and `v0.4.1` were deleted from the remote and locally on 2026-10-02,
each unsuitable for a recipe: `v0.3.0` predated `.gitattributes`, `v0.4.0` the
emitter audit's H-1 fix, and `v0.4.1` every recipe and the RUSTSEC-2026-0285
`rustls` bump. `v0.4.2`, the first to carry a recipe, was deleted from GitHub
the same day at 20:19 UTC, on purpose (confirmed 2026-10-05, when the docs
still said to build from it), and from this clone on 2026-10-06. It was tag
object `d7eea26` on commit `a1ed1d0`, which is still in `main`'s history.

---

## GUARDIAN FINDINGS — `xtgeoip-docgen.rs`, 2026-10-05

From `private/guardian/guardian_report_20261005_112412.md`, the re-audit of the
licence row before v0.4.4. The file **passed** and was re-signed: 0 CRITICAL,
0 HIGH, 0 MEDIUM. The licence `source` is checked exactly as a file's is, it
is the only licence field the emitter writes, and no value of `source`,
`producer` or `summary` forges, splits or reshapes a manifest row (254,050
generated values a field, and 47 edited copies of `install.yaml` through the
real binary). All nine loops that read the manifest, the five recipes' among
them, gave the same call log for seven hostile licence rows as for the real
one, and each exits non-zero on an unknown kind.

L-1 is closed: the reference loop in `contrib/README.md` is the right-hand
side of a pipe and failed open without `set -e`, which it now has and
explains. What remains is informational. I-1, I-2 and I-6 are edits to the
signed file, so they wait for the next change that touches it; it will need
re-signing either way.

- **I-1** — `every_recipe_has_an_arm_for_every_kind` matches the substring
  `{kind})`, not an arm. Removing each of the 18 arms in turn, two survive:
  the ebuild's `dir)`, masked by `$(cargo_target_dir)`, and the README's
  `license)`, masked by prose. A commented-out arm also survives. The `*)`
  arm at build time is unaffected. *Match a line that starts with the arm,
  and skip comments.*
- **I-2** — `recipes_state_the_declared_compiler_floor` checks the first
  ebuild `read_dir` returns, and `contains` accepts a commented-out
  restatement beside a stale live line. *Check every ebuild, as the arm test
  does. Until then, never keep two ebuilds in the directory.*
- **I-3** — nothing reads the licence row's `source`: each recipe installs a
  literal `LICENSE`. Pointing the row at `README.md` changes the manifest and
  fails no test. This is the design (the row asks the question, the format
  answers it), but three recipe comments say the row "names this file" and
  nothing checks that. *No action.*
- **I-4** — four licence-specific mutations survive `cargo test`: dropping
  `no_control` on `producer` (equivalent, the allowlist subsumes it);
  dropping the licence `PRODUCERS` check (no case sets a bad one, and
  `producer` is never emitted); removing `deny_unknown_fields` from
  `InstallLicense` (untested, though the binary does refuse `dest:` there);
  and an emitter that writes `none` in the third field, which only
  `docgen-check` catches. *Add cases when the file is next open.*
- **I-5** — the emitter's second layer checks `\n`, `\r` and the tab count,
  not control characters in general; the first layer does that. U+2028 and
  U+2029 pass both, since `char::is_control` is category `Cc` only. dash and
  bash read such a row as one row of five fields. Not new with this change.
  *Re-check if a consumer ever splits lines the Unicode way.*
- **I-6** — the comment on `InstallLicense::producer` says nothing requires a
  licence to be `tracked`, and `install_set_sources_exist` requires exactly
  that; two `read_dir` loops use `filter_map(Result::ok)`, which skips an
  unreadable entry in silence; and `packaging.yml`'s two checking loops are
  not on the arm test's list of recipes.

---

## GUARDIAN FINDINGS — `fetch.rs`, `config.rs`, `conf.rs`, 2026-09-12

From `private/guardian/guardian_report_20260912_190213.md`, the re-verification
run that cleared the four queued rows. All three files **passed**: 0 CRITICAL,
0 HIGH, 0 MEDIUM, and all three were re-signed. The pre-flight sweep raised BAD
on all three under its own power — the queue's rows were never taken as
evidence, which is what the deliberately-retained `.sig` files are for.

`--ca-file` was the change that wanted the audit and it holds. Seventeen
adversarial bundles — unreadable, directory, `/dev/null`, empty, whitespace,
junk, key-only, truncated `BEGIN`, invalid base64, PEM-wrapped garbage DER,
good-cert-then-corrupt — all error; none falls back to the system roots.
Replacement rather than merge was shown live against a public site.

C-1, CF-1, I-1, I-2 and I-4 are closed and in `DONE.md`. What remains is
informational, and stays here so a later audit does not re-file it.

### Informational, from the same run

- **I-3** — L-2 traded unbounded memory for unbounded *time*: a FIFO planted at
  `archive_path` blocks `io::copy` where `fs::read` grew memory instead.
  Requires a local write to root-owned `archive_dir`. Not a regression, and the
  new behaviour is the better of the two. *No action.*
- **I-5** — a valid certificate surrounded by junk text is accepted (standard
  PEM skipping). A corrupt PEM *section* is still a hard error. *No action.*
- **I-6** — the userinfo check fails open when `Url::parse` fails. Benign while
  exactly one `url` crate is in the graph, since reqwest parses with the same
  one. *Re-check if a second `url` version ever enters.*
- **I-7** — `--config` lets whoever runs the binary choose every path it writes,
  but only under the precondition already recorded for `$EDITOR` and
  `--log-file`: a sudoers grant or unit file, neither of which we ship.
  *Re-score immediately if one is ever shipped.*

## GUARDIAN FINDINGS — `src/fetch.rs`, 2026-09-05

From `private/guardian/guardian_report_20260905_213041.md`. The file **passed**:
0 CRITICAL, 0 HIGH, 0 MEDIUM, and it was re-signed. Everything below is
advisory, and every item fails closed. Locations are given as function names
rather than line numbers, which drift.

L-1, L-2 and I-2 are closed and in `DONE.md`. The other three informational
findings stay here, because two of them exist to stop a future
reader "fixing" something deliberate.

### I-1 — uppercase hex digests are rejected *(INFORMATIONAL)*

The format gate accepts `A-F` because `is_ascii_hexdigit()` is case-insensitive,
but `format!("{:x}")` emits lowercase — so an uppercase digest passes the gate
and then fails the comparison. Fails closed; no live path affected.

Worth keeping for its methodology note: the auditor's first fixture used an
all-digit digest, for which upper and lower case are identical — a **false
PASS**. It was retracted as void and re-run with a digest containing real `a-f`
letters. The same shape as asking "does this SHA resolve?" when the question is
"is it reachable?".

### I-3 — `MAX_REDIRECTS` permits one fewer hop than its name suggests *(INFORMATIONAL, CVSS 0.0)*

`attempt.previous()` includes the *original* request URL, because `reqwest`
pushes it before calling the policy. With `>= MAX_REDIRECTS` and
`MAX_REDIRECTS = 3`, **two** redirects are followed, not three.

**Do not "fix" this.** The policy is stricter than its constant implies, one hop
is what the endpoint actually uses, and the same off-by-one is what makes the
https-downgrade check sound — the chain scanned by `.any()` includes the origin.
Recorded specifically to stop a future reader making it more permissive.

*No action. If anything, a comment on the constant.*

### I-4 — an overlong version token yields `ENAMETOOLONG` *(INFORMATIONAL, CVSS 0.0)*

`Version::parse` imposes no length limit, so a `Content-Disposition` filename
with a 253-character token produces a derived path whose basename exceeds
`NAME_MAX` (255). The path is still correctly **confined** to `archive_dir`; it
simply cannot be created, so `File::create` fails and the fetch aborts. No
traversal, no truncation into a neighbouring name.

*No action.* `part_path`'s own doc comment already records the reasoning.

---

## DEPENDENCY POLICY — a question, not a finding

`Cargo.lock` floated two crates under their caret ranges since the previous
audit, both on `fetch.rs`'s path:

- `reqwest` 0.13.2 → **0.13.4** — carries the TLS and redirect logic. Reviewed
  at source level; the delta **strengthens** the guarantee (`Authorization` is
  now also stripped on a scheme change with identical host and port).
- `zip` 8.1.0 → **8.6.0** — parses untrusted MaxMind input. No applicable
  advisory; RUSTSEC-2025-0168 is inapplicable twice over (far past the patch,
  and its functions are never called — extraction is hand-rolled).

Both moves were benign. **The question is whether that should be left to luck.**
The six credential-path crates (`argon2`, `chacha20poly1305`, `secrecy`,
`zeroize`, `toml_edit`, `serde-saphyr`) are exact-pinned; `fetch.rs`'s own
dependencies are caret-ranged, so the TLS stack can move between audits without
anyone deciding it should.

This is a policy question for the maintainer, consistent with the standing
position that updates happen on their terms. It is **not** a request to
reintroduce automated advisory tooling — see DECIDED.

Supply chain otherwise clean as of 2026-09-05: the crates.io August-2026
compromise name check found none of the nine names, no typosquats, and
`toml_edit`'s `0.25.13+spec-1.1.0` satisfies `=0.25.13` (build metadata is not a
version difference — it looks like a pin violation and is not one).

---

## DECIDED — do not re-propose

Carried forward from `DONE_tldr.md`. This list exists because each of these was
proposed, examined, and rejected for a recorded reason; re-proposing them costs
the same investigation twice.

- **`restore` primitive: REJECTED.** Backups are context-free; restores are not.
  Restoring means adopting responsibility for a problem you have not diagnosed.
  `docs/design/98-state-ownership-recovery.md` §0. General test: **if an
  operation is only correct given knowledge of *why* it is being performed, it
  does not belong in this tool**
- **Rollback and atomic swap (#24 stages 2–3): REJECTED.** Stage 3 was
  implemented once (`d2bce08`) and caused data loss
- **Cached-archive fallback on failed fetch: REJECTED.** Rebuilding the same
  version over an intact install is a guaranteed no-op with real risk. `build`
  already spells that request
- **Unattended cron: removed by design (#103).** Do not restore it by stashing
  the passphrase anywhere
- **Fuzzing/proptest for CLI semantics: dropped.** 136 total combinations;
  `cli::snapshot` already enumerates all of them exhaustively
- **Both toolchains are pinned**, and `sync.py` refuses to run unless the local
  ones match. Stable in `rust-toolchain.toml`; the rustfmt nightly, by date, in
  `rustfmt-toolchain` — CI and `sync.py` both read that file. Do not reintroduce
  `dtolnay/rust-toolchain@stable`, `@nightly`, `cargo +stable` or
  `cargo +nightly`: all four float and reopen the drift
- **No automated dependency-advisory tooling.** Removed entirely 2026-09-04 —
  the CI job, `.cargo/audit.toml` and the `sync.py` pre-flight; Dependabot
  rejected the same day. `cargo audit` remains a perfectly good command to run
  by hand; what was rejected is anything that runs it *for* you and blocks on
  the answer. Do not re-propose the job, the config, the pre-flight, a schedule,
  or Dependabot
- **Do not add `rustfmt` to the stable toolchain.** A stable rustfmt discards all
  five nightly-only options in `rustfmt.toml` — including `ignore` — and so
  rewrites `src/generated/`, failing docgen-check rather than the lint job.
  There is no stable escape: file-level `#![rustfmt::skip]` does not compile
  (E0658)
- **#104's live-host verification: RETIRED, premise moot** (2026-09-05). It
  existed to check that the top-level handler does not echo plaintext
  credentials from the config; since #103 the config cannot contain any

---

## RECORDED NON-ACTIONS

Deliberate omissions, kept so the next audit does not re-file them as new
findings.

- **Three INFORMATIONAL notes from `guardian_report_20260920_185130.md`**, left
  open at the 0.4.1 release because all three are text or test-quality and the
  audit chain had converged — that round found nothing above INFORMATIONAL
  after three consecutive rounds that each found something real. Worth doing,
  not worth another round before the tag:
  - `plain_relative`'s error message says "start at neither `/` nor a drive",
    but on Unix `Component::Prefix` is never produced: `C:foo` is a single
    `Normal` component and is accepted. Same overstatement class as IN-3,
    reintroduced by the closure written to fix M-2, two lines above it
  - the five `source` cases in
    `control_characters_cannot_forge_a_manifest_row` assert `is_err()` where
    every other assertion in that test pins the reason. Verified to fail for
    the right reason today, so rot-prone rather than wrong. Remedy is to carry
    the expected fragment in each tuple
  - `plain_relative` bounds shape, not character class. Spaces, globs,
    `$(...)`, backslashes and `~` still pass, and are inert only because the
    documented consumer quotes its expansions. That is a dependency on the
    recipe, not a property of the manifest. **Now stated**, as the fourth of
    `contrib/README.md`'s loop details (2026-10-02)
- **The tab assertion cannot detect decay of its own fixture.** A `\t`
  reflowed to a bare `t` by `format_strings` leaves seven tabs — still not
  four, still the same error message, still green. No test can close this;
  fixture intactness is established by reading the committed bytes, which is
  what the 18:51 audit did. Noted so nobody adds a test believing it covers
  this

- **A check comparing OPTIONS prose against the guard table.** The five
  man-page checks compare prose against the *planner* and the *config*; nothing
  compares the "is an error" claims in the OPTIONS `.RS` block against the
  guards, which is why the sixth defect (`run -b -p`) survived them. Harder
  than the other five — the claims are free prose, not a structured list — and
  worth doing only if a second one of these appears
- **The `zip` decode probe is not a permanent test.** It was driven through
  `extract_archive_to_temp` against the five real archives in
  `/var/lib/xt_geoip` and costs no MaxMind budget and no root, but making it
  permanent needs a decision about depending on machine state that is not in
  the repo
- **The pin protects only this repo.** rustup's *default* toolchain is the
  floating `stable` (1.98.1 as of 2026-09-12, one patch ahead of the pin), so
  outside `xtgeoip` a bare `cargo` is whatever `rustup update` last fetched.
  Changing a machine-wide default is the maintainer's call; recorded in
  `private/OUTSTANDING.md`
- **Commit signing is unconfigured**, and optional. SSH signing with
  `~/.ssh/id_ed25519` would work (it is unencrypted, so `sync.py` would not
  hang) and the `.pub` must be added to GitHub's *Signing keys* list separately
  from Authentication keys. **Do not backfill** — that needs another full
  history rewrite, which would undo the repaired commit references
