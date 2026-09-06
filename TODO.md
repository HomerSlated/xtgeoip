# TODO

Open work only. Everything closed up to 2026-09-05 is archived in
[`DONE.md`](DONE.md) (full history and reasoning) and
[`DONE_tldr.md`](DONE_tldr.md) (the summary that accompanied it). This file
replaces both going forward; there is no `TODO_tldr.md` any more.

Opened 2026-09-06, after a sweep confirmed all 41 tickets in the old file
carried a closed marker. What survived that sweep is below: three genuinely
open items, the advisory findings from the 2026-09-05 guardian audit, one
policy question, and the two lists that are normative rather than historical.

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

### #98 residual — the test setup/teardown lifecycle

The last genuinely open ticket from the old file. A known-good initial state,
and a teardown that survives a mid-run failure. Two of three halves are done
(documentation 2026-09-01; fail-fast preconditions 2026-09-02) and the
`restore`-based plan is **rejected** — see DECIDED.

**Analysed but not implemented**:
[`docs/design/98-test-isolation.md`](docs/design/98-test-isolation.md). Three
findings from that analysis:

- The suite **cannot run on a clean system at all**. `TL-006` (`xtgeoip -b`) is
  the sixth case and the first needing a populated `output_dir`; nothing before
  it builds. `--rebuild` cannot repair that either, because `build` is
  `fetch_mode: local` and needs a CSV archive that is equally absent. So it
  depends on production state it cannot itself create.
- Only **10 of 51** cases reach the WAN. 21 are rejected at argument validation,
  the whole `build` context is `fetch_mode: local`, and `top_level` has no fetch
  step. The network cost is a property of two contexts, not of the suite.
- Redirecting `[paths]` to a temp tree removes the root requirement at the
  *filesystem* level but not at the *program* level — see the root check below.

**Blocked on one decision (§4 of the note).** Reaching `[paths]` needs the
binary to read a different config, and `SYSTEM_CONFIG` is hardcoded. Routes:

| | Route | Status |
|---|---|---|
| (a) | `--config PATH` as a `global_options:` entry | **recommended** — an argument passes through `sudo` unchanged; costs one spec entry, does not widen the guard bitmask or regenerate the corpus |
| (b) | `XTGEOIP_CONFIG` environment variable | **dead unless (d) lands** — `sudo` runs with `env_reset`; inline `VAR=value` needs sudoers `setenv`, `sudo -E` needs the `SETENV` tag, neither default on Debian/Ubuntu |
| (c) | bind mount over `/etc/xtgeoip.conf` | blocked — `kernel.apparmor_restrict_unprivileged_userns = 1`, so it needs root, which is what we are removing |
| (d) | make the root check reflect what it guards | separate decision, below |

**Verification cost**: the first full run still costs one real `xtgeoip-tests`
pass against live, rate-capped MaxMind to prove the temp tree behaves as the
production tree did. Design on paper first.

### `Action::requires_root()` asks the wrong question

Split out of the above as §4(d), because it stands on its own merits and should
not be smuggled in as test infrastructure.

`requires_root()` is `!matches!(self, Action::Conf(_))` — a blanket euid test on
every command except `conf`, **independent of where the paths point**. It asks
"am I root?" where the question is "can I write to the configured `output_dir`
and `archive_dir`?". On a default install those coincide; on any other
configuration the tool refuses work it could do.

The right pattern is already in the tree: `conf.rs::check_system_config_writable`
probes writability by attempting `NamedTempFile::new_in(dir)` and treats root as
*advice* in the error text ("Re-run as root (e.g. with sudo)") rather than as a
gate.

`requires_root` has **no test coverage** — one call site, no tests — so it is
also the least-pinned thing in this file. It is a behaviour change to a
security-relevant check, so it wants its own decision.

### `src/config.rs` re-signing

One outstanding row in `private/guardian/needs_reverification.md`, dated
2026-09-05T22:30:00Z. The file was modified for the `deny_unknown_fields`
extension and the userinfo rejection; its `.sig` is **deliberately left in
place** so the next guardian run raises the BAD signature under its own power
rather than inheriting anyone's word for it.

Note `src/fetch.rs.sig` was BAD by design for the same reason until 2026-09-05
and is now GOOD. Do not confuse the two.

### Packaging and deployment

Early. Staging exists (`conf/etc`, `conf/usr`, `extra/dkms`, `extra/ufw`);
there is no `debian/` directory and no spec file. Verified 2026-09-06: neither
`debian/`, `rpm/`, nor any `*.spec` is present.

---

## GUARDIAN FINDINGS — `src/fetch.rs`, 2026-09-05

From `private/guardian/guardian_report_20260905_213041.md`. The file **passed**:
0 CRITICAL, 0 HIGH, 0 MEDIUM, and it was re-signed. Everything below is
advisory, and every item fails closed. Locations are given as function names
rather than line numbers, which drift.

**The cost calculus favours doing these together.** `src/config.rs` is already
queued for re-verification, so a guardian run is coming regardless; bundling any
`fetch.rs` change into that same run is close to free. Doing them piecemeal
costs one re-audit each.

### L-1 — the M-1 hardening was not applied to its sibling *(LOW, CVSS 3.1)*

`verify_cached_archive` reads the checksum sidecar with an unbounded
`fs::read_to_string` and no 64-hex-character gate. The *download* path now does
both (`MAX_CHECKSUM_BYTES`, and `.take(n + 1)` so an exactly-at-limit body stays
distinguishable from a breach).

It **cannot flip a decision**: `expected_hash` is only ever compared against a
digest computed locally over the archive bytes, so a hostile sidecar forces a
re-download and nothing else. An empty sidecar bails.

The argument for fixing it is not exposure, it is that **two paths now validate
the same value by different rules and will drift further apart**. Factoring the
bound-and-gate into one helper is roughly 20 lines.

*Recommended.*

### L-2 — `verify_cached_archive` loads the whole archive into memory *(LOW, CVSS 3.3)*

The same function `fs::read`s the entire archive rather than streaming it into
the digest. The archive is ~10 MB, so this is a resource note, not a
vulnerability. Fixing it trades an obvious correctness proof for memory that is
not scarce here.

*Not recommended, but recorded so it is a decision rather than an oversight.*

### I-1 — uppercase hex digests are rejected *(INFORMATIONAL)*

The format gate accepts `A-F` because `is_ascii_hexdigit()` is case-insensitive,
but `format!("{:x}")` emits lowercase — so an uppercase digest passes the gate
and then fails the comparison. Fails closed; no live path affected.

Worth keeping for its methodology note: the auditor's first fixture used an
all-digit digest, for which upper and lower case are identical — a **false
PASS**. It was retracted as void and re-run with a digest containing real `a-f`
letters. The same shape as asking "does this SHA resolve?" when the question is
"is it reachable?".

### I-2 — the raw remote checksum body is persisted verbatim *(INFORMATIONAL, CVSS 0.0)*

After verification succeeds, the *entire* response body is written to the
sidecar, not the validated 64-character token — so up to 4 KiB of
attacker-chosen UTF-8 lands in a root-owned file under `archive_dir`.

Inert: post-M-1 the body is capped at 4 KiB, its first token is proven to be 64
hex characters, and nothing ever reads past that first token
(`verify_cached_archive` takes `split_whitespace().next()`). Recorded only
because storing unvalidated remote text is not obvious from the call site.

*Optional hardening*: persist a canonical `format!("{expected_hash}  {name}\n")`.
Cheap, and it would compose well with the L-1 helper.

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
- **`Config::validate` echoes the configured URL** in its https rejection
  (`got {:?}`). A URL is not a credential and the message is much less useful
  without it. The one way it *could* have carried a secret — userinfo — is now
  rejected outright (2026-09-05)
- **The pin protects only this repo.** rustup's *default* toolchain is still
  `stable` at 1.94.0, so outside `xtgeoip` a bare `cargo` on this machine is the
  stale compiler that started the drift episode. Changing a machine-wide default
  is the maintainer's call; recorded in `private/OUTSTANDING.md`
- **Commit signing is unconfigured**, and optional. SSH signing with
  `~/.ssh/id_ed25519` would work (it is unencrypted, so `sync.py` would not
  hang) and the `.pub` must be added to GitHub's *Signing keys* list separately
  from Authentication keys. **Do not backfill** — that needs another full
  history rewrite, which would undo the repaired commit references
