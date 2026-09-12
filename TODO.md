# TODO

Open work only. Everything closed up to 2026-09-05 is archived in
[`DONE.md`](DONE.md) (full history and reasoning) and
[`DONE_tldr.md`](DONE_tldr.md) (the summary that accompanied it). Tickets that
close here move to the end of `DONE.md`, under *Closed after the archive*;
there is no `TODO_tldr.md` any more.

Opened 2026-09-06. What is here now: three open items, the informational
findings from two guardian audits, one policy question, and the three lists
that are normative rather than historical.

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

### `--config` is rejected before a subcommand *(cosmetic)*

Found during #98 (closed; see `DONE.md`). `xtgeoip --config X build` fails with
*"the subcommand 'build' cannot be used with '--config <PATH>'"*, because the
top level treats its own options as conflicting with a subcommand; `xtgeoip
build --config X` is accepted. The man page calls it a global option, so a user
will type the rejected form first. Cosmetic, unfixed, recorded here so it is
not rediscovered.

### Guardian re-signing

One outstanding row in `private/guardian/needs_reverification.md`:
`src/fetch.rs`, for a **comment-only** correction made after the
2026-09-12T20:43 run signed it (the note on what `from_pem_bundle` does and
does not validate). Its `.sig` is **deliberately left in place** so the next
guardian run raises the BAD signature under its own power rather than
inheriting anyone's word for it. Batch the re-sign with the next change that
touches `fetch.rs`.

`src/config.rs`, `src/conf.rs` and `src/secrets.rs` verify GOOD as of
2026-09-12. The `requires_root()` change touched no signed file.

### Packaging and deployment

Early. Staging exists (`conf/etc`, `conf/usr`, `extra/dkms`, `extra/ufw`);
there is no `debian/` directory and no spec file. Verified 2026-09-06: neither
`debian/`, `rpm/`, nor any `*.spec` is present.

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

C-1, CF-1 and I-4 are closed and in `DONE.md`. What remains is informational.

### Informational, from the same run

- **I-1** — `--ca-file` is silently ignored in `FetchMode::Local`: `fetch()`
  early-returns before `build_client`, so a mistyped bundle is not diagnosed on
  that path. No security impact (no TLS happens). *Worth a man-page sentence.*
- **I-2** — `fs::read(ca_file)` is the one unbounded read left in `fetch.rs`.
  Operator-supplied path, process already root, so not a trust-boundary read; a
  1 MiB cap would make provenance irrelevant. *Optional, consistent with M-1.*
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

L-1 and L-2 closed on 2026-09-12 and are in `DONE.md`. The four
informational findings stay here, because two of them exist to stop a future
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

### I-2 — the raw remote checksum body is persisted verbatim *(INFORMATIONAL, CVSS 0.0)*

After verification succeeds, the *entire* response body is written to the
sidecar, not the validated 64-character token — so up to 4 KiB of
attacker-chosen UTF-8 lands in a root-owned file under `archive_dir`.

Inert: post-M-1 the body is capped at 4 KiB, its first token is proven to be 64
hex characters, and nothing ever reads past that first token
(`verify_cached_archive` takes `split_whitespace().next()`). Recorded only
because storing unvalidated remote text is not obvious from the call site.

*Optional hardening*: persist a canonical `format!("{expected_hash}  {name}\n")`.
Cheap, and it would compose well with `expected_digest`, the L-1 helper.

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
