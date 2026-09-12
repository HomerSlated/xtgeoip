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

### #98 residual — the WAN traffic, and how a stub would reach the child ✅ CLOSED — sandbox, `--ca-file` and stub all shipped and verified on a real root run (2026-09-09)

**The temp-tree redirect is done (2026-09-08).** Each run of `xtgeoip-tests`
now builds a private sandbox (`create_sandbox`): a `0700` temp tree holding a
retargeted copy of the system config (`0600`), seeded by copying the
configured `output_dir` and `archive_dir` into it. `--config <sandbox>` is
appended to every case *and* to the `--rebuild` spawn, and the tree is removed
on the way out behind a guard (`sandbox_is_removable`) because teardown runs
`rm -rf` as root. Production data is read once to seed and never written.
`--keep-sandbox` retains the tree for inspection. Route (a) of §4.

Three things were settled by measurement rather than by reading:

- **`SSL_CERT_FILE` does steer the trust store.** reqwest resolves through
  `rustls-platform-verifier` → `rustls-native-certs`, with no `webpki-roots`
  anywhere in the tree. Pointed at an empty PEM it fails at *client
  construction* — "No CA certificates were loaded from the system" — not at
  request time. Previously read off the dependency's source; now observed.
- **Appending `--config` perturbs nothing.** All 51 cases were run against the
  release binary with and without it: every exit code identical. (`C-002`
  differs only non-root, where bare `conf -s` cannot read `0600` root
  `/etc/xtgeoip.conf`; under the runner's `sudo` both are 0.)
- **The suite's implicit fixture is now a checked precondition.** Seeding
  fails with a named remedy when the configured directories are empty, rather
  than surfacing at `TL-006` as "Nothing to back up".

**The stub is unblocked (2026-09-08).** Simulating the ten remote cases needs
the stub's CA to reach the *child* process, and `sudo` resets the environment,
so `SSL_CERT_FILE` cannot get there however well it works elsewhere. Route
(ii) was chosen: **`--ca-file PATH`**, a documented global option, since
arguments pass through `sudo` unchanged.

It replaces the system trust roots for the run (`tls_certs_only`) rather than
merging with them. Merging would let a wrong path or a stale bundle succeed
against a different anchor than the one named — the silent-success failure
mode #98 exists to remove. An unreadable, malformed or empty bundle is an
error, never a fall back. Verified end to end with `openssl s_server`: a
private CA verifies a local https server, and with only that CA trusted a real
public site is refused — which is what proves replacement rather than merge.

**The stub is done (2026-09-09), on route (B).** Each run starts
`openssl s_server -HTTP` inside the sandbox, serving the two endpoints `fetch`
uses; `--ca-file <sandbox>/stub/ca.pem` is appended to every case alongside
`--config`. **No new crates.** The route was chosen after measuring both:

| | Route | Measured cost |
|---|---|---|
| (A) | `rcgen` + `rustls` as direct dependencies | `rustls` 0.23.43 is already in the lock via reqwest, so it is a declaration, not a crate. `rcgen` adds four compiled crates — `rcgen`, `pem`, `yasna`, and a second `base64` (0.23.x, semver-incompatible with the 0.22.1 already there). It still needs the TLS listener written. |
| (B) | shell out to `openssl(1)` | zero crates, zero server code |
| (C) | a committed CA and key fixture | **rejected** — a private key in a public repository, in a repo that has already had to scrub leaked material |

The deciding argument was not the crate count, which is smaller than it first
looked. It is *who pays*: CI runs `cargo build --all-targets`, which compiles
`xtgeoip-tests`, and never runs it — so (A)'s dependencies would be built on
every CI run and every local build to support a binary that requires root, a
release build and a deliberate invocation. (B) puts its cost where the suite's
other preconditions already are, and nothing in the shared build.

How it works, all measured rather than read:

- `-HTTP` does no URL parsing. `GET /download?suffix=zip` maps onto the literal
  path `./download?suffix=zip`, and `?` is a legal POSIX filename byte, so the
  query string survives as part of the name. Two files serve two endpoints.
- In `-HTTP` mode, unlike `-WWW`, each file supplies its own status line and
  headers verbatim — which is what puts `Content-Disposition` under the
  runner's control with no server code.
- Verified with a `reqwest` 0.13.2 probe on our own feature set, not just
  `curl`: `s_server` answers in HTTP/1.0 with no `Connection` header and
  closes, reqwest opens a fresh connection per request, and `s_server` loops on
  accept.
- Nothing is fabricated. The stub replays the newest seeded
  `GeoLite2-Country-CSV_*.zip` byte for byte and computes the digest over
  exactly those bytes. Only the version in `Content-Disposition` is synthetic
  (`99999999-stub`) — `Version` is an opaque token ordered lexicographically,
  so no calendar arithmetic is involved, and a version absent from the seeded
  tree is what drives the first remote case down the full download-and-verify
  path rather than the cached-reuse short circuit.
- Only the happy path is needed. The ten remote cases are exactly the `key: p`
  `fetch`/`run` cases, pinned by `the_corpus_has_exactly_ten_remote_cases`;
  `build` plans `FetchMode::Local`, and the `key: f` ones fail argument
  validation before any I/O.

**The trap it is designed against.** `testcases.yaml` asserts only exit
statuses, so a stub that is never reached still passes every case — a missed
URL rewrite, an unthreaded `--ca-file`, or a cached-reuse short circuit would
all look green. `s_server` is run without `-quiet` so it logs one `FILE:`
marker per request served, and a run whose remote cases produced no markers
exits non-zero with a named remedy.

**A second trap, found by its own test.** The obvious readiness probe — TCP
connect to the port — is wrong, and `create_stub` returned `Ok` with a dead
server the first time it was tried against a taken port. The port is reserved
and released before `s_server` is spawned (it cannot report a port it chose),
so a stranger can hold it, and a connect-based probe then succeeds against the
stranger while the real server exits. Every remote case would fail later with
a TLS error pointing nowhere. The probe now waits on `s_server`'s own `ACCEPT`
marker, which it prints once when listening and never when the bind failed —
a signal specific to *this* child. Pinned by
`a_stub_that_cannot_bind_fails_instead_of_hanging`.

Related, and fixed alongside: `process::Child` has no killing `Drop`, so the
`Stub` is constructed *before* the readiness probe rather than after. Both `?`
paths in the probe previously leaked a live `s_server` holding the port.

**What the stub does not fix.** It removes the WAN traffic and the rate cap.
It does *not* remove the passphrase prompts: `secrets::decrypt` prompts once
per process, so the ten remote cases prompt ten times. That is #103's permanent
design, not a defect. The runner must not mint its own credentials to route
around it — `secrets::encrypt` is double-entry, which would make it twelve
prompts rather than none. `retarget_config` carries `[maxmind.credentials]`
across untouched and rewrites only `maxmind.url`.

**Verified, unlike the sandbox.** `stub_serves_what_fetch_expects` is an
`#[ignore]`d test that stands the real stub up, fetches both endpoints with
`reqwest` over TLS anchored on the freshly generated CA, and checks the
headers, the body bytes, the digest and the request log. It is ignored because
it spawns a process and binds a port, which `cargo test` is otherwise free of:
`cargo test --bin xtgeoip-tests -- --ignored stub_serves`.

**`--config` is rejected before a subcommand.** `xtgeoip --config X build`
fails with *"the subcommand 'build' cannot be used with '--config <PATH>'"*,
because the top level treats its own options as conflicting with a subcommand;
`xtgeoip build --config X` is accepted. The man page calls it a global option,
so a user will type the rejected form first. Cosmetic, unfixed, recorded here
so it is not rediscovered.

**Proven end to end (2026-09-09).** `sudo target/release/xtgeoip-tests
--rebuild` ran clean: **49 passed, 0 failed, 0 timed out, 2 skipped** (the
`conf -e`/`-c` prompts), seeding 509 output files and 17 archives, and
reporting **11 stub requests to 10 remote cases**. Every path that had only
unit coverage has now executed — `create_sandbox` reading root-only
`/etc/xtgeoip.conf`, `create_stub`, and `remove_sandbox`'s `sudo -n rm -rf`.

The request count is the load-bearing number, not the pass count. Eleven is
2 + 9x1: the first remote case downloads and verifies, the nine after it take
the cached-reuse path. A bypassed stub would have reported 0 and failed the
run; a version token colliding with a seeded archive would have reported 10.

Verified independently of the runner's own report: no `/tmp/xtgeoip-tests-*`
survived, no `s_server` was left running, and `archive_dir`, `output_dir` and
`/var/log/xtgeoip.log` all still carry their pre-run mtimes.

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

### Guardian re-signing

Three outstanding rows in `private/guardian/needs_reverification.md`, all dated
2026-09-12 and all created by remediating that day's own audit: `src/config.rs`
(C-1), `src/conf.rs` (CF-1) and `src/fetch.rs` (the I-4 follow-on). Their `.sig`
files are **deliberately left in place** so the next guardian run raises the BAD
signatures under its own power rather than inheriting anyone's word for it.

`src/secrets.rs` is signed and current. Of the three, only `src/fetch.rs`
changes behaviour — an error message on the `--ca-file` path; the other two are
the fixes the audit asked for. The 2026-09-05 row this section used to describe
was cleared by the 2026-09-12T19:02 run, along with eight others.

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

### C-1 — the https error echoed the URL, and ran before the userinfo gate *(LOW, CVSS 3.3)* ✅ DONE (2026-09-12)

`Config::validate` quoted `maxmind.url` with `{:?}` in the scheme `bail!`, and
that bail runs *before* the userinfo rejection. So the one branch a credential-
carrying `http://` URL takes was the branch that printed it — defeating the
gate added specifically because a URL is "stored in plaintext *and* quoted back
in error messages". Reaches stderr always; the log file only if `--log-file`
was passed, since a failed `load_config` has no `[logging]` yet.

**Fixed by not quoting.** The ordering swap is *not* sufficient, and this is
the part worth remembering: `url::Url::parse("user:pass@host/x")` takes `user`
as the **scheme** — RFC 3986 allows it — so `username()` is empty and that
shape slips the userinfo gate whatever the order; and a URL that fails to parse
at all takes I-6's fail-open branch to the same `bail!`. The error now reports
`scheme "http"` or `no scheme`, via a small `url_scheme` helper: a scheme
cannot contain `:` or `@`, so it cannot carry userinfo, which is what makes it
safe to echo when the URL is not. Six-shape regression test, confirmed failing
against the old message.

### CF-1 — `create_default_config` wrote through a symlink *(LOW, CVSS 1.8)* ✅ DONE (2026-09-12)

`fs::copy(DEFAULT_CONFIG, system_config_path())` follows a symlink at the
destination. With `--config P` naming a path in a directory a less-privileged
user can write, that user could pre-plant `P` as a link to a file the invoker
can write. Needs the invoker to name that path *and* confirm the prompt, so it
only bites a root operator who chose an untrusted path.

**Fixed** with `OpenOptions::create_new` (`O_CREAT|O_EXCL`), which refuses the
symlink rather than following it, and also collapses the caller's "does it
exist?" test and the create into one atomic step. The credential write path
never had this shape — `NamedTempFile` + `persist` replaces the link itself —
so this is the same two-paths-drifted story as L-1. The write also now sets
0644 explicitly: `fs::copy` carried the source's mode, and the installed
example is **0755**, so every config created this way had a pointless execute
bit. Three tests, asserting on the victim file rather than only on the error.

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
- **I-4** — `a_malformed_ca_bundle_is_rejected_not_ignored` actually exercises
  the *empty* path: its input has no PEM markers, so it is caught by the
  `is_empty()` bail, not by `from_pem_bundle`. The decode paths are pinned only
  by reqwest's internals in-repo (the audit verified them externally).
  **✅ DONE (2026-09-12).** Renamed to `a_ca_bundle_with_no_pem_sections_is_
  rejected`, and three decode-path tests added: valid base64 that is not a
  certificate, a truncated real certificate, and a good certificate followed by
  a corrupt one (which must fail whole — silent truncation to the good half
  would install a trust set the operator's file does not describe).

  **The truncation test found a defect, which is why it was worth doing.**
  `Certificate::from_pem_bundle` refuses a section that is not plausibly a
  certificate, but a *truncated* one survives it and is rejected later, by
  `build()` — and that call carried no context. The operator got `builder
  error: invalid peer certificate: BadEncoding`: no file named, no mention of
  `--ca-file`, for the one option whose entire purpose is to be explicit about
  which anchors are trusted. `build_client` now contexts that call. Confirmed
  load-bearing by removing the context and watching exactly one test fail.

  Note what this says about the audit: its 17-case harness proved every bad
  bundle *errors*, which was the security question, and it does. Whether the
  error is usable is a different question, and it took a test in the tree to
  ask it.
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

**The cost calculus favours doing these together.** `src/config.rs` is already
queued for re-verification, so a guardian run is coming regardless; bundling any
`fetch.rs` change into that same run is close to free. Doing them piecemeal
costs one re-audit each.

### L-1 — the M-1 hardening was not applied to its sibling *(LOW, CVSS 3.1)* ✅ DONE (2026-09-12)

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

**Fixed 2026-09-12.** The gate is now `expected_digest(&str) -> Result<&str>`,
called by both sites; the bound stayed site-local, since one reads a response
stream and the other a file. `verify_cached_archive` opens the sidecar and
`.take(MAX_CHECKSUM_BYTES + 1)`s it, with the same exactly-at-limit distinction
the remote read uses. Two regression tests, both confirmed failing against the
unfixed function: an oversized sidecar carrying a *valid* digest prefix (so only
the size can reject it), and a non-digest body. The digest bytes the suite
actually reads are unchanged, so no root run was needed — `cargo test --lib` is
the whole proof. 186 lib tests, clippy and fmt clean.

The two sites can still drift in one respect: the bound is duplicated in spirit,
not in code. That is deliberate — sharing it would mean a helper taking
`impl Read`, which buys nothing and hides which limit applies where.

### L-2 — `verify_cached_archive` loads the whole archive into memory *(LOW, CVSS 3.3)* ✅ DONE (2026-09-12)

The same function `fs::read`s the entire archive rather than streaming it into
the digest. The archive is ~10 MB, so this is a resource note, not a
vulnerability.

The original verdict was *not recommended*, on the grounds that fixing it
**"trades an obvious correctness proof for memory that is not scarce here."**
That premise was wrong, and it is worth recording why rather than just
reversing it. The implied alternative was a hand-rolled read loop, which would
indeed be less obvious than `Sha256::digest(&data)`. It is not the alternative:
`sha2`'s hasher implements `std::io::Write` (digest-0.10.7
`core_api/wrapper.rs:245`, under the `std` feature, on by default), so the
streaming form is `io::copy(&mut file, &mut hasher)` — four lines for two, no
loop and no buffer arithmetic. The same file already streams-and-hashes on the
download path via `HashingWriter`. There was no trade to make.

The benefit is small but real, and it is **not** about the 4.5 MB measured on
this box. It is that the read has no bound while every other read in this file
does (`MAX_DOWNLOAD_BYTES`, `MAX_EXTRACT_BYTES`, `MAX_CHECKSUM_BYTES`), and two
facts stop 4.5 MB being a ceiling: `maxmind.url` is operator-configurable —
Country is a default in `src/config.rs`, not a constraint — and this function
runs *before* anything validates the file, so whatever sits under the expected
name is read whole whether or not the program could use it.

**Fixed 2026-09-12.** Streaming was preferred to a new `MAX_ARCHIVE_BYTES`
deliberately: a cap needs a number guessed between "breaks a legitimate City
archive" and "is not really a bound", whereas `io::copy` makes the size
irrelevant rather than checked. Peak memory is the copy buffer. No behaviour
changes — the digest and the returned bool are identical — so no test can
discriminate between the two forms, and the added one does not pretend to. A
256 KiB non-repeating archive, many copy chunks long, must hash to what a
one-shot digest gives; it passes against the old `fs::read` form too, confirmed
by stashing the change and re-running. It is a forward guard on the streaming
path, not evidence for it.

Exposure was and remains nil: only root writes `archive_dir` (`drwxr-xr-x root
root`), so anyone who can plant an oversized file there already owns the host.
This was robustness, not security, and the LOW ranking was right.

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
