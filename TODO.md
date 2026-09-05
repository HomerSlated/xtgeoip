# TODO

## INVARIANTS

Any refactoring, optimisation, or cleanup must be evaluated in this order of precedence. A change that violates a higher-priority constraint must not be made, regardless of other benefits:

1. **No hard errors** — no segfaults, panics, or undefined behaviour
2. **No soft errors** — the function must still work correctly
3. **Not unsafe** — no potential memory leaks or unsound code
4. **Not insecure** — does not introduce or worsen any vulnerability
5. **Doesn't undermine optimisation or parallelism** — existing parallelism (Rayon, parallel writes, mmap) must be preserved or improved; never traded away for readability
6. **Consistent methods** — follows the established patterns in the codebase
7. **Consistent style** — formatting, naming, structure match the rest
8. **All other factors** — helpers, readability, DRY, etc.

This applies globally. Every item in this TODO must be assessed against these constraints before implementation begins.

---

## ✅ v0.3.0 — version bump ✅ DONE (2026-09-05)

76 commits since `c2bfcfd` set 0.2.0 on 2026-06-09. The bump is warranted by
the one **breaking** change among them: `62e554a` rejects `run -b -p`, which
previously ran. Note the symmetry — 0.2.0's own bump commit was also a
flag-ambiguity rejection (`-b -c -f`), so the precedent for what earns a minor
bump in 0.x here is "a combination that used to run now exits 1".

Also in it, in rough order of size: the spec-derived 4-stage planner and the
generation-side validator (#92); credential encryption (#103, XChaCha20-Poly1305
+ Argon2); the audit triage — F-001, F-002, F-003, F-006, F-007, M-1, O-003 —
and O-001/O-002, the two large optimisations (`build` 1.92×, `backup` 1.64×);
toolchain pinning for both channels plus staleness reporting; six dependency
advisories cleared; and the test suite from nothing to 216.

One line changed (`Cargo.toml`). Everything else is derived from it and was
regenerated rather than edited: `Cargo.lock`, the man page's `.TH` line via
`CARGO_PKG_VERSION` in `xtgeoip-docgen`, `xtgeoip --version` via clap, and the
MaxMind `User-Agent` (`xtgeoip/0.3.0`) in `fetch.rs`. No tag was created — this
repo has never used version tags.

---

## ✅ build: reverted atomic swap ✅ DONE (2026-06-13, `f2a68bd`)

`build.rs::atomic_swap` removed; write-in-place + `detect_orphans` reinstated.
`CountryCode` enum and incremental hasher retained (behaviour-neutral). Proven:
`sudo xtgeoip build` no longer touches foreign files in `output_dir`. See #24
for the constraint that must hold if an atomic swap is ever revisited.

---

## ✅ Spec-driven validator — COMPLETE (v0.2.0, 2026-06-09)

Design of record: `docs/design/spec-driven-validator.md` (approved 2026-06-08).
Gate 1 (`99e3362`): CLI rules declared in `cli.yaml`; docgen validates + cross-checks.
Gate 2 (`7d072ba`): `cli.rs` drives generated `cli_rules.rs` guard tables (u8 bitmask,
`first_guard` evaluator); snapshot green byte-for-byte across all 136 combos.
Proven live (`c2bfcfd`): `-b -c -f` → `force_ambiguous` added purely through `cli.yaml`.

Open follow-up resolved (2026-07-11): spec `conf` block changed from
`positional: {name: mode}` to `selector_flags: {choices: …}` with
`exactly_one_required:`, matching the flag-based implementation.

---

## OVERVIEW: Spec-Driven Architecture [#9, #26, #27, #34]

Currently there are three sources of truth for CLI semantics that will drift apart:

| Source | Role |
|---|---|
| Clap struct (`cli.rs`) | syntax — what flags exist |
| `normalize_cli_to_action()` | semantics — what flags mean (hand-written) |
| `cli.yaml` | intended semantics (the spec) |

`normalize_cli_to_action()` is effectively a hand-written semantics interpreter. It encodes allowed contexts, flag dependencies, conflicts, and ambiguity rules as Rust control flow. This logic duplicates (or anticipates) what the YAML spec expresses. It will drift.

Example — this rule:
```rust
if *prune && !*backup {
    return Err(anyhow!("--prune cannot be used without --backup"));
}
```
should be derived from:
```yaml
prune:
  requires: [backup]
```

The target architecture:
```
CLI → parsed args
    → semantic validator (data-driven from cli.yaml)
    → ActionPlan (generated/derived)
    → execution
```

The `Action` enum is explicit, type-safe, and easy to extend — keep this shape. The Action construction blocks (e.g. `Ok(Some(Action::Build { legacy, backup, ... }))`) are the right pattern; the change needed is that they should be generated from the semantics layer rather than hand-written. The individual items in this TODO are stepping stones toward this architecture; #22, #29 and #93 were the structural enablers and are all closed. What remains is the second half of the arc — spec-derived *planning* — which has never had a ticket of its own (see below).

Note [#32]: Preserve the `Action` construction pattern — the change is in the source of the construction logic, not its shape.

**⚑ #27 is not a ticket — it is an editing artifact (traced 2026-09-02).**
Of the four named above, #22 is closed (subsumed), #29 is closed and #93 is
done, which left **#27** looking like the last live enabler. It is not one.
It has never been an entry in any revision of this file. In the first
numbered `TODO.md` (`ebf7313`, 2026-04-19) it appeared exactly twice: in the
header above, and in this sentence —

> items #5, #17, #19, #20, #22, **#27/#31**, #28, #29 are the key structural
> enablers

`#31` was *"cli.rs: validation error strings are hand-written and
inconsistent — wire them to the spec's `reason_templates`."* At `2250465`
(2026-05-02, "Spec rewrite, wire error constants") that work landed: the
same commit deleted the `### #31` entry and rewrote `#27/#31` to `#27`. The
surviving half has read as an independent undefined ticket ever since.

**Nothing is blocked on a missing ticket.** The remaining work is described
in three places — the OVERVIEW body above, `docs/design/spec-driven-validator.md`,
and `docs/design/29-ambiguity-planner-vs-guards.md` §5–6 ("spec-derived
planning ... remains the #26/#27 endpoint"). What is missing is scope and
acceptance criteria, not knowledge. The next step is a *decision*, not an
investigation. Note also that #26 is in exactly the same state; #27 only
looked singular because of which sentence happened to get pruned.

**Finding (2026-09-02): the ordering has a rank, not a graph.** Enumerating
all 76 `Action` values and computing `plan()` for each shows every plan is a
subsequence of one fixed order:

    Backup → PruneBin → Fetch → Clean → PruneCsv → Build

Ten of the eleven step pairs co-occur in some command and all agree with it;
the one exception, `PruneBin`/`PruneCsv`, never co-occurs (bin pruning
belongs to top-level and `build`, CSV pruning to `fetch` and `run`). There is
exactly one data dependency in the system, `Fetch → Build`. So a spec would
need an integer rank per step plus membership rules — a topological sort over
one edge is machinery in search of a problem. This is the ordering analogue
of the validator's finding that all 17 guards were pure conjunctions.

**This is an observation about today's six steps, not an invariant.** Nothing
makes the total order hold. A future step that runs at different points in
different commands (an install or module-reload step) would break the rank
model and force the dependency graph the finding says is unnecessary.

Two further constraints on any implementation:

- **Generate Rust, not a data table.** `Plan::Pipeline` cannot be constructed
  without naming the fetch that feeds the build, so the Fetch-before-Build
  invariant is enforced by the type system. A flat `steps:` list can express
  `[Build]` with no fetch, which would trade that compile-time guarantee for a
  runtime check — a regression under the INVARIANTS precedence order. If
  docgen emits `plan()` as code constructing the existing `Plan` type, a spec
  that violates the invariant produces output that does not compile, exactly
  as a missing `error_text::` constant does today.
- **A rank integer has nowhere to put the *why*.** `action.rs:150-155` records
  that `Clean` follows `Fetch` because a failed download otherwise emptied
  `output_dir` with no replacement. `rank: 40` keeps the conclusion and
  discards the reasoning; the spec would need a `why:` field per step.

**Stages 1-3 landed 2026-09-02.** Design note:
`docs/design/26-spec-derived-planning.md`. `cli.yaml` gained a `plan:` section
(rank per step, membership per context, and a **mandatory `why:`** so the
reasoning behind each position survives instead of becoming a bare integer);
`xtgeoip-docgen` emits `src/generated/plan.rs`; and a differential test proves
`plan_generated()` reproduces `plan()` exactly over all 76 `Action` values,
parameters included — which the `steps:` examples deliberately do not cover.
Verified to have teeth: moving `clean`'s rank before `fetch`'s (the
pre-#24-stage-1 order) fails 32 of 76 with the precise diff.

The generator emits **Rust constructing `Plan`**, not a data table, so a spec
selecting `build` without `fetch` produces code that does not compile — the
Fetch-before-Build guarantee stays in the type system rather than degrading to
a runtime check.

✅ **Stage 4 — deleting `plan()` — DONE (2026-09-02, `98d5015`).** Sign-off
given. `action.rs` lost 98 lines of hand-written `plan()` and gained
`use crate::generated::plan::plan_generated as plan;`, so every call site and
every test kept working unchanged and the generated planner now drives
execution. The differential test that proved the two agreed over all 76
`Action` values went with it, necessarily: there is no longer a second planner
to differ from. `spec_steps_agree_with_plan` and the eleven goldens are what
pin the survivor.

Both follow-ups are also closed, one by doing and one by deciding:

- ✅ **canonical-order enumeration is now a permanent test** —
  `action::tests::manpage_execution_order_agrees_with_the_planner`
  (2026-09-03, `bd1c56c`), which parses the four `.TP` pairs out of the
  generated `.1` and compares them against the real planner. It pins exactly
  the property the man page's EXECUTION ORDER section promises users.
- ✅ **`outcome:` versus `plan()` (the #92 remainder) — resolved by
  decision, not by code.** `outcome:` stays authored prose and `steps:` is the
  machine-checkable half; the reasoning is recorded at the head of
  `spec_steps_agree_with_plan`. Asserting free prose against a step list would
  either constrain the prose to a generated sentence or compare it loosely
  enough to pass anything, and the defect it was proposed against (three
  `outcome:` strings claiming clean-before-fetch for six weeks) is caught by
  the `steps:` check that now exists.

**Dangling ticket references (audited 2026-09-01).** These numbers are
cited across this file as dependencies or enablers but have **no `###`
entry anywhere**: **#9, #12, #17, #18, #26, #27, #32, #34, #61**. (#27 is
traced above: it is the orphaned half of `#27/#31`, not a lost ticket.) They
carry real weight in prose — "#12/#18 configurability is the enabler"
(#88), "execution planner (#17) is the right place" (#24), "ties into #61"
(#76) — while carrying no scope. Listed here so the gap is visible rather
than implied. Deliberately *not* invented into entries: what each covers is
the user's to define, and guessing would be worse than the blank.

---

## CONFIG AND CONF SUBCOMMAND

### #1 — messages.rs / config.rs: file logging not optional ✅ DONE (2026-09-02)

**Root cause found:** terminal output was welded to file logging. `init_logger` built
the stdout/stderr *and* file dispatches together and was only called when `[logging]`
provided a log-file path — so with no `[logging]` section, no logger was installed at
all, and the `log` facade silently no-op'd *every* message (not just file output).

Fixed: `init_logger` now always installs stdout+stderr; the file sink is added only
when a path is configured. `main` calls `init_logger(cfg.logging…map(log_file))`
unconditionally (and `init_logger(None)` on the `conf` path). Resolves the "TBD":
when file logging is disabled, output still goes to stdout/stderr. Done with #94.

✅ **Residual DONE (2026-09-02).** Two global options, `--log-file <PATH>` and
`--no-log`, with `messages::resolve_log_file` holding the precedence: `--no-log`
beats `--log-file` beats `[logging]`. Both apply to every command, including
`conf`, which previously forced `init_logger(None)` and so would have ignored
an explicit flag.

A useful side effect: the override is known *before* the config is read, so
`--log-file` captures a **config-load failure** — which the configured path
structurally cannot, since that path is only known once the load has succeeded.
That is the same ordering constraint the #1 core fix was about, now with an
escape hatch.

**Spec placement — a deliberate choice.** These are declared in a new
`global_options:` block in `cli.yaml`, *not* in `flags:`. That map is the
universe the guard bitmask is built from (`cli_rules.rs`, 5 bits over
B/C/F/L/P), and every entry must be referenced by some guard — so a sixth bit
for an option with no combination semantics would fail
`every_flag_is_referenced_by_some_guard` immediately. Being outside that map
means being outside every check that reads it, so
`cli::contradiction::global_options_are_documented` covers the gap: it derives
the list from clap itself and asserts each appears in the generated man page.
Verified to have teeth by deleting `--no-log` from the template.

**A known wart, pinned by a test rather than hidden.** `args_conflicts_with_
subcommands` makes any top-level argument conflict with a subcommand, and a
clap `global` arg is not exempt — so `xtgeoip build --log-file X` works while
`xtgeoip --log-file X build` is rejected. The rejected form is the one a user
is more likely to type. Removing that setting would change the whole CLI's
semantics (it is what makes `xtgeoip -b build` an error), which is far too
large a blast radius for this; `global_options_follow_the_subcommand` pins
both directions and the man page documents the working position.

Verified live: `xtgeoip conf -d --log-file /tmp/xt-override.log` creates the
file, and the same command without the flag does not. Seven tests in total
(4 precedence, 3 clap/doc).

**`logging.verbose` deleted (2026-09-02, user's call):** logging is either on
or off; there is no verbose option. It shipped in `xtgeoip.conf.example` and
no code ever read it — `Logging` has no such field.

`deny_unknown_fields` was deliberately *not* added to `Logging` at the same
time. It is why the key went unnoticed (`[maxmind]` has it, `[logging]` and
`[paths]` do not), but adding it now would make every config copied from the
shipped example fail to load — the key is inert today and would become fatal.
That needs a migration story, and is recorded in HOUSEKEEPING rather than
done here.

---

### #103 — config.rs: MaxMind credentials stored in plaintext ✅ DONE (2026-07-20)

`maxmind.account_id`/`license_key` were plaintext TOML in `/etc/xtgeoip.conf`.
`#102` closed the *transport* leak (https-only); this closed the *storage*
leak. Design: `docs/design/103-encrypted-credentials.md` (all rationale —
KDF/AEAD choice, TOML shape, memory hygiene, UX — lives there, not here).

Implemented: `src/secrets.rs` (new — Argon2id derives a key for
XChaCha20-Poly1305; `rpassword`/`secrecy`/`zeroize`/`mlock` for memory
hygiene; `encrypt`/`decrypt` plus round-trip/tamper/wrong-passphrase unit
tests). `config::MaxMind.credentials: Option<Credentials>` replaces the
plaintext fields. `conf --set-credentials` (`-c`) prompts for
account_id/license_key/passphrase and splices ciphertext into
`/etc/xtgeoip.conf` via `toml_edit` + atomic `tempfile` write, preserving
the rest of the file untouched. `conf::splice_credentials` is a pure,
`pub(crate)` helper (source string + `Credentials` → new source string) so
the write→read seam has real test coverage:
`secrets::tests::splice_then_parse_then_decrypt_round_trips` writes
ciphertext via `toml_edit`, parses it back via plain `toml::from_str
::<Config>`, and decrypts it — proving the two parsers actually agree on
field names/types/table nesting, without touching the real
`/etc/xtgeoip.conf`. `fetch()` now takes already-decrypted
`account_id`/`license_key` as plain `&str` params instead of reading
`Config` directly — `action.rs`'s new `fetch_step` calls `secrets::decrypt`
before `fetch()` for `FetchMode::Remote` only. This split (not decrypting
inside `fetch.rs` itself, as §7 first said) was necessary to keep
`fetch.rs`'s existing mock-HTTP unit test suite running under plain
`cargo test` — those tests construct plaintext credentials directly and
have no terminal to prompt on; decrypting inside `fetch()` would have
broken all of them. `cli.yaml`/docgen/`xtgeoip-tests.rs` updated to match
(new case count 52; `-c` skipped by the integration harness, same as `-e`,
since both need a real terminal).

**Two bugs found by the user's own manual testing, both fixed 2026-07-20:**
1. **Migration didn't strip legacy plaintext.** Running `conf -c` against a
   pre-#103 config (still holding plaintext `account_id`/`license_key`
   under `[maxmind]`) added the new `[maxmind.credentials]` table but left
   the old plaintext fields sitting right next to it — the operator's real
   credentials stayed in cleartext through the whole exercise, defeating
   the feature. Fixed: `conf::splice_credentials` now removes those two
   keys before writing. Also added `#[serde(deny_unknown_fields)]` to
   `MaxMind` as a second layer, so any stray field there fails loudly at
   load time instead of parsing silently. Test:
   `secrets::tests::splice_removes_legacy_plaintext_fields`.
2. **`load_config()` failures were completely silent on the terminal** —
   pre-existing, not specific to #103, but the new `deny_unknown_fields`
   check gave an easy way to trigger it. Cause: `init_logger` (which
   installs the "always on" stdout/stderr dispatch) only ran *after* a
   successful config load, inside `init_runtime`; on a load failure,
   `log_early_error` wrote only to syslog and the `log` crate silently
   drops everything if no logger has been installed yet. Fixed in
   `main.rs`: `init_logger` now runs unconditionally right after
   `load_config()` is attempted (with `log_file: None` if it failed),
   *before* the failure is propagated — so every config-load error is now
   visible on the terminal. Confirmed fixed against the user's real config.

**Guardian audit complete (2026-07-20).** `src/secrets.rs` (first signature)
and `src/fetch.rs` (re-signature) both pass with no CRITICAL/HIGH finding —
report: `private/guardian/guardian_report_20260719_182033.md` (signed). No
RustSec advisories for any of the 7 new dependencies. One MEDIUM finding
outside both signed files, tracked as `#104` below. Two LOW/informational
notes accepted as-is, no action needed (both already within `docs/design/
103…md` §4's stated threat-model boundary — see the report's L-1/I-1/I-2 if
ever revisited): a theoretical stale-unzeroized-heap-copy in
`secrets::lock_and_wrap` reachable only by an attacker who can already read
live process memory (already out of scope), and `conf.rs`'s pre-encryption
locals being `zeroize`d but not `mlock`ed (narrower window than the decrypt
path, consistent with the design's own "best-effort" framing).

**Ultrareview pass (2026-07-20), two findings, both resolved:**
1. `src/secrets.rs`/`.sig` and `docs/design/103-encrypted-credentials.md` had
   never been `git add`ed — the review's diff scope only sees tracked
   changes, so from its vantage point `mod secrets;` pointed at nothing and
   it (correctly, given what it could see) reported the crate as
   non-compiling. Not a code defect — `cargo build` passed locally the whole
   time — but a real process gap: `git commit -a` only stages tracked
   modifications, so committing without an explicit `git add` on those three
   files would have shipped a broken build to anyone else. Fixed: staged.
2. `conf -c` prompted for account_id, license_key (the real one, into an
   unprivileged process), and ran the ~1s Argon2id KDF *before*
   `write_system_config_atomically` discovered `/etc` wasn't writable and
   failed with a bare EACCES. Nit, not a security hole (nothing reaches
   disk; the key is `zeroize`d), but bad UX for the first operator who
   forgets `sudo`. Fixed: new `conf::check_system_config_writable()` runs
   right after the terminal check, before any prompt — verified live over a
   pty as non-root: fails immediately with "Cannot write to /etc. Re-run as
   root (e.g. with sudo)."

**Documentation residual — found and fixed 2026-09-02.** The man page was
never updated for this ticket. `docs/spec/manpage-template.toml` still told
operators that `account_id` and `license_key` "must be configured" in
`/etc/xtgeoip.conf`, and its `conf` entry listed only `-d|-e|-s`, so
`--set-credentials` appeared nowhere in `docs/generated/` at all. The shipped
documentation was therefore instructing users into precisely the state
`#104`'s migration path treats as an exposed credential. Fixed: CONFIGURATION
now describes the `conf -c` workflow (encrypted under a passphrase, stored as
`[maxmind.credentials]` in the config file, plaintext removed on write, and
the passphrase re-prompted on every `fetch`/`run` — hence no unattended
operation); COMMANDS documents `-c`. The example config's comment was also
tightened: it said credentials are "not in this file", but the *ciphertext*
is written there — only the plaintext is not.

### #104 — main.rs: top-level error handler can echo raw config source (incl. credentials) to stderr/log ✅ DONE (2026-09-01)

Found auditing `#103`. `main()`'s catch-all prints `{e:#}` (anyhow's full
cause chain) on any unhandled error. `toml`'s parse-error `Display` embeds
the offending source line. Combine that with `#103`'s new `#[serde(deny_
unknown_fields)]` on `MaxMind`: a host upgraded to the `#103` binary but not
yet migrated (`conf --set-credentials` not run — legacy plaintext `account_
id`/`license_key` still under `[maxmind]`) fails to load config on **every**
non-`conf` command, and the raw source line leaks to stderr/log. Live-
reproduced: in the canonical field order (matching the pre-#103
`conf.example` — `account_id`, `license_key`, `url`) only `account_id`
(non-secret, per `#103`'s own §3 reasoning) is exposed; `license_key`
surfaces only in less-common orderings or an adjacent syntax typo landing
on its line. CVSS 6.2 (MEDIUM) — real, not blocking.

Two independently-correct changes combined to create this: `deny_unknown_
fields` (correct hardening) and today's other fix (installing `init_logger`
before `load_config`, so failures stop being silently dropped — see #103
item 2 above). Neither change alone would have caused it.

**Not attributable to `secrets.rs` or `fetch.rs`** — both were audited and
confirmed clean of this; the issue is `main.rs`'s formatting choice
interacting with `config.rs`'s hardening.

Guardian's advisory (non-prescriptive) options:
1. Use `{}` (top message only) instead of `{:#}` in `main()`'s catch-all for
   errors that may wrap a raw config-parse failure; reserve the full chain
   for a `RUST_LOG`-gated debug path.
2. Special-case a config-load failure to emit a fixed, field-name-only
   message instead of the raw parser error.
3. Proactively detect "legacy plaintext fields still present" (nothing does
   today) and nudge the operator to run `--set-credentials`, since that's
   the durable fix regardless of (1)/(2) — the leak only exists during the
   migration window.

Worth doing (2) and (3) together: (2) closes the leak mechanically; (3)
shortens the window it can ever fire in, and is a good UX nudge on its own
merits post-upgrade.

**Fixed 2026-09-01. Took (2) and (3), rejected (1).** Option (1) was the
wrong lever: `main`'s catch-all cannot know how sensitive any link in a
chain is, and downgrading it to `{}` would have stripped the `.context()`
chains fetch/build/backup rely on to turn a bare syscall failure into an
actionable message — a real usability regression in exchange for a narrower
fix. The rule adopted instead is **errors are sanitized where they are
made, not where they are printed**, documented at the `{e:#}` site in
`main.rs` so the next person to look at that line finds the reasoning.

Root cause, more precisely than the entry above had it: `toml::de::Error`
holds an `Arc<str>` of the **entire input file** and quotes the offending
line from its `Display`. The error object *is* a container for the config,
so no formatting choice downstream could have been sufficient. New
`config::parse_config` (split out of `load_config`, which can only ever
read the hardcoded path, so the invariant is unit-testable) rebuilds the
message from the two safe pieces — `err.message()` and `err.span()`, with
line/column computed locally by `line_col` — and the toml error never
enters the chain.

`message()` turned out **not** to be categorically safe, which the original
entry did not anticipate: serde builds type-mismatch text with `Debug`, so
`m_cost = "<value>"` yields `invalid type: string "<value>", expected u32`.
Every field we accept today holds something non-secret (paths, url,
ciphertext/KDF params), so nothing reachable leaks — but that made safety a
promise about fields not yet written. `redact_quoted_values` closes it
instead: values are double-quoted, names and expected literals are
backticked, so blanking double-quoted runs is precise. One carve-out, found
by observing real output: a `"` *inside* backticks is the parser's expected
literal (``expected `"` ``), not the start of a value.

Also added the (3) nudge: `legacy_plaintext_credentials` detects a
still-unmigrated `[maxmind]` and replaces the parser's symptom report with
`"/etc/xtgeoip.conf still holds MaxMind credentials in plaintext
(account_id and license_key). Run \`sudo xtgeoip conf --set-credentials\`
…"`, including advice to treat the old key as exposed and rotate it. It
returns empty when the file will not parse as TOML at all — a syntax error
is not evidence of a migration state, so those fall through to the
sanitized parse error.

11 new tests (169 total, up from 158). The one that matters asserts on the
**`{:#}`** rendering, not `{}`: `{}` printed only the outermost context and
was already safe before this fix, so a test against `{}` would have passed
on the bug. Verified by reverting `parse_config` to the old
`.context("Failed to parse TOML configuration")` form — 4 of the new tests
fail against it, including `errors_never_echo_config_source`; both new
`conf.rs` tests fail against the pre-fix `.context(...)` form too.

**Two corrections to this entry's original threat model:**
- "`license_key` surfaces only in less-common orderings" is wrong. Field
  order in the file is irrelevant: `toml` 1.0.7 deserializes through a
  sorted map, so among unknown fields `account_id` is *always* reported
  first — proved by reordering the file and getting the identical result.
  `license_key` surfaces when `account_id` is **absent**, i.e. a partially
  hand-migrated host. That is an incidental property of the crate's
  internal map, not a guarantee, and the fix correctly does not depend on
  it.
- "stderr/log" understates reach. On a config-load failure `init_logger`
  gets `None` (no log path is known yet), so there is no log *file* — but
  stderr is captured into the journal under systemd and into any redirect,
  so it is durable. The syslog line via `log_early_error` was already safe:
  it uses `{}`, not `{:#}`.

**A second instance, found by review rather than by the audit:** `conf.rs`
parses the same file with `toml_edit`, whose `TomlError` quotes the source
line exactly as `toml::de::Error` does. Two call sites — the
`has_existing` probe in `set_credentials` and `splice_credentials` — both
read `/etc/xtgeoip.conf`, in the one module that is only ever reached while
handling credentials. Live-reproduced: an unterminated quote *on* the
`license_key` line prints the key verbatim through `main`'s `{e:#}` (a
syntax error one line earlier does not — the span decides). Fixed by
routing both through a shared `conf::parse_document`, which reports via
`config::sanitize_toml_error`; that function now takes a loose
message + span rather than a `&toml::de::Error`, since the two crates'
error types are distinct but structurally identical. Shipping the
"sanitized where they are made" rule in `main.rs` while a sibling module
still made unsanitized errors would have guaranteed the next audit re-filed
this as new.

Confirmed the nudge is followable: `run()` matches `Action::Conf(_)` before
the arm that calls `load_config`, so `conf -d`/`-s`/`-c` all still work on
a host whose config now fails to load — verified by running `conf -d`
non-root against the real config. A nudge naming a command blocked by the
same error would be worse than the raw parser output.

Not fixed here, same class but non-secret: `Config::validate` echoes the
configured URL in its https rejection (`got {:?}`). A URL is not a
credential and the message is much less useful without it; noted so the
next audit does not re-file it as new.

Not verified end-to-end on a live host — AppArmor blocks unprivileged user
namespaces (`kernel.apparmor_restrict_unprivileged_userns = 1`) and the
config path is hardcoded, so shadowing `/etc/xtgeoip.conf` needs root. To
check by hand:
`sudo unshare -m sh -c 'mount --bind /path/to/legacy.conf /etc/xtgeoip.conf && xtgeoip fetch'`
(the mount is private to that namespace; config load fails before any
network call).

---

## MIGRATION

### #2 — Cargo.toml / docgen: migrate from `serde-yaml` to `serde-saphyr` ✅ DONE (2026-07-18)

`serde-yaml` is deprecated. Migrate to `serde-saphyr` (maintained successor, compatible API). Do alongside #77 and #79 to avoid touching the YAML serialisation path twice.

**Done.** API was drop-in — `from_str`/`to_string` under the same names — so all four call sites were one-word changes. Migrated in two stages, each with its own oracle:

1. **Readers** (`xtgeoip-docgen.rs:175`, `structure-errors.rs:26`, `xtgeoip-tests.rs:99`). Oracle: regenerate and `git diff --exit-code src/generated/ docs/generated/` — byte-identical, so the parser is equivalent on this spec.
2. **Emitter** (`xtgeoip-docgen.rs:867`, the only site producing committed output). Expected formatting churn in `docs/generated/testcases.yaml`; got **byte-identical output** instead, verified by `cmp` against the pre-swap file after forcing a rewrite. Byte-identity is strictly stronger than the semantic-equivalence check that was planned as a fallback.

**Pinned to `=0.0.29`** deliberately. Despite the README's "1.0 API" language the crate is published `0.0.x`, where Cargo treats *every* release as incompatible and the author guarantees nothing between versions. An exact pin makes upgrades a reviewed event rather than something that can silently shift emitter output and churn `testcases.yaml`. Revisit the pin if/when it reaches 0.1+.

**No YAML 1.1→1.2 scalar hazards**: `cli.yaml` has no bare `yes`/`no`/`on`/`off`, no unquoted nulls, no leading-zero numerics — the cases where saphyr (1.2) and serde_yaml (1.1-era) resolve types differently. The byte-diff would have caught a flip regardless.

Guard added: `xtgeoip-tests.rs` gained a `#[cfg(test)] mod tests` (3 tests) that parses the committed `testcases.yaml` and asserts case count (51), well-formedness, and `case_id` uniqueness. That reader was previously exercised *only* by the root + live-MaxMind run, so a deserialiser regression could not have surfaced without a rate-capped run.

Bearing on **#79** (verify `BTreeMap` ordering survives YAML deserialisation): the byte-identical regeneration is direct evidence that iteration order is preserved, and CI's `docgen-check` job re-proves it on every push. #79's explicit round-trip assertion is still unwritten; left open.

---

---

## ARCHITECTURE: ANALYSIS AND SMALL REFACTORS

### #93 — config.rs: split into config.rs (data/load) and conf.rs (command handler) ✅ DONE

Done 2026-06-07. `config.rs` is now the pure data/load leaf (`Config` + structs,
`validate()`, `load_config()`; the shared `SYSTEM_CONFIG` / `system_config_path()`
are `pub(crate)`). `conf.rs` holds the CLI-originated `ConfAction`, `run_conf()`,
preconditions, interactive `prompt_create_config()`, and the conf-only
`DEFAULT_CONFIG`; it depends on `config` for the path seam (never the reverse).
`cli.rs` and `action.rs` now import `ConfAction` from `conf`, not the data layer.
Behavior-preserving — the CLI-semantics snapshot stayed green byte-for-byte.

### #94 — backup.rs / fetch.rs: remove double-error reporting ✅ DONE

**Original premise was stale and the fix inverted it.** The double-print the entry
described only existed when `main` did `eprintln!("Error: {e}")`; that print was
removed in commit `410a482`, after which the `error()`+`bail!()` pairs were *not*
redundant — `error()` was the only thing reporting (it logs via the custom handler;
the propagated `bail!()` was dropped silently by `main`'s `process::exit`). Deleting
the `error()` calls verbatim would have made those errors silent.

Resolution: centralised reporting in `main` instead — `messages::error(&format!("{e:#}"))`
on the propagated error before exit — then removed the now-redundant inline `error()`
calls across `backup.rs` (verify_manifest_files, gather_files, backup, delete,
prune_archives) and `fetch.rs` (credentials). Kept `delete_all`'s per-file `error()`
calls (distinct detail, not duplicates). Every propagated error now reports exactly
once, via the custom handler (stderr + file, never stdout). Done together with #1.

---

## ARCHITECTURE: build.rs RESTRUCTURING

### #38 — build.rs: CSV parsing materialises all rows before grouping ❌ CLOSED — premise invalidated; small win taken (2026-07-18)

**The ticket's premise is stale.** It quotes `let parsed: Vec<(String, Option<(u32, u32)>)>` and calls it "high memory risk". The code has since changed to `Vec<(CountryCode, ...)>`, where `CountryCode` is a `Copy` enum over `[u8; 2]` — **no heap allocation at all**. The ticket was never updated.

Measured at real row counts (564,448 IPv4 / 558,545 IPv6 rows, from the cached `GeoLite2-Country-CSV_20260714.zip`):

| | element size | rows | transient |
|---|---|---|---|
| v4 `(CountryCode, Option<(u32,u32)>)` | 16 B | 564,448 | 9.0 MB |
| v6 `(CountryCode, Option<(u128,u128)>)` | 64 B | 558,545 | 35.7 MB |
| *(what #38 assumed)* `(String, Option<(u32,u32)>)` | 40 B | — | 22.6 MB **+ 564,448 heap allocations** |

v4 and v6 load in **sequence**, so peak transient is the larger — **35.7 MB**, not a sum and not a risk.

**The DashMap proposal is rejected on invariant #5.** Today parsing is parallel (`par_bridge`), grouping is a serial loop, merging is parallel (`par_iter_mut`). Streaming into a `DashMap` moves grouping into the parallel phase at the cost of **564k contended inserts on 2 cores** — plausibly a net loss, and invariant #5 forbids trading away existing parallelism for structure. With the memory rationale gone, there is nothing left to justify the risk or the new dependency.

**Small win taken instead.** `map` → `filter_map` so rows without a usable range are dropped at parse time rather than carried and skipped during grouping. This removes the `Option`:

- v6: 64 → 48 bytes/element (**~25%** off the larger path — `Option<(u128,u128)>` costs 16 bytes at 16-byte alignment)
- v4: 16 → 12 bytes/element
- one branch removed from each grouping loop

No new dependency, no parallelism traded, behaviour identical (the same rows were being discarded, just later).

`let parsed: Vec<(String, Option<(u32, u32)>)>` materialises millions of rows before grouping. Stream directly into grouping structure:
```rust
let pools: DashMap<String, Vec<(u32, u32)>> = DashMap::new();
rdr.into_records().par_bridge().for_each(|r| {
    if let Ok(rec) = r {
        if let Some((cc, range)) = parse_record(&rec) {
            pools.entry(cc).or_default().push(range);
        }
    }
});
```
Requires `dashmap` or `Mutex<HashMap>`. **Check against invariant #5.**

---

## ARCHITECTURE: fetch.rs RESTRUCTURING

### #57 — fetch.rs: `fetch()` mixes version resolution, acquisition, and extraction ✅ DONE (2026-07-16)

Landed as two commits: the fetch.rs test net (`6ae8735`), then the
behaviour-preserving decomposition. `fetch()` is now a recognisable orchestrator
calling `resolve_version(&resp)`, `check_download_size(&resp)`,
`acquire_remote_archive(resp)`, and `extract_and_validate(path)` — the last a
single home for the extract+validate step all three exit paths share (removed
the triplication). No HEAD request: the one `?suffix=zip` `Response` is threaded
through (headers read before the body is consumed, borrow-checker enforced).
Caveat: unit tests cover the helpers, not the HTTP orchestration — verify the
remote path end-to-end with `sudo target/release/xtgeoip-tests`.

Original description:

`fetch()` mixes version resolution, acquisition, and extraction. Split:
```
resolve_version()  → determine version (remote HEAD or local)
acquire_archive()  → download or confirm local archive valid
verify_archive()   → checksum + size + magic checks (#49, #50, #51)
extract_archive()  → unpack to temp, flatten, move into place (#54)
```
**Constraint: must not break any existing parallelism inside `fetch()`.**

**Scoping notes (2026-07-16):**
- Behaviour-preserving is achievable *without* the HEAD request the "remote
  HEAD" line implies: the single `?suffix=zip` GET already carries both the
  Content-Disposition (version) and the body (download). Thread that one
  `Response` through — `resolve_version(&resp)` borrows headers, then
  `acquire_archive(resp, …)` consumes the body — so no second request, no
  behaviour change.
- **Verify first.** `fetch.rs` has almost no test net (the M-1 tests below are
  the only unit tests). A behaviour-preserving refactor of security-critical
  download/verify/extract code can't be checked cheaply — add fetch.rs test
  coverage (mock HTTP, CSV/zip fixtures) *before* the split, or the refactor
  rests on inspection alone. Kept deliberately separate from the M-1 fix for
  this reason.
- **M-1 (guardian audit: unbounded extraction / zip bomb) — DONE (2026-07-16).**
  `extract_archive_to_temp_capped(path, max_bytes)` bounds cumulative extracted
  bytes (`MAX_EXTRACT_BYTES = 2 GiB`) via a per-entry `take(remaining + 1)`;
  covers both `FetchMode::Remote` and `::Local`. Two unit tests added. When #57
  lands, this logic moves into `extract_archive()` verbatim. See
  `private/guardian/guardian_remediation_M-1_20260716_100638.md`.

### #100 — fetch.rs: shared `.part` path allows concurrent-fetch interference ✅ DONE (2026-09-02)

Guardian finding **F-1**, audit `guardian_report_20260718_214129.md`. **LOW — CVSS 3.3** (`CVSS:3.1/AV:L/AC:H/PR:L/UI:N/S:U/C:N/I:N/A:L`). Pre-existing; *not* introduced by the `PartialDownload` guard.

`fetch.rs` derives the temp path as `archive_path.with_extension("zip.part")`, which is deterministic per version. Two concurrent `xtgeoip fetch` processes resolving the same version therefore share one temp file: interleaved `io::copy` writes could corrupt it, and one process's guard could remove a path the other is still writing.

**Fails closed.** Any corruption is caught by SHA-256 verification before the archive is trusted, so a bad archive is rejected rather than consumed. The guard does not worsen it — the shared path predates it, and `remove_file` tolerates `NotFound`. Worst case is a spurious failed fetch that a retry resolves.

Optional hardening: a unique suffix (PID, or `tempfile::NamedTempFile` in `archive_dir`), or an advisory lock on `archive_dir` for the duration of a fetch. Weigh against the fact that concurrent fetches are not an expected usage pattern for this tool.

⚠ Touching `fetch.rs` invalidates its guardian signature and needs a re-audit.

**Fixed 2026-09-02.** The path is now `part_path()`, which appends the PID:
`…_20260714.zip.<pid>.part`. A PID is exactly the right amount of uniqueness
for this finding — the failure requires two processes running *at once*, and
two live processes cannot share one. It is deliberately not a general-purpose
unique name: a PID repeats after the original exits, and separate PID
namespaces sharing an `archive_dir` could collide. Neither of those is the
concurrent-writer problem, and both remain caught by SHA-256 downstream.

`NamedTempFile` was considered and rejected. It gives unconditional
uniqueness, but only by replacing `PartialDownload` — 45 lines of documented
Drop-guard plus six tests — inside guardian-signed code, for a LOW finding
that already fails closed. Not worth the audit surface. (The usual argument
for it, that a crashed process leaves a stale file behind, does not separate
the two: on SIGKILL neither `Drop` runs.)

Two tests, both verified to fail against the pre-fix code:
`part_path_is_not_shared_between_processes` (asserts the path is *not*
`archive_path.with_extension("zip.part")` — the exact old derivation — and
that the rename stays same-filesystem) and
`part_path_is_neither_discoverable_nor_prunable` (a `.part` name must stay
invisible to both archive discovery and pruning, the combination that made
the pre-#99 leaks immortal).

Signature: `src/fetch.rs.sig` is now stale and was **left in place**, with a
row added to `private/guardian/needs_reverification.md`, so the next guardian
pre-flight raises the BAD signature itself rather than taking my word for it.

### #101 — fetch.rs: no explicit HTTP redirect policy ✅ DONE (2026-07-18)

Guardian finding **F-2**, same audit. **LOW — CVSS 3.7** (`CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:L/I:N/A:N`).

`Client::builder()` sets a User-Agent and a 300 s timeout but no `.redirect(Policy::…)`, so it inherits reqwest's default of following up to 10 hops.

**The credential risk is already mitigated — by the library, not by us.** reqwest strips the `Authorization` header on cross-origin redirects, so the license key is not forwarded to a redirect target. The residual is precisely that this is an *inherited* guarantee: a credentialed request whose safety depends on a library default that could change on upgrade, with nothing in this codebase asserting it. Secondary residual is an unpinned redirect destination — a compromised or misconfigured MaxMind endpoint could redirect the download to an arbitrary host.

Substantially defused downstream: the archive is trusted only after SHA-256 verification against the separately-fetched `.sha256`, so an attacker would have to redirect *both* requests consistently.

**Live measurement (2026-07-18) — settles the policy choice and corrects the remediation above.** Observed against the real endpoint with credentials, using a `.sha256` request and a 1-byte range request to keep the cost minimal:

1. `GET download.maxmind.com/geoip/databases/GeoLite2-Country-CSV/download?suffix=…` with basic auth → **302**
2. Redirect target is a **Cloudflare R2 bucket host** (`*.r2.cloudflarestorage.com`, different origin), URL carrying a query string
3. That target is **pre-signed**: fetched with *no credentials at all* it returned **206**

Consequences:

- **`Policy::none()` would break the tool outright** — every fetch depends on following that 302.
- **Host-pinning would break it too.** The target is a different origin, and its hostname embeds a bucket identifier that is not ours to depend on.
- **The security argument is concrete, not theoretical.** The license key goes to `download.maxmind.com`; reqwest strips it on the cross-origin hop; and the R2 request needs no credentials — proven. So the stripping prevents the key being sent, *on every fetch*, to a third party that demonstrably does not need it. A reqwest default change would leak it silently and continuously.
- **The remediation as originally filed was mis-specified.** A redirect policy *cannot* assert the stripping guarantee: reqwest's `Policy` only decides follow-or-stop and cannot inspect or modify headers. Only a **test** can assert non-forwarding.

**Revised remediation.** A custom policy that asserts what a policy actually can:
- bound the hop count (observed: 1, so 3 gives headroom without brittleness), and
- **reject any redirect target whose scheme is not `https`** — expressible in a policy, and it closes the downgrade case noted as the secondary residual.

The credential-stripping property is asserted separately, by a test under **#88**: serve a cross-origin redirect from the mock and assert no `Authorization` header reaches the second origin. That is the pairing between these two tickets — #101's *fix* is one line; its *verification* needs #88's harness.

✅ **DONE (2026-07-18).** `redirect_policy()` added and wired into the client builder.

The rule implemented is **no scheme downgrade** (https → http refused), not "targets must be https". That is deliberate and is the better rule on two counts: the property that matters is that a secure request is never silently downgraded, and stating it that way keeps the behaviour testable over plain HTTP — a blanket https requirement would have rejected its own test's redirect. Hop count is bounded by `MAX_REDIRECTS = 3` against 1 observed.

Verified by two tests: `redirect_loop_is_bounded` (a self-referential redirect terminates instead of looping) and — the one a policy *cannot* express — `credentials_are_not_forwarded_across_origin_redirect`, which serves a 302 from one local origin to another and asserts the second sees no `Authorization` header while the first does. That converts reqwest's cross-origin stripping from an inherited library default into a property this codebase proves, which matters because the R2 hop makes it load-bearing on **every** fetch.

Still not asserted: the downgrade rule itself, which would need a TLS origin to exercise. Recorded as a known gap rather than claimed.

⚠ Touching `fetch.rs` invalidates its guardian signature and needs a re-audit.

### #102 — config: `maxmind.url` scheme is unconstrained ✅ DONE (2026-07-19)

Guardian finding **F-3**, audit `guardian_report_20260719_000315.md`. **LOW — CVSS 2.4**. Pre-existing; not introduced by the #101 delta.

Nothing requires `maxmind.url` to be `https`. An `http://` value in `/etc/xtgeoip.conf` would send the license key in cleartext — and `redirect_policy`'s downgrade check cannot fire, because with an `http` origin there is no `https` predecessor to downgrade *from*.

Bounded: setting it requires root write access to `/etc/xtgeoip.conf`, and root already wins. The realistic risk is **operator misconfiguration**, not attack.

**Remediation — note where it goes.** `.https_only(true)` on the client would close it but break all eight mock tests in `fetch.rs`, which drive `http://127.0.0.1`. The guardian's suggestion is better: **validate the scheme in `config.rs` at load time** and leave `fetch()` scheme-agnostic. That also keeps the change out of guardian-signed `fetch.rs`, so it costs no re-audit.

**Resolved: reject any non-https, no exception.** Loopback is deliberately not special-cased — a local http mirror must be fronted with https rather than carved out. The shipped `xtgeoip.conf.example` already uses https, so nothing documented breaks.

Implemented in `Config::validate()`, which `load_config()` calls on every real load. Scheme comparison is case-insensitive, since RFC 3986 defines schemes that way and rejecting `HTTPS://` would be wrong. The empty-URL check still fires first with its own message.

7 tests: https accepted, uppercase scheme accepted, http rejected *with the reason in the message*, http loopback rejected, other schemes (`ftp:`, `file:`, bare string) rejected, surrounding whitespace neither smuggles a bad scheme through nor fails a good one, and an empty URL still reports as empty rather than as a scheme error.

This and `fetch::redirect_policy` are complementary halves of one property: the policy refuses an https→http *downgrade*, but cannot help when the origin is already http, because there is no https predecessor to downgrade from.

### #54 — fetch.rs: ZIP entry writes are sequential ❌ CLOSED — WONTFIX, measured (2026-07-18)

ZIP entry enumeration is sequential but file writes after decompression are independent. Decompress to buffer sequentially, then spawn parallel write tasks via Rayon. Not critical now; worthwhile if archive grows. **Benchmark before committing.**

**Benchmarked as instructed; the proposal does not pay for itself.** Measured against the real cached archive (`/var/lib/xt_geoip/GeoLite2-Country-CSV_20260714.zip`, 45.58 MB uncompressed over 12 entries), same `zip` crate, release build, mean of 5 runs. `fetch.rs` was **not modified**, so its guardian signature is untouched.

| Phase | Time | Share |
|---|---|---|
| A — serial extract (today) | 124.24 ms | 100% |
| B — decompress only | 71.97 ms | 57.9% |
| C — write only, serial | 45.45 ms | 36.6% |
| D — write only, Rayon (**the #54 proposal**) | 43.88 ms | 35.3% |

**#54 saves 1.57 ms of 124 ms — 1.3% of extraction.**

Three independent reasons it cannot be worth it:

1. **It parallelises the cheap half.** The proposal explicitly keeps decompression serial and parallelises only writes. Decompression is 57.9% of the work; writes are 36.6%. The expensive part is left untouched by construction.
2. **The entry profile caps entry-level parallelism at 1.89×.** Two entries — `Blocks-IPv6` (52.8%) and `Blocks-IPv4` (47.0%) — are **99.8%** of all bytes; the other 10 entries total 0.2%. Effective parallel width is 2, not 12, and perfect parallelism still waits on the single largest entry. Even with infinite cores and perfectly parallel writes, the ceiling for the #54 proposal is ~17% of extraction, not the measured 1.3%.
3. **Extraction is not the bottleneck of the operation it lives in.** `fetch()` downloads a ~4.7 MB archive over the network first. Extraction is ~124 ms against a multi-second download; shaving 1.6 ms off it is unmeasurable in practice.

Caveat recorded honestly: the benchmark machine has **2 cores**, so the measured 1.3% is a lower bound and a many-core host would land somewhere under the ~17% ceiling. That does not change the conclusion — reason 3 is independent of core count, and reason 2 caps the ceiling regardless.

If extraction ever *does* need to be faster (much larger archive), the measurement says to parallelise **decompression** across the two Blocks files — ceiling 1.89× on the whole extraction — not the writes. Recorded here so the next person does not re-derive it. Same reasoning applies to **#71** (sequential manifest verification): measure the split before parallelising, and check what fraction of the enclosing operation it represents.

Benchmark harness is not committed (it was a scratch project depending only on `zip`/`rayon`/`tempfile`); the numbers above are the deliverable.

### #71 — backup.rs: manifest verification is sequential ❌ CLOSED — WONTFIX, measured (2026-07-18)

Consider Rayon `.par_lines()` or `.par_iter()`. On small datasets, overhead may exceed benefit. On NVMe with many files, likely a win. **Measure first.**

**Measured as instructed. The ticket's own hypothesis is confirmed — and it still isn't worth doing.** Benchmarked against the real data directory (`/usr/share/xt_geoip`, 509 files / 10.95 MB, manifest `GeoLite2-Country-bin_20260714.blake3`), release build, mean of 5 runs, 2 cores. `backup.rs` was **not modified**.

| Phase | Cold cache | Warm cache |
|---|---|---|
| A — verify, serial (today) | 39.05 ms | 14.23 ms |
| B — verify, Rayon (**the proposal**) | 8.46 ms | 8.07 ms |
| speedup | **4.61×** | 1.76× |
| C — tar + gzip | 888 ms | 950 ms |

Unlike #54, the parallelism genuinely works: 506 similarly-sized files give an Amdahl ceiling of 6.9×, and it reaches 4.61× on two cores — superlinear against core count because the work is I/O-latency-bound and the syscalls overlap. "Likely a win on many files" was correct.

**It is nonetheless immaterial: verification is 1.5–4% of a backup; tar+gzip is 96–98.5%.** #71 saves **0.6–3.3%** of the operation.

Two reasons to close rather than take the free 3%:

1. **Scale.** The saving is invisible next to the ~950 ms compression step it sits beside.
2. **It would cost error determinism** — a cost the ticket does not mention. `verify_manifest_files` `bail!`s on the *first* mismatch and names that file. Under `par_iter` the winning failure becomes nondeterministic: the same corrupted directory could report a different filename on each run. This is an **integrity check**; reproducible diagnostics matter more here than 30 ms. Doing it properly would mean collecting all failures and choosing deterministically (e.g. lowest manifest index), which is more code and more risk than the gain justifies.

**Redirect — the real win is compression, filed as #99.** See below; level 1 would cut the whole backup ~84%.

Method note, same as #54: measure the split, *then* check what share of the enclosing operation it represents. Both parallelism tickets failed on the second question, not the first.

---

## ARCHITECTURE: action.rs / EXECUTION PLANNER

### #22 — action.rs: FetchMode semantics exist only in code ❌ CLOSED — SUBSUMED by #26/#27 (2026-07-18)

`FetchMode::Remote` and `FetchMode::Local` are a clean abstraction but their semantics exist only in code. Bring into spec:
```yaml
fetch:
  mode: remote | local
```
Depends on #17 and spec-driven direction.

**Closed as subsumed**, on the reasoning that doing it piecemeal builds machinery #26/#27 will replace. Spec-derived planning brings `FetchMode` in along with the whole step sequence, from one declarative origin; a standalone `fetch_mode` key would create a *second* partial spec→plan path while the step sequence stayed in code — two mechanisms for one concern, which is the problem the spec-driven arc exists to remove.

Dependency status, for whoever picks up #26/#27:

- **`#17` (execution planner) is satisfied.** It has no ticket of its own — it is a dangling ID referenced by #22, #29 and #24 — but the planner exists: `enum Step` + `enum Plan` + `fn plan()` in `action.rs`, restructured 2026-07-18 (#29) so that `Plan::Pipeline` encodes Fetch-before-Build structurally. Anything blocked "on #17" is now unblocked.
- **`#26`/`#27` (spec-derived planning) is not started.** That is the real blocker here.

**Carry forward — a documentation gap that is independent of the codegen question.** `FetchMode` is not merely internal: it determines **whether a command contacts the rate-capped MaxMind API**. `run` and `fetch` are Remote; `build` reuses the cached archive (Local). That is arguably the most operationally significant fact about these commands — it is why `build` can be used to exercise the pipeline for free (see #29's live verification), and why full `xtgeoip-tests` runs are expensive. It currently appears in **neither** `cli.yaml`, the man page, nor `--help`. Whether or not the mode is ever code-generated, it should be *documented*. Worth folding into #26/#27, or raising separately if that stalls.

*(Note: `#61`, referenced by #76, is likewise dangling — no such ticket exists.)*

### #29 — cli.rs: ambiguity checks have no formal basis ✅ CLOSED (ratified 2026-07-16)

Ad hoc ambiguity checks (`if *prune && *force && *clean`, etc.) have no formal basis. "Ambiguous" is undefined. A combination is ambiguous if and only if the execution planner (#17) cannot produce a deterministic `Vec<Step>`. Remove current checks once planner exists; let inability to plan be the rejection signal.

**Reframe (2026-07-16) — this is now a DESIGN FORK, not a coding task:**
- The planner already exists: `enum Step` + `fn plan(action: &Action) -> Vec<Step>`
  in `action.rs`. #29's "once planner exists" precondition is already met.
- But `plan()` is currently *total* — it always returns steps, because invalid
  flag combos are rejected *earlier*, by the declarative guards the v0.2.0
  spec-driven validator shipped (`cli.yaml` → `cli_rules.rs`). So "ambiguous"
  now HAS a formal basis — just a different one than #29 imagined (declarative
  guards, not planner-inability).
- Decide before writing code:
  - (a) Treat the shipped guards as the formal basis #29 asked for and largely
    **close #29** — the complaint ("no formal basis") is answered.
  - (b) Push validity *down* into the planner: make `plan()` partial
    (`Result<Vec<Step>>`), move ambiguity detection there, retire the guard
    layer. Bigger; must keep the 136-combo `cli::snapshot` green byte-for-byte,
    and reconciles with #22 (FetchMode into spec).
- First deliverable is a short design note (in the vein of
  `docs/design/spec-driven-validator.md`) resolving (a) vs (b) — not an
  implementation. Research before production.

**CLOSED (a), ratified by user 2026-07-16.** Design note:
`docs/design/29-ambiguity-planner-vs-guards.md`. Rationale: the declarative
guards ARE the formal basis #29 asked for; (b) would move validity *backward*
(declarative spec → imperative `plan()`) and isn't the north star either
(#26/#27 is spec-*derived* planning, declarative all the way).

Redirected residual:
- ✅ **DONE (2026-07-16)** — unit-pin `plan()`'s `Vec<Step>` per `Action`.
  11 golden tests in `action.rs` assert each plan's `Debug` form (sequence +
  fields), pinning e.g. run→`Fetch{Remote}`+`PruneCsv` vs
  build→`Fetch{Local}`+`PruneBin`, and `build_is_always_preceded_by_fetch`
  sweeps every flag combination to pin the invariant behind
  `execute_step`'s `.expect("Build step requires prior Fetch")`.
- ✅ **DONE (2026-07-18)** — Fetch-before-Build is a construction guarantee.
  `Step` lost its `Build` variant; `plan()` now returns
  `Plan::Simple(Vec<Step>)` or `Plan::Pipeline { pre, fetch, mid, legacy }`,
  so a build is not expressible without naming its fetch. `RunContext`, its
  `Option<(TempDir, Version)>`, and the `.expect("Build step requires prior
  Fetch")` are all gone; `run_action` binds the fetch result by value.
  `mid` exists because the two are *not* adjacent — `run --prune` prunes CSVs
  between fetch and build — so fusing them would have reordered that prune.
  The 11 goldens' expected strings are unchanged (the helper flattens a `Plan`
  back to linear form), proving the encoding altered no observable order or
  argument. #29's redirected residuals are now both closed.

  **Live-verified (2026-07-18).** `sudo xtgeoip build -b -c -p` executed
  `[Backup, PruneBin, Clean, Fetch { mode: Local }, Build]` in exactly that
  order, matching the `build_full_sequence` golden; no MaxMind request (Local
  fetch). Its real output (253 countries, 352,296 IPv4 / 254,153 IPv6 ranges)
  also proves the `TempDir` survived to build time — the one lifetime risk in
  moving the fetch result from a struct field to a local binding, which would
  otherwise have failed *silently* as missing data. The `mid` slot (`run
  --prune` only) remains unverified against a live run because `run` fetches
  Remote; pinned structurally by `run_full_sequence`.

The proper "one source" endpoint is #26/#27 (spec-derived plan).

---

## SPEC-DRIVEN ARCHITECTURE: SPECIFIC TASKS


### #92 — docgen / tests: expand spec validation and utilise CLI matrix

`proof.unique_maps_to` is now enforced by the validator. Remaining: expand validation to catch logical contradictions (declared but never used flags, undeclared mutual exclusions, unreachable valid states). Also: `pub const CLI_MATRIX: &[CliExample]` is generated but underutilised — use for fuzzing (seed corpus), property testing (`proptest`/`quickcheck`), and exhaustive parser validation.

**Proof-model blind spot (found 2026-06-07 via exhaustive enumeration).** The
`unique_maps_to` model — one canonical example per error case — *cannot* verify
behavior exhaustively: it can't distinguish `prune+force+clean` from `prune+force`
because both collapse to the same `maps_to`. This is exactly why the `p⊕f` leak
(`build/run -b -p -f` accepted) survived undetected. An exhaustive run of all ~136
flag combinations through `normalize_cli_to_action` is the real oracle. Target model:
declare rules (`p conflicts f`, `prune requires backup|fetch-context`) once and check
*every* combination against them — examples then prove the rules rather than stand in
for coverage. A committed enumeration harness should back this (overlaps #88).

**✅ CONTRADICTION CHECKS DONE (2026-07-18)** — `cli::contradiction`, 4 tests, no new dependencies.

*Fuzzing/proptest deliberately dropped* (user's call). The flag space is 5 bits — 32 combinations per context, 136 total — and `cli::snapshot` already enumerates all of them. At that size exhaustive enumeration strictly dominates random sampling: proptest could only ever rediscover a subset of what the snapshot already pins, non-deterministically, at the cost of a dependency. The "seed corpus / property testing" framing predates the snapshot.

What was actually uncovered was contradiction *between* layers, now closed:

- `spec_examples_agree_with_parser` — runs all 51 `CLI_MATRIX` examples through the real parser and asserts `valid` matches. Nothing previously checked the spec's hand-written examples against the implementation; a lying example would have shipped as docs, man page, **and** test case, all wrong and all mutually agreeing. This is the `p⊕f` shape.
- `every_guard_is_reachable` — enumerates all 32 masks per context and asserts each guard is *first to fire* for at least one. Catches a guard fully subsumed by an earlier one, whose error message would then be unreachable while still appearing live in spec and docs. Failure output names the shadowing guard.
- `guard_keys_are_unique_within_context` — keys identify errors (`[key]: message`) and are what `testcases.yaml` asserts against; duplicates make both ambiguous.
- `every_flag_is_referenced_by_some_guard` — a flag constrained by no guard is either deliberate or an omission; this pins which.

Both substantive tests were **verified to have teeth** by injecting the fault each targets (flipped a matrix `valid`; inserted a catch-all guard) and confirming failure with a useful diagnostic, then reverting.

**Finding — `CliOutcome::ShowHelp` is misnamed.** Writing the oracle surfaced it: `ShowHelp` is produced at exactly one site (`cli.rs`, bare invocation, `flags == 0`) and `main.rs` renders it as `Error [top_level_no_args]` with a non-zero exit. An explicit `-h` never reaches it — clap intercepts that as a `DisplayHelp` error first. So the variant means "no args: print usage and **fail**", the opposite of what its name suggests, and the validity distinction lives in `main.rs` rather than in the outcome type. Not a bug; a naming trap that cost one wrong oracle. Consider renaming to `NoArgs` — filed as a note here rather than a ticket, since it is cosmetic.

✅ **The motivating case is closed (2026-09-02) — but at test time, not
generation time.** `cli.yaml` examples now carry an optional `steps:` list, and
`action::tests::spec_steps_agree_with_plan` drives every documented command
through the real parser and the real `plan()` and compares. `outcome:` stays
authored prose; `steps:` is the machine-checkable half beside it.

The check also refuses a *silent opt-out*: an example that reaches `Action` but
declares no `steps:` is a failure, not a skip. Otherwise the way to defeat the
check would be to omit the field, which is exactly how the drift happened.

Verified to have teeth by reverting the spec, twice:

    "xtgeoip run -c -p": spec says ["clean", "fetch", "prune_csv", "build"],
                         plan() gives ["fetch", "clean", "prune_csv", "build"]
    "xtgeoip build -b -c": reaches Action but declares no `steps:` in cli.yaml

The first is the *actual* historical bug — R-004's order between `850bfd8` and
2026-09-02 — so the check demonstrably catches the thing that motivated it.

✅ **The generation-side validator landed 2026-09-02** — `validate_plan()`,
running before any output is written. It covers the `plan:` section added by
#26/#27, which drove execution with no generation-time checks at all: duplicate
ranks (the order between two steps at one rank is undefined), a context that
builds without fetching, a `fetch_mode` on a context that never fetches, a step
declared but run by no context (the plan-model analogue of
`every_flag_is_referenced_by_some_guard`), selecting on a flag absent from
`flags:`, an empty `why:`, and example `steps:` naming undeclared steps.
Five unit tests plus a live demonstration of three classes being refused.

**The boundary this settled is the more useful result.** docgen links the
library, and the library is built from the *previously generated* sources — so
any check comparing the spec against the program's behaviour is inherently one
generation behind: change a guard and docgen validates the new spec against the
old rules. (Observed directly: a broken `src/generated/plan.rs` stops docgen
itself from building.) So the split is not a matter of taste —

> **generation time owns spec-internal contradictions; test time owns
> spec-versus-program agreement.**

That is why `spec_examples_agree_with_parser` and `spec_steps_agree_with_plan`
stay where they are rather than moving into docgen, and it retires the framing
below.

~~⚑ **Still open, and now with a structural reason.**~~ #92 asks for the validator
on the **generation** side, and this class cannot go there: `xtgeoip-docgen` is
a separate binary and the crate has no `lib` target, so docgen cannot call
`plan()`, `normalize_cli_to_action`, or anything else it would need to know
what a command *does*. Generation-time validation is limited to what is
derivable from the spec alone (contradictions, unreachable states, unused
flags); anything requiring the program's own semantics has to live in the main
binary's tests, as this does. That is a limit on #92's framing, not a gap in
this change — and it is one more argument for the `lib` target that #88's
closure and `tests/` being empty both keep running into.

**A concrete case for it, found 2026-09-02.** `cli.yaml`'s `outcome:` strings
are free text that docgen copies verbatim into the man page, and their
*content* is never checked — `xtgeoip-docgen.rs` asserts only that a valid
example has one, and `xtgeoip-tests.rs` never reads the field. Three of them
had been wrong since `850bfd8` (#24 stage 1, 2026-07-18) moved `Clean` after
`Fetch`: R-004 `run -c -p`, R-005 `run -c -f` and R-010 `run -b -c` all still
claimed clean-before-fetch, as did the man page's EXECUTION ORDER section —
i.e. the shipped docs described the behaviour that change was written to
eliminate. Corrected 2026-09-02, along with the build example (which had
omitted the local fetch entirely) and the invariant sentence, which now
states *acquire before destroying* with its reason.

The fix for the *class* is cheap and belongs here: `action.rs`'s test helper
`steps()` already flattens a `Plan` into exactly the linear sequence these
strings describe, so asserting `outcome:` against it would turn this drift
into a build failure. That check is also the oracle any spec-derived planning
work would need first — see the OVERVIEW.

**A third instance, 2026-09-02.** The CONFIGURATION section documented a
`[maxmind]` request-timeout key. There is no such config option — the timeout
is the `DEFAULT_TIMEOUT_SECS` constant in `fetch.rs` — and `MaxMind` carries
`#[serde(deny_unknown_fields)]`, so an operator who followed the man page
would have produced a config that *fails to load*. Documentation is the only
place that key has ever existed. Corrected along with the rest of the section
list, which had also omitted `paths.archive_prune`, the `[maxmind.credentials]`
sub-table, and `[processing]` entirely.

---

## DOCGEN (xtgeoip-docgen.rs)

### #75 — docgen: `resolve_outcome` conflates resolution and presentation ✅ DONE (2026-07-18)

`resolve_outcome` conflates template resolution, fallback logic, and user-facing output strings — a mini templating engine inside business logic. Split into semantic resolution (typed `ResolvedOutcome`, no strings) and presentation rendering (format-specific, no logic). Each generator renders a `ResolvedOutcome` independently.

**Done as written** — and the split turned out to be justified by a defect, not by taste. The initial assessment was that it was speculative (all four generators appeared to render the outcome text identically, so per-format renderers would have had one variant). That reading was wrong: the code was uniformly *unescaped*, which was the bug rather than the requirement. Two live defects existed, in two different formats:

- **`cli_matrix.rs` emitted unescaped text into a Rust `&'static str` literal.** An outcome containing `"` or `\` produced code that does not compile — docgen would succeed and the *build* would fail on generated source.
- **`xtgeoip.1` emitted unescaped text into roff**, where a leading `.` or `'` is a control line and `\` starts an escape. Silent corruption of the rendered man page.

Both latent today (no such characters in the spec), but `cli.yaml` is hand-edited.

**Structure now:**

- `enum ResolvedOutcome { Succeeds { description }, Fails { reason } }` — semantic, format-free. Template-arg substitution stays in resolution (it produces the same message regardless of target).
- Deliberately **not** `Display`: rendering must be an explicit choice of target, so interpolating an outcome without escaping requires visibly reaching past the renderers.
- `render_plain` (usage.md, tldr.md — prose, no metacharacters), `render_rust_literal` (`{:?}`, which escapes `"`/`\`, supplies the quotes, and unlike `escape_default` preserves the em-dashes in the messages), `render_roff` (escapes `\` → `\e`, prefixes `\&` before a leading `.`/`'`).

Also fixed the two generators that bypassed resolution entirely (`tldr`, `manpage` read `ex.outcome` directly with `unwrap_or_default()` / `unwrap_or("")`) — residual #76 fallbacks in code that didn't call `resolve_outcome`. Both now route through it, so the missing-data guarantee applies uniformly.

**Verification:** generated output is **byte-identical** across all files, so the refactor is provably behaviour-preserving on current data. End-to-end fault injection — a spec outcome containing both `"` and `\` — now emits correctly escaped Rust that compiles; before the split the same input produced a syntax error. 7 new unit tests cover resolution (variant selection, arg substitution) and each renderer's escaping, including non-ASCII preservation.

*(Note: #76's "Ties into #61" is a dangling reference — no #61 exists in this file.)*

### #76 — docgen: silent fallbacks mask missing spec data ✅ DONE (2026-07-18)

Silent fallbacks like `.unwrap_or_else(|| "OK".into())` and `"ERROR".into()` let missing spec data silently become valid-looking output. Distinguish explicit defaults (optional field, spec-defined meaning) from missing required fields (hard error at spec-load time). Enforce required fields via `deny_unknown_fields` or explicit validation. A spec with missing data should not produce output. Ties into #61.

**Analysis first.** Both fallbacks were *unreachable*, so this was a latent risk rather than an active bug — structurally the same as #29's `.expect()`. All 51 examples follow a strict bimodal rule with zero exceptions, which was written down nowhere and enforced by nothing:

| `valid` | `outcome` | `reason` | `maps_to` | count |
|---------|-----------|----------|-----------|-------|
| `true`  | required  | rejected | rejected  | 30    |
| `false` | rejected  | required | required  | 21    |

The first example to break that rule would have been absorbed by `"OK"` / `"ERROR"` and shipped as real-looking text into the man page, the markdown *and* `CLI_MATRIX` simultaneously.

**All three parts done:**

- ✅ **Invariant enforced** — `validate_examples` runs at spec-load time and reports *every* violation at once, naming the case_id and cmd (a spec author otherwise gets "something is wrong" with 51 candidates).
- ✅ **Fallbacks removed** — `resolve_outcome` returns `anyhow::Result<String>` and errors instead of inventing placeholder text. Two closures (`render` in usage.md, `add` in cli_matrix) became fallible to propagate it. It is now unreachable given validation, and is documented as the enforcement of last resort for a caller that skipped it.
- ✅ **`deny_unknown_fields`** on all 14 spec structs. This closes a *live* bug class, not a latent one: a typo'd key in `cli.yaml` was previously ignored in silence. Verified `serde-saphyr` honours the attribute — a `outcomee:` typo now fails with `unknown field 'outcomee', expected one of case_id, cmd, valid, outcome, ...` plus the line number. Safe to add: every key present in `cli.yaml` was already modelled at all three levels.

**Verified by fault injection, then made permanent.** Injecting a typo'd key and a valid-example-missing-`outcome` both produced precise failures; `cli.yaml` was restored after each. Those checks are now 7 unit tests in a new `#[cfg(test)]` module in `xtgeoip-docgen.rs` (the binary previously had none), covering each invariant direction, the "names the case" requirement, and `resolve_outcome` refusing to invent text.

Generated output is **byte-identical** after the change, confirming the fallbacks were indeed never taken.

### #77 — docgen: testcase YAML output has no ordering or schema guarantees ✅ DONE (2026-07-18)

`serde_yaml::to_string(&testcases)?` has no ordering guarantees, no schema enforcement, no versioning metadata. Improvements: stable ordering (by `case_id`), top-level `schema_version` field, post-generation round-trip validation. Do alongside #2 migration.

**Done, with one sub-part deliberately rejected.**

- ✅ **`schema_version` field.** `testcases.yaml` is now `{ schema_version: 1, testcases: [...] }` instead of a bare sequence. `TESTCASES_SCHEMA_VERSION` is declared in *both* `xtgeoip-docgen.rs` (writer) and `xtgeoip-tests.rs` (reader), and the reader **validates** it — `load_testcases` bails with a regenerate hint on mismatch rather than running cases whose meaning may have shifted. A version field nobody checks is exactly the decorative-metadata smell #76 exists to remove, so it is gated by two tests (`wrong_schema_version_is_rejected`, `current_schema_version_is_accepted`). Note this is distinct from the *input* spec's `SUPPORTED_SCHEMA_VERSION` ("3.1", versioning `cli.yaml`); don't conflate "schema 1" with "schema 3.1".
- ✅ **Post-generation round-trip validation.** `generate_testcases_yaml` now serialises → parses back → re-serialises and asserts the two strings match, failing generation if the emitter and parser ever disagree. Catches divergence at generation time instead of as a confusing failure inside the integration suite.
- ❌ **Stable ordering *by `case_id`* — rejected.** The order is already deterministic (top-level first, then `spec.commands` in `BTreeMap` alphabetical order: build, conf, fetch, run). Sorting on `case_id` would yield B, C, F, R, TL, **moving all 15 top-level cases from first to last** — and this suite is order-dependent (#87): `TL-007` (`-c`) empties `output_dir`, so every later case would run against a different state sequence. Validating that costs a rate-capped live MaxMind run, for no gain over the existing determinism. Pinned instead by `emission_order_is_stable`, which asserts the run-length encoding `TL·15, B·13, C·4, F·6, R·13` and carries a comment telling future readers not to re-sort it.

Emission is otherwise byte-stable: the regenerated file differs from the pre-change version by exactly the two new lines — entries were not re-indented or reordered.

### #79 — docgen: BTreeMap ordering not verified for YAML deserialisation ✅ DONE (2026-07-18)

Covered by the same work. The round-trip assertion in `generate_testcases_yaml` plus `emission_order_is_stable` together verify that `BTreeMap` iteration order survives deserialisation *and* is preserved through emission. #2's byte-identical regeneration across a full parser swap (serde_yaml → serde-saphyr) was the original evidence; this makes it an assertion rather than an observation, and CI's `docgen-check` job re-proves it on every push.

Original text: `BTreeMap<String, CommandSpec>` gives deterministic alphabetical ordering at Rust level. Verify the YAML deserialiser preserves stable iteration order when deserialising into `BTreeMap`. Test with round-trip assertion. Do alongside #2 migration.

---

## TEST INFRASTRUCTURE (xtgeoip-tests.rs)

### #87 — tests: system integration nature not documented ✅ DONE (2026-07-18)

Explicitly document that `xtgeoip-tests` is a system integration test suite (not unit tests): tests are order-dependent, require root, require a real release build, and depend on prior test execution. Add to comments and `--help`. ~~Consider a setup/teardown phase for known-good initial state.~~ → split to **#98**.

**Documentation done.** The module header now leads with "SYSTEM INTEGRATION test suite (not unit tests)" and states each constraint explicitly: requires root, requires a real release build (not debug, not a Cargo harness), order-dependent with cases depending on prior execution (`TL-007` empties `output_dir`, so everything after it runs against a cleaned system), hits the fetch-capped live MaxMind API, and must run from the repository root. It also notes that the `#[cfg(test)]` module at the bottom *is* root-free and covers only parsing/versioning/path-resolution — not the cases.

**`--help` added.** The binary previously had none. It documents every flag plus the binary resolution order and the operational requirements, and exits 0 root-free.

**`--rebuild` is called out as effectively required**, with the concrete failure mode named ("Nothing to back up" false failures). That omission has already cost one debugging session; it now appears in both the header and `--help`.

Two tests guard against doc drift: `help_documents_every_flag` (every flag `main` parses appears in `HELP` — help that omits a flag is worse than none, since it implies the flag doesn't exist) and `help_states_the_operational_constraints` (root / release build / MaxMind / REQUIRED survive future edits).

**Observation, not actioned:** unknown flags are still silently ignored, so a typo'd `--rebuil` does nothing and produces exactly the false-failure pattern documented above. Rejecting unknown arguments would close that, but it is a behaviour change and was outside this ticket's agreed scope. Recorded in #98.

### #98 — tests: setup/teardown for a known-good initial state ❌ PLAN REJECTED (2026-07-18)

**Design note: [`docs/design/98-state-ownership-recovery.md`](design/98-state-ownership-recovery.md) — REJECTED, §0.** The note proposed a `restore` primitive as the missing capability underlying #98, #24 and #89. Rejected by the user, and the reasoning is a permanent scope boundary worth reading before proposing anything similar:

> **Backups are context-free; restores are not.** A backup can be made without knowing or caring about the circumstances — it is never made *because* there is a problem, but to provide part of the means to *solve* one. Adopting responsibility for restoring means adopting responsibility for solving the problem, and you cannot solve a problem you do not understand. That is the user's job.

The note's framing was the error: it called "there is no restore" *the finding*, treating an absence as an omission when it was a **boundary already decided**. Three data sources already exist (`output_dir`, `archive_dir`, MaxMind); restore adds convenience, not data. The manifest is our only contract — we may overwrite and delete what it lists, nothing more. "Force clean, then restore" would delete what may be the last intact copy and replace it with something merely hoped to work.

The note was also internally inconsistent: it rejected implicit backups as an unrequested surprise, then proposed restore — which *is* data loss — without applying the same test.

**Still open, and each needing its own decision** (none depends on restore): documenting the ownership model and the two orphan-clean paths; rejecting unknown flags in `xtgeoip-tests`; #24 stage 1 (reorder `Clean` after `Fetch`); #89 integration cases. See §12.

✅ **Documentation residual DONE (2026-09-01)** — §4's ownership model and
the two orphan-clean paths are now written down, which was the item with the
best value-to-cost ratio in this whole area. Evidence it was needed: the
design note's own author got the `build -c` vs `build -c -f` distinction
wrong from reading the code.

- New man-page section **FILE OWNERSHIP** (`docs/spec/manpage-template.toml`
  → `file_ownership`, wired into `xtgeoip-docgen` between EXECUTION ORDER
  and LEGACY MODE): the three categories (owned / unowned / stale-owned),
  the manifest as the ownership record, and the point that the unowned
  guarantee is **structural** — eligibility requires extension `iv4`/`iv6`
  *and* a two-character `[A-Z0-9]` stem, so `xtgeoip.conf.example` cannot be
  selected by any clean, `--force` included.
- **LEGACY MODE** extended with the timing distinction: `build -c` during
  the switch back (manifest still lists `EU`, still owned), `build -c -f`
  after the fact (stale-owned, needs the glob). Re-verified against
  `action.rs` before writing it: the pipeline is `pre` → `fetch_step` →
  `mid` (which holds `Clean`) → `build`, so #24 stage 1's reorder moved
  `Clean` after `Fetch` **but still before** `Build` regenerates the
  manifest. The claim survives that change.
- `src/cli.rs` long help for `-l` and `-f` carries the same distinction, so
  it is reachable from `--help` without opening the man page. Left out of
  `-h`, which stays a one-liner.
- Man page verified with `groff -ww` (no warnings) and read back rendered.

✅ **Precondition checks DONE (2026-09-02).** `HELP`'s REQUIREMENTS block had
listed root, a release build and the repo root since #87, and nothing enforced
any of them — so a non-root shell or a missing `cargo build --release` surfaced
as every case failing in turn, each reporting an error about `xtgeoip` rather
than about the runner, with the real cause in whichever line scrolled past
first.

`check_preconditions()` now runs after argument validation and before anything
is read or spawned. It reports *all* faults at once rather than the first, so
an operator with three things wrong fixes them in one pass:

    Error: 3 unmet precondition(s) — see REQUIREMENTS in --help:
      * docs/generated/testcases.yaml not found — run from the repository root (cwd: /tmp)
      * binary under test not found at target/release/xtgeoip — `cargo build --release`, …
      * not running as root and `sudo -n true` failed — every case is spawned via sudo, …

`/etc/xtgeoip.conf` was added to the list (it is needed and was never
documented as a requirement). MaxMind reachability is deliberately **not**
checked: the only honest probe is a request, and spending part of a
rate-capped budget to discover whether the budget exists is the wrong trade —
recorded in `HELP` so the omission reads as a decision rather than an
oversight.

The environment facts are gathered into a `Preconditions` struct and the
judgement is a pure function over it, so the logic is unit-testable without a
root shell or a release build (3 tests). Verified live from `/tmp` as non-root:
three faults reported, exit 1, before any case ran. `--help` and the
unknown-argument check still precede it, both re-checked.

Still open under #98: the setup/teardown lifecycle itself — a known-good
initial state and a teardown that survives a mid-run failure. Unresolved, and
not addressed here; the `restore` framing was rejected (§0 above) and #89,
which shared the assertion-vocabulary cost, is closed.

Split out of #87 (2026-07-18) because it is a behaviour change to an order-dependent suite, not documentation, and bundling the two would have made a cheap verifiable change risky.

`xtgeoip-tests` has no setup or teardown phase. Cases run in file order against whatever system state the previous case left, so a failure mid-run leaves the system in an arbitrary state and the next full run starts from it. `--rebuild` is a partial, opt-in mitigation rather than a guarantee.

Scope to consider:
- A setup phase establishing a known-good initial state (populated `output_dir`, known archives) rather than inheriting whatever was left behind.
- A teardown restoring that state, so a mid-run failure doesn't poison the next run.
- Whether `--rebuild` should then be the default, or become unnecessary.
- ✅ **Reject unknown CLI flags — DONE (2026-07-18).** `validate_args` rejects anything unrecognised instead of ignoring it, and suggests a near match when the input is a prefix of a real flag — which is exactly the shape of the motivating typo (`--rebuil` → `--rebuild`). Value-taking flags consume the following argument whatever it looks like, so `--case --failed` reads `--failed` as the case id; flags *after* a value are still validated. A value flag with no value is also an error. Checked after `--help` (so help still works alongside a bad argument) and before anything else, so a typo cannot reach a live run. 7 unit tests; verified live: exit 1 with the suggestion, exit 0 for valid args.

Overlaps **#89** (orphan scenarios need deterministic state transitions) and **#24** (no rollback on mid-pipeline failure) — the same "arbitrary state after failure" problem at two levels. Consider designing them together.

**Verification cost:** any change here must be validated by a full `xtgeoip-tests --rebuild` run against live, rate-capped MaxMind. Design on paper first; do not iterate against the server.

### #81 — tests: binary path hardcoded ✅ DONE (2026-07-18)

`format!("target/release/{}", program)` hardcodes release build path. Two options: (1) `env!("CARGO_BIN_EXE_xtgeoip")` if restructured to Cargo integration tests, (2) accept `--bin <path>` flag or `XTGEOIP_BIN` env var. Option 2 is the simpler near-term fix.

**Option (2) implemented**, per the ticket's own recommendation. Option (1) was not viable: `xtgeoip-tests` is a standalone binary invoked under `sudo`, which is not how Cargo integration tests run, so `CARGO_BIN_EXE_*` would have required restructuring the suite and would still collide with the root requirement.

Resolution order is `--bin <path>` → `$XTGEOIP_BIN` → `target/release/<program>`. The default is byte-identical to the previous hardcoded behaviour, so existing invocations are unaffected.

Two pure functions carry the logic — `resolve_bin_override(argv, env_value)` and `resolve_bin(program, override)` — with the environment value passed in as a parameter rather than read inside, so precedence is testable without mutating the process environment. Six unit tests cover it: default, flag, env, flag-beats-env, trailing `--bin` with no value falling through, and the override *not* applying to a non-`xtgeoip` program name (precautionary — every case invokes `xtgeoip` today, asserted by `every_case_is_well_formed`, but a future helper binary shouldn't be silently redirected).

Flags are now documented in the file header, which partly overlaps #87.

Verified root-free by running the release binary with `--case NOSUCH` under both override forms: all 51 cases skipped, exit 0, no `sudo` spawned — confirming the new argv parsing doesn't disturb `--case`/`--failed`/`--rebuild`.

### #89 — tests: orphaned file scenarios not covered ❌ CLOSED — not worth the mechanism (2026-09-01)

**Closed 2026-09-01 without implementing.** The scenarios would cost a new
mechanism to guard a path that already has two guards, one of them
structural.

- **The bug it would regression-test is in deleted code.** `d2bce08`'s data
  loss came from the atomic swap, which was reverted; #24 stages 2–3 are
  permanently rejected. The regression test would guard a feature on a
  do-not-implement list.
- **The unowned-file guarantee is already asserted, twice.**
  `build::tests::detect_orphans_foreign_file_untouched` tests it directly,
  root-free, in milliseconds. And `iv_files` enforces it *structurally* —
  extension `iv4`/`iv6` **and** a two-character `[A-Z0-9]` stem — so
  `xtgeoip.conf.example` cannot match whatever the clean logic does. A
  structural impossibility does not need an integration test.
- **The cost is a mechanism, not a case.** Nothing in the harness can
  express any assertion these scenarios need: `check_output` does substring-
  contains plus empty-means-empty, and `Testcase` carries only `maps_to`,
  `exit_status`, `expected_stdout`/`stderr`, `rebuild`, `timeout_secs`.
  There are no filesystem, negative, or count assertions, and no ordering
  between cases. Adding them also needs a decision about *where* such cases
  live: `testcases.yaml` is generated from `cli.yaml`, which is a spec of
  **flag semantics** and has no notion of scenarios or filesystem state.
- **It would be the most destructive case in the suite**, running
  `build -c -f` against the real `output_dir`.

Two smaller findings from the assessment, recorded so they are not
rediscovered:

- **These scenarios are network-free.** `action.rs` maps
  `FetchMode::Local => fetch(cfg, mode, "", "")` — no credentials read at
  all — so the whole `build -l` → `build` → `build -c -f` cycle runs against
  a pre-existing local archive. The suite's rate-cap warning applies to
  `fetch`/`run`, not to these. Cost was never the blocker; value was.
- **The counts in the design note (§8) are observations, not invariants.**
  "254 countries" / "506 `.iv4`/`.iv6`" is what MaxMind's data held on
  2026-07-18. Asserting them exactly would fail whenever MaxMind adds a
  country — a false positive shaped like a regression.

**What was kept instead:** #98's documentation residual, which is the real
gap here — see below. The design note's own author got the `build -c` vs
`build -c -f` distinction wrong from reading the code, which is direct
evidence the behaviour is confusing. Documented 2026-09-01.

**Premise partly wrong.** The *detection* exists (`detect_orphans`, called after every build, 6 unit tests) and the full legacy→default→clean cycle was demonstrated working on 2026-07-18. What is missing is an *integration* case and documentation of which clean form applies when (`build -c` during the switch back, `build -c -f` after the fact — see §4 of the design note). Concrete scenarios in §8; the final assertion (`xtgeoip.conf.example` survives) is the regression test for the `d2bce08` data-loss bug.

Orphaned files from legacy/default mode switching are not covered by the rebuild logic. Add two explicit test scenarios:

**Scenario A (orphan detection)**: produce orphans → do not clean → run detection command → assert orphans identified.

**Scenario B (orphan cleanup)**: produce orphans → clean → run same detection → assert no orphans. Requires `requires:` dependencies and `rebuild:` annotations in YAML. Further analysis needed to establish if all state transitions are covered.

### #96 — CI / sync: run `cargo test` so the snapshot guard is enforced ✅ DONE (2026-07-18)

Original complaint: `scripts/sync.py` ran docgen → clippy → `+nightly fmt --check` → `build --release`, but **not** `cargo test`, so the CLI-semantics snapshot (`cli::snapshot::cli_semantics_snapshot`, golden at `src/cli_snapshot.golden`, commit `6a92c6f`) and any future `#[cfg(test)]` unit tests (#88) weren't enforced automatically.

**Stale as written (found 2026-07-18).** `cargo test` was wired in at some point after this was filed and the ticket was never updated: `scripts/sync.py:87` and the `test` job in `.github/workflows/rust.yml` both run it. The snapshot guard has in fact been enforced.

The real residual was narrower: both gates ran `cargo clippy --` **without** `--all-targets`, so lints in `#[cfg(test)]` code were never gated — test code compiles under `cargo test`, so this was lint coverage, not correctness. It let the `build.rs` `items_after_test_module` lint sit undetected until a manual `--all-targets` run caught it (fixed `4e610d7`). Both gates now pass `--all-targets`, matching the `build` job, which already used it.

---

### #97 — structure-errors: dead binary, broken at HEAD ✅ DELETED (2026-07-18)

Found 2026-07-18 while migrating #2. `src/bin/structure-errors.rs` was dead code and had been for some time:

- **It fails.** Running it aborts with `error_case 'build_force_ambiguous' refers to unknown template 'build_force_ambiguous'`. Confirmed pre-existing at HEAD (reproduced with the original `serde_yaml` reader, so it is not migration fallout). Its `ErrorSpec` model expects every `error_cases.*.maps_to` to name a `reason_templates` key; `cli.yaml:83` has `build_force_ambiguous: { maps_to: build_force_ambiguous }`, which names no such template. The spec moved on (guards now carry `error: build_force_ambiguous`, `cli.yaml:240`) and this binary was never updated.
- **Its output is unused.** It writes `src/generated/errors.rs.in`, which is untracked, absent from `src/generated/mod.rs` (which declares only `cli_matrix`, `cli_rules`, `error_text` — all written by docgen), and the `CliError` type it generates appears nowhere in `src/`.
- **Nothing runs it.** `sync.py` and CI both invoke only `xtgeoip-docgen`. That is why the breakage went unnoticed.

It was superseded by docgen's `generate_error_text_rs` (`xtgeoip-docgen.rs:776` → `src/generated/error_text.rs`).

**Deleted** on the user's call — redundant and superseded. `Cargo.toml` needed no change (bins under `src/bin/` are auto-discovered, and there was no `[[bin]]` entry); no stray `errors.rs.in` existed to clean up, since the binary always failed before reaching its write. References updated in `CLAUDE.md` and `TODO_tldr.md`.

**Lesson worth keeping.** No gate would have caught this: a binary that *compiles* but fails at runtime is invisible to `cargo build`, `clippy`, and `cargo test`, and nothing in `sync.py` or CI executed it. That is a different failure class from the `--all-targets` lint gap closed the same day. Any future helper binary should either be invoked by `sync.py`/CI or have a smoke test, otherwise it can rot silently exactly like this one did.

---

## TOOLING / AGENTS

### #95 — import generic agents from private/agents-out/ ✅ COMPLETE (2026-07-18)

**All seven imported and adapted** into `.claude/agents/`. The initial plan was docs-auditor alone, on the reasoning that the rest had no consumer; the user reviewed the roles and confirmed all are useful here, which also corrected a mistaken assumption on my part — **bug-hunter and guardian-security are deliberately disjoint**, not overlapping: bugs-only versus security-only, with each instructed to hand findings in the other's domain across rather than audit them.

| Agent | Remit | Output |
|---|---|---|
| `guardian-security` | security vulnerabilities **only** | `private/guardian/` + `.sig` files |
| `bug-hunter` | correctness bugs **only** | `private/bug-hunter/` |
| `optimisation-advisor` | performance **only**; identifies candidates, does not decide them | `private/optimisation/` |
| `docs-auditor` | hand-maintained docs vs source | edits the audit set in place |
| `data-flow-tracer` | path responsible for one named value | `private/traces/` (+ `tools/`) |
| `flow-doc-generator` | Mermaid + prose, for understanding and porting | **`docs/flow/`** (tracked) |
| `deep-research-collector` | internet research, cited | `private/research/` |

Adaptations worth recording:

- **`optimisation-advisor` carries this session's method as normative rules**, with the three worked examples: measure the *enclosing* operation, not the function (#71 achieved 4.61× and still saved only 0.6–3.3%); check the proposal targets the expensive half (#54 parallelised writes while leaving decompression serial); sweep the parameter space before assuming a trade-off exists (#99 — level 6 was strictly dominated, no trade to make); and Amdahl's ceiling comes from the work distribution, not the core count (2 of 12 ZIP entries are 99.8% of the bytes). It must state cost-share, ceiling, and the deciding experiment for every candidate.
- **Every agent is given real magnitudes** — 564,448 IPv4 / 558,545 IPv6 rows, 253 countries, 506 output files, ~45.6 MB CSV from a ~4.7 MB ZIP, extraction ~124 ms, backup ~363 ms — so findings are judged against actual scale rather than guessed at.
- **All are told `src/generated/` and `docs/generated/` are docgen-owned**: a defect there is a defect in `docs/spec/cli.yaml` or the generator, to be reported by spec key, never edited.
- **All are told not to run `git`** (commits go via `private/COMMIT_MSG` + `scripts/sync.py`), not to run `xtgeoip-tests` (root + **rate-capped** MaxMind), and not to run docgen (it rewrites generated files).
- **`TODO.md`'s INVARIANTS are cited as normative**, with constraint 5 called out to `optimisation-advisor` and `bug-hunter`: never trade away working parallelism.
- **`deep-research-collector` is explicitly barred from transmitting project material** — MaxMind `account_id`/`license_key` and anything under `private/` must never appear in a query, URL or saved document.
- **`bug-hunter` is told to distinguish latent from live.** Several invariants here are held by construction (`Plan::Pipeline`, `Version::parse` confinement); reporting them as bugs would waste the reader's time.
- **`flow-doc-generator` writes to `docs/flow/`, which is tracked** — the one agent whose output is not under `private/`. Deliberate: these documents exist to be read by someone porting or reimplementing the code, which is impossible if they do not survive a clone. The agent is told two consequences: its output is *published*, so it must meet the standard of committed documentation; and `docs/flow/` is hand-maintained, so it falls inside `docs-auditor`'s remit and must not be confused with docgen-owned `docs/generated/`.

Two of seven previously; now all seven.

Adaptation notes:
- Default audit set is exactly what `private/WORKFLOW.md` names: `README.md`, `CLAUDE.md`, `TODO.md`/`TODO_tldr.md`, `docs/design.md`, `docs/legacy.md`. All six verified to exist.
- **`docs/generated/` and `src/generated/` are hard off-limits** — docgen-owned. If their content is wrong the *spec* is wrong; the agent must report the offending `cli.yaml` key and stop. Also excluded: `src/`, `docs/spec/`, `private/`, `Cargo.*`, `scripts/`, `.github/`.
- The generic template's standard set assumes a hand-written man page and config example. Here the man page is **generated** (`docs/generated/xtgeoip.1`), so it is off-limits; `conf/usr/share/xt_geoip/xtgeoip.conf.example` is in scope only when explicitly named, since `WORKFLOW.md` does not list it.
- Flagged for report-only, not editing: `docs/xtgeoip-usage.md`, `docs/xtgeoip-usage.yaml` and `docs/xtgeoip-wip.1` are hand-maintained files sitting alongside generated equivalents. Their status is unclear and resolving it is a decision, not an audit.
- Given a **"stale TODO premises"** section in its output format and an explicit instruction to verify each open ticket's premise against source. That is the highest-value work available to it: this session alone found four tickets describing code that no longer existed (#96, #54, #88, #38).

⚠ `.claude/` is gitignored, so the agent definition is **local-only** and will not survive a fresh clone — the same asymmetry as `scripts/`. `private/agents-out/` is likewise gitignored, so the templates are local too.

Two directories exist, and this ticket only named one: `private/agents-in/` holds the original definitions from another project (`cdda2img`) with full frontmatter but foreign paths; `private/agents-out/` holds the genericised role descriptions with `[bracketed]` placeholders and no frontmatter by design. Import from **`agents-out/`**.

Original ticket text follows.

### #95 (original) — import generic agents from private/agents-out/ ✅ SUPERSEDED — original text, kept for provenance

*(The ticket as first written. Delivered and closed at `#95 … ✅ COMPLETE (2026-07-18)` above; this copy is history, not open work. Marked 2026-09-01 — it was the last `###` heading that read as open without being so.)*

The seven project-agnostic agent role descriptions in `private/agents-out/` (bug-hunter, data-flow-tracer, deep-research-collector, docs-auditor, flow-doc-generator, optimisation-advisor, guardian-security) are to be imported as actual project agents, adapting each by filling its `[bracketed]` placeholders for xtgeoip (`[language]` = Rust, `[source-dir]` = `src/`, `[output-dir]` under `private/`, etc.).

Priority / notes:
- **docs-auditor** first — `private/WORKFLOW.md` already delegates its documentation-check step to this agent. Audit set: `README.md`, `CLAUDE.md`, `TODO.md` / `TODO_tldr.md`, `docs/design.md`, `docs/legacy.md`. Mark `docs/generated/` and `src/generated/` as docgen-owned (off-limits — change `docs/spec/cli.yaml`, not the output).
- **guardian-security** — GPG key already provisioned (ed25519, fpr `227E5FE6EB2D3E9EE5883CB1F4BF35E6DC8029B0`; public key `docs/guardian_public.asc`; keyring `private/guardian/gnupg/`; setup script `private/guardian/guardian-security-pre.sh`). Set `[signable-dirs]` to the tracked source dirs (note: anything under `private/` is gitignored, so per-file `.sig` signatures only make sense for files outside it).
- Remaining (bug-hunter, optimisation-advisor, data-flow-tracer, flow-doc-generator, deep-research-collector): adapt as needed when wanted.

---

## TOOLCHAIN MAINTENANCE

### Pin staleness — the other half of the drift problem ✅ REPORTED (2026-09-04)

This closes the thread the whole episode started from. The original failure
was one lint:

```
error: can be more succinctly written as a byte str
   --> src/build.rs:712:35
712 |             Some(CountryCode::Iso([b'U', b'S']))
    |                                   ^^^^^^^^^^^^ help: try: `*b"US"`
    = note: `-D clippy::byte-char-slices` implied by `-D warnings`
```

Six occurrences, all in `build.rs`, all in test code, `lint` job only —
`build`, `test` and `docgen-check` were green throughout. **The code was never
broken.** `clippy::byte_char_slices` was a *style* lint introduced in a Rust
newer than the local one: CI resolved `@stable` at run time and got 1.98.0
while a rustup **directory override** held this repo at 1.94.0, invisible
because such an override outranks `rust-toolchain.toml`. `-D warnings` did the
rest. Fixed in `78df343`; both toolchains pinned in `4ef7aa1`.

**Pinning solved that and created its successor.** `_check_toolchain` catches
the local toolchain diverging from the pin. Nothing caught the **pin itself**
ageing — and a pin that is never revisited is the old stale toolchain with
better paperwork. That was the residual left open in HOUSEKEEPING.

**`rustup check` is the whole answer.** Local, ~0.46 s, installs nothing,
reports the newest stable, nightly and rustup. Wired into `sync.py` as
`_check_toolchain_freshness`, which compares it against `rust-toolchain.toml`
and `rustfmt-toolchain` and prints, for example:

```
Toolchain pins have drifted (reporting only — nothing is blocked):
  * rust-toolchain.toml pins 1.98.0; stable is now 1.98.1
  * rustfmt-toolchain pins nightly-2026-09-01; latest nightly is 2026-09-03 (2 day(s) older)
  * rustup is 1.26.0; 1.29.1 is available
```

**A report, never a gate, and that is the point.** A compiler release is
somebody else's timetable. A check that refused to commit until the pin was
current would be the original surprise back in a different costume — the
operator decides when to move, and reads the new lints when they do. Throttled
to weekly (`SYNC_FRESHNESS_DAYS`) so it is a reminder rather than noise;
`rustup check` is the on-demand form.

Four paths exercised: pins drifted (all three reported, exit 0), throttled
(silent), `rustup` refusing to answer (warns and continues — and deliberately
does **not** write the stamp, so a failed check cannot silence the next one for
a week), and all pins current (`✅ Toolchain pins are current.`).

**`sync.py` is gitignored**, so the durable half lives in the repository:
`rust-toolchain.toml` carries an "is this pin stale?" note naming the command,
and `CLAUDE.md` lists it among the development commands.

⚠ **The pin protects only this repository.** rustup's default is still
`stable` at 1.94.0, so outside `xtgeoip` a bare `cargo` on this machine is the
same compiler that started this. Deliberately not changed — a machine-wide
default is a decision about other projects. Recorded in
`private/OUTSTANDING.md`.

---

## MAN PAGE PROSE

### The template was the last unchecked surface ✅ CHECKED (2026-09-03)

`docs/spec/manpage-template.toml` was hand-written and verified by nobody,
sitting in the middle of a pipeline where everything else derives from
`cli.yaml`: flag validity via `cli_rules.rs`, step ordering via `plan.rs`,
error text via `error_text.rs`, test cases via `testcases.yaml`. Three defects
were found in it by reading on 2026-09-02 — stale step ordering after #24
stage 1, the whole `conf -c` credential workflow missing since #103, and a
`[maxmind] timeout` key that never existed and that `deny_unknown_fields`
would have *rejected* had a reader copied it. Nothing prevented a fourth.

**Five checks, all at test time.** The #92 boundary decides the placement:
these compare prose against the *program*, so they belong with the program,
not in docgen. They also follow the one that already existed —
`cli::contradiction::global_options_are_documented`, which reads
`docs/generated/xtgeoip.1` — and each sits with what it asserts about rather
than in a new module of its own.

| Check | Home | Catches |
|---|---|---|
| `manpage_execution_order_agrees_with_the_planner` | `action.rs` | the #24 stage 1 defect |
| `manpage_documents_every_shipped_config_key` | `config.rs` | a shipped key nobody wrote up |
| `manpage_names_no_unknown_config_key` | `config.rs` | the `timeout` defect |
| `manpage_config_defaults_match_the_shipped_example` | `config.rs` | a documented default that drifted |
| `unknown_maxmind_key_is_rejected_as_documented` | `config.rs` | the strictness claim going false |

**EXECUTION ORDER is the valuable one.** The section lists four invocations
with the exact step sequence each produces — the same claim
`spec_steps_agree_with_plan` checks for `cli.yaml`'s `steps:`, written a
second time by hand in prose. The check parses the four `.TP` pairs out of the
generated `.1`, maps each prose phrase to the name `step_names` uses, and
compares against the real planner. Two phrases map to `fetch` ("fetch" and
"read local archive"): the man page's distinction between a download and a
cached read is useful to a reader, and the explicit map is what lets the prose
stay readable without the check losing its grip. An unmapped phrase is a
failure, never a skip.

**A fourth defect, found by the tooling this time.** `[logging] log_file`
named the key and stated no default, alone among the keys that have one, while
the shipped example sets `/var/log/xtgeoip.log`. The first three were found by
reading; this one was not. Fixed in the same pass.

**One stated exception.** `credentials` is documented but is not in the
shipped example, because `conf -c` writes it and the example says in as many
words that credentials must never be put there by hand. Recorded as a named
constant with the reason, and asserted to stay small — an exception list that
grows is the universe being chosen to fit the test.

**Two scoping decisions worth keeping.**

- The defaults check only fires where the prose actually says `default:`.
  `url` and `threads` are *described* rather than defaulted, and making the
  man page repeat a 78-character download URL would be a check written for the
  tooling's convenience rather than the reader's.
- Commented-out TOML in the example counts as shipped. `[processing]` and
  `threads` are commented *because they are optional*, not because they are
  absent, and the man page documents them. The uncommenting heuristic guards
  itself: its result must still parse as TOML, so swallowing a prose comment
  fails loudly instead of quietly checking less.

**Every failure message names `docs/spec/manpage-template.toml`, not the
generated `.1`.** A reader who fixed the generated file would see the test
pass and have the fix silently reverted by the next docgen run.

**Teeth verified, one perturbation per check**, each reverted afterwards: the
historical clean-before-fetch ordering (caught, with the exact diff), a
deleted ordering (the count guard), an invocation the guards reject, the
`timeout` key reinstated verbatim (caught), an undocumented key added to the
example, a drifted default, and `deny_unknown_fields` removed from `MaxMind`.

**Follow-up the same day** (the commit after *check the man page against the planner and the config*)**.** Both directions now
compare against one extraction of the section's `.I key` / `.B [section]`
tokens rather than searching the prose for a substring. Not a live bug — no
current key is a substring of another, so the substring form gave the right
answer for every key that exists today — but `archive_dir` and
`archive_prune` are already prefix-siblings, and a future `output_dir_mode`
would have been reported as documented the moment `output_dir` was. Removing
a latent trap while the reasoning is fresh is cheaper than rediscovering it.
Teeth re-verified after the refactor, since a refactor that quietly disarms a
check is precisely the failure this area exists to prevent.

The same follow-up explains an asymmetry that would otherwise read as an
oversight: `manpage_execution_order_agrees_with_the_planner` pins an *exact*
count of four, while the defaults check asserts a *floor* of four. The
orderings are a closed list of illustrations, so a change in their number is
worth a look; the documented defaults grow whenever a key gains one, and
`log_file` took that count from three to four in this very change. An exact
count there would fail on an improvement to the documentation.

**Not done: the generation-time half.** Checking the template against
`cli.yaml` — that every command and flag it names exists in the spec — is
spec-internal and would belong in docgen's `validate_*` family. Left out
deliberately: the five above are a complete deliverable, and bundling a second
validator into the same pass would dilute both.

---

### The sixth defect: OPTIONS prose vs the guard table ✅ FIXED (2026-09-05)

Found by the docs-auditor sweep of 2026-09-04, decided by the maintainer on
2026-09-05. `manpage-template.toml:212` says `run -b -p` "is an error: the
prune target is ambiguous"; `cli.yaml` R-012 declared it **valid**, the guard
`run_prune_ambiguous` required `b ∧ c ∧ p`, and the binary accepted it. Both
halves live under `docs/spec/`, so this was the spec contradicting itself, not
docs drifting from code — which is why it needed a decision rather than a
patch. **The prose was right; the guard was wrong.**

**The planner was never the enforcement point, and could not have been.**
#29 proposed exactly that (option (b), planner-as-arbiter) and it was declined
on 2026-07-16 in favour of guards. `plan_generated` returns `Plan`, not
`Result<Plan>`; every arm is straight-line `if flag { push }`, so it has no
failure mode by construction. But the sharper point is that **option (b) would
not have caught this either**: `run -b -p` yields a perfectly deterministic
`Pipeline { pre: [Backup], fetch: Remote, mid: [PruneCsv] }`, satisfying #29's
own test. The hole was in the rule, not the mechanism.

**Root cause — an implicit target broke the pattern.** Sweeping all three
modifiers, a modifier is ambiguous exactly when the invocation offers it more
than one candidate target:

| Modifier | Targets | Implicit target? | Verdict |
|---|---|---|---|
| `-l` legacy | `build` only | never — always exactly one | sound |
| `-f` force | `backup`, `clean` | never — both flag-driven | sound in all three contexts (`b ∧ c ∧ f`) |
| `-p` prune | `prune_bin` (after backup), `prune_csv` (after remote fetch) | **yes, in `run`** | the one gap |

`top_level` has no fetch, and `build` fetches `local` — it *reads* an existing
archive, so no new CSV exists and `prune_csv` is meaningless there; both
therefore have `prune_bin` as their only target and guard `p` against a missing
`b`. `run` is the exception: `always: [fetch]` with `fetch_mode: remote`
produces a new CSV **unasked**, and `-b` adds a new binary tarball beside it in
the same `archive_dir`. Two targets. The guard was copied from the `-f` shape
(`b ∧ c ∧ f`), where both targets are named by flags — but here one target is
implicit, so the correct predicate is `b ∧ p`, and `-c` plays no part in
choosing a prune target at all.

**Mis-transcribed, not mis-designed.** `docs/xtgeoip-usage.yaml:61` — the
hand-written enumeration that predates `cli.yaml` — already states the rule as
`run_prune_ambiguous_with_backup: when: has: [b, p]`, the exact predicate now
shipped. The intent was `b ∧ p` from the start; the `-c` was picked up in the
move to the spec. That file's per-combination rows list both `run -b -c -p`
and `run -b -p` as ambiguous, which is consistent: the first is an instance of
the rule, not a separate one.

**`proof.unique_maps_to` refused the first attempt**, fittingly. Tightening the
guard made R-007 (`run -b -c -p`) and R-012 (`run -b -p`) fire the same error
key, and the invalid example is the *sole* declared link from an error key to
its message text (`xtgeoip-docgen.rs:708`). R-007 became a strict superset
firing the identical guard, so it retired; R-012 carries the mapping because it
is the case the OPTIONS prose actually names. Every surviving `case_id` still
denotes the same command. Corpus 52 → 51 cases, `R` 13 → 12.

**Breaking, and knowingly so.** `run -b -p` worked before; it under-pruned
(tarballs accumulated, nothing destructive). It now exits 1. Any script or cron
entry using it must split into two invocations, which is what the prose has
always instructed: "To prune both, run the program twice."

**Not done: a check for this class.** The five checks above compare prose
against the *planner* and the *config*; nothing compares OPTIONS prose against
the guard table, which is why this survived them. A sixth check would parse the
"is an error" claims out of the OPTIONS `.RS` block and assert each names a
combination the guards actually reject. Harder than the other five — the claims
are free prose, not a structured list — and worth doing only if a second one of
these appears.

---

## AUDIT TRIAGE (2026-09-04 sweep)

### Eight findings cleared ✅ DONE (2026-09-05)

Four agents swept `src/` on 2026-09-04 (reports in `private/`). The findings the
maintainer triaged as worth acting on are all closed. Each carries a regression
test, and each test was checked against the *old* behaviour before being kept.

**F-001 — a failure reported through the logger that failed to install.**
`init_logger` chained `fern::log_file(path)?` inside the dispatch, so one `?`
aborted the whole dispatch, the terminal sinks were never installed, and the
resulting error was then reported by `main`'s catch-all via `messages::error`
→ `log::log!` → discarded. `xtgeoip conf -s --log-file /nonexistent-dir/x.log`
exited 1 having written nothing to stdout *or* stderr. Worse in a
`[logging] log_file` whose directory has gone: it poisons every command on the
host, silently.

That inverted this module's own documented contract — "no file" must never mean
"no output" (#1) — by making a *broken* file worse than no file at all. Two
changes, because the class is wider than the instance:

1. `messages::init_logger` now **degrades**: a file sink that will not open is
   a warning (to terminal *and* syslog, for the cron case), not a fatal error.
   The decision is split into `open_file_sink` so it is testable — the global
   logger can only be installed once per process, so `init_logger` itself
   cannot be exercised twice in one test binary.
2. `main::install_logger` reports a genuinely unusable logger to stderr and
   syslog **directly**, never through `messages::error`. That funnel
   structurally cannot print this one error.

**M-1 (guardian) — the one unbounded remote read.** `fetch.rs` capped the
archive body (`MAX_DOWNLOAD_BYTES`) and extraction (`MAX_EXTRACT_BYTES`) but
read the checksum response with a bare `read_to_string`, then wrote the whole
thing to `archive_dir`. A hostile origin in the MaxMind chain — including the
credential-less redirect target — could drive the root-privileged process into
OOM. Now bounded by `MAX_CHECKSUM_BYTES` (4 KiB) with a `+1` breach check
matching the archive path. `expected_hash` is additionally validated as exactly
64 ASCII hex characters: that changes no accept/reject decision (a non-digest
can never equal the computed hash) but turns "checksum mismatch" into "invalid
checksum format" when the body is an HTML error page, which sends the reader
to the right place.

**F-003 — `detect_orphans` deleted any `.blake3`/`.sha256`, not only ours.**
The man page's FILE OWNERSHIP section promises unowned files are "**never**
touched, by any operation", scopes the stale-manifest exception to files "from
an earlier build", and says the guarantee is "enforced structurally, not by
convention". That was true of the clean path (`backup::iv_files`) and was not
applied here at all: the partition tested the extension and nothing else, so an
operator's `SHA256SUMS.sha256` in `output_dir` was deleted silently by the next
build. `build::is_ours` now applies the documented structural test to both
families — two-character `[A-Z0-9]` stems for `iv4`/`iv6`, and the
`GeoLite2-Country-bin_<version>` shape for manifests — and unowned files are no
longer even listed as orphans, since they are not ours to have an opinion
about.

**F-006 — the plan emitter dropped steps silently, twice.** `generate_plan_rs`
had `if let Some(expr) = flag(letter)` with no `else`, so a `selects:` entry
whose letter `ACTION_BINDINGS` does not bind for that context vanished from the
generated planner with docgen exiting 0. And the `pre`/`mid` rank windows are
open either side of the fetch, so a step ranked at or after `build` fell into
neither and was discarded. Both are now hard errors naming the context and the
step. The second is unreachable while `build` holds the maximum rank — but
`26-spec-derived-planning.md` records that assumption as the one most likely to
break next, so it fails loudly rather than emitting a short plan. Verified by
introducing each fault into `cli.yaml` and confirming the message, then
reverting.

**O-003 — BLAKE3 fed 4 and 16 bytes at a time.** `write_country_v4`/`_v6`
hashed each range as it was appended, so the wide AVX2/AVX-512 path (which
needs >= 1 KiB per call) degraded to one 64-byte block per call. Now one
`blake3::hash(&buf)` over the finished buffer. **Reproduced independently
before accepting the finding**: 10.42 MB at production volume, best of 7,
**338 -> 1,739 MB/s (5.14x)**, digests asserted identical each rep. The
optimisation report claimed 318 -> 1,725 MB/s; the measurement stands. The
figure is conservative — the incremental arm of the benchmark omits the buffer
construction the real code also does. The manifest digests are compared on
every later *verified* operation, so the equivalence is load-bearing and is now
pinned by test against the incremental form, including the empty-range case.

**`CLAUDE.md` said "There is no `cargo test` suite".** There were 197 when the
claim was found and 205 after this pass added its regression tests. Replaced
with the two-suite description: unit tests are hermetic and free to run,
`xtgeoip-tests` needs root, hits the live rate-capped API, and is not to be run
casually.

**F-002 — a failed country-file write left a dangling `version` pointer.**
`write_outputs` runs all 506 parallel writes and collects the results, so a
single ENOSPC/EACCES/EIO aborts the build only after most files have landed;
`generate_manifest` never runs, and `output_dir` then holds a partially written
database while `version` and the manifest still describe the previous one. An
atomic swap is not the answer and is not on the table (#24 stages 2–3, rejected
— `d2bce08` lost data). Two smaller changes instead:

1. **The pointer is written last.** `generate_manifest` wrote `version` before
   the manifest, so a failure between the two left `version` naming a manifest
   that was never written — `backup::gather_files` in `Verified` mode reads the
   pointer, derives the manifest name from it, and aborts with "Manifest
   missing … Use -f to force" on data that is otherwise intact. Reversed, the
   same failure leaves the previous pointer and previous manifest still
   agreeing, plus an unreferenced new manifest that `detect_orphans` sweeps up
   on the next successful build. Nothing is renamed or staged; one write simply
   precedes the other. Checked before changing: the `Verified` path follows the
   pointer (`backup.rs:238-266`), and the force path's `all_blake3_files` glob
   only *collects* files, so a stray manifest is harmless there.
2. **The failure says what state the directory is in.** The old `bail!` was
   `"N file write(s) failed during build"` and said nothing about `output_dir`
   being half-written, so the operator had no reason to expect the follow-on
   refusals. It now names the count written, the directory, the disagreement
   between it and the manifest, which operations will refuse while that holds
   (a backup or a clean without `-f` — both take `BackupMode::Verified`), and
   that a successful re-run rewrites every country file and regenerates the
   manifest.

Two tests, one for the invariant (`version` resolves to the manifest that was
written) and one for the regression, which fails against the old order: with a
directory standing where the manifest goes, `fs::write` returns EISDIR and the
pointer must not have advanced. Verified — under the old order it advanced to
`20260324` while the manifest write failed.

**F-007 — docgen left the generated tree half-regenerated.** The eight outputs
were generated and written one at a time, so an emitter that fails part-way
overwrote the earlier files and left the rest stale. `cargo test` then compared
a new `CLI_MATRIX` against an old planner and failed somewhere unrelated, and
the partial regeneration was easy to commit by accident. #92 moved the
*validators* ahead of all writes; this closes the remaining gap, for the errors
the validators cannot see. `render_outputs` now renders all nine into memory
and `main` writes them in a loop. Verified that this is safe to hoist: no
`fs::` call appears in any `generate_*` body — all of them are in `main` — and
the regenerated tree is byte-identical.

No unit test, for the same reason F-006 has none: the property is
end-to-end, and the test would be a new mechanism rather than a new case
(TODO.md's own standard). Verified by fault injection instead, against both
codepaths. A `plan.steps` entry with no `Step` variant passes every validator
and then fails in `step_ctor`; paired with a spec change that lands in
`usage.md`, the old code wrote the new `usage.md` and *then* failed, leaving
the tree modified, while the new code fails with the tree untouched. Note the
first injection attempt proved nothing — it tripped F-006's `ACTION_BINDINGS`
check at validator stage, and a second attempt changed only outputs the fault
does not affect, so `git diff` saw nothing. The reproduction only counts once
an *earlier* output genuinely differs.

**Taken later.** O-001 and O-002, the two large optimisations, were left out of
this pass because `build` and `backup` were not the complaint. The maintainer
asked for both on 2026-09-05; see the next section.

**`src/fetch.rs` needs re-signing.** M-1's remediation invalidated guardian's
signature. Row added to `private/guardian/needs_reverification.md` by hand with
the `.sig` deliberately left in place, per that file's own convention, so the
next guardian run raises the BAD signature itself rather than inheriting this
note's word for it.

### O-001 and O-002 — the two large optimisations ✅ DONE (2026-09-05)

Both accepted on the acceptance criteria the report itself wrote, and both
proved by A/B against the implementation they replaced rather than by
assertion. Measured on the 2-core host, over the real 2026-09-01 archive
(1,088,244 rows) and the real 509-file `/usr/share/xt_geoip` tree.

**The oracle came first.** The current code was run twice into two directories
and the trees diffed: byte-identical, 508 files each. Without that, "the output
is unchanged" is unfalsifiable — a nondeterministic baseline can never be shown
to have been preserved. Everything below rests on it.

**O-001 — block loading, 91.1% of `build`.** Three changes, one function:

* `par_bridge` over `csv::StringRecord` became a chunked `par_iter`. The bridge
  pulls rows from one sequential iterator through a mutex, so the csv reader
  itself never parallelised; the mmap is now split into byte ranges at newlines
  and each worker gets its own reader. This also lifts the width cap — the old
  shape was parallel only across the two files.
* A reused `ByteRecord` replaced the per-row `StringRecord`, so the row loop
  allocates nothing, and `ipnetwork::IpNetwork::parse` was replaced by byte
  parsers that never build a `str`.
* The `HashMap<String, CountryCode>` regroup — ~1.09M SipHash probes on **one**
  thread, a serial section between two parallel ones — became a sorted
  `Vec<(u32, u16)>` and a dense `Vec<Vec<_>>` indexed by position.

Whole-`build` **563.2 → 292.6 ms, 1.92×** (min of 15 interleaved runs,
alternating two binaries; the box was under load from other agents and single
runs spread 30%, so the ratio is the number, not the absolutes). Acceptance
was ≥ 1.8×. Output byte-identical to the baseline tree, manifest included.

Three wrong-answer holes the report flagged in its own sketch, all closed:

* **Chunk splitting assumes nothing is quoted.** A quoted comma shifts every
  field boundary after it *inside one chunk*, with no parse error — a wrong
  country, silently. Both blocks files hold zero `"` bytes across 44 MB, but
  that is checked on every run, not assumed; a quoted file falls back to a
  single range and the csv reader's own quoting rules.
* **`parse_u32` must be checked, not wrapping.** A geoname above `u32::MAX`
  would wrap and could land on a *real* ID in the sorted table. The string map
  simply missed and yielded `O1`.
* **A non-numeric geoname has no dense slot.** Demoting it to `O1` would be a
  wrong country. Such keys go to a fallback map, normally empty and never
  probed. Leading-zero spellings go there too: `"0123"` and `"123"` are
  distinct `HashMap` keys but one integer.

The old implementations stay in the tree under `cfg(test)` as differential
oracles — `resolve_country_code`, `cidr_to_range_ipv4`/`_ipv6` — and `ipnetwork`
moved to `[dev-dependencies]`. That is what caught the three divergences no
amount of re-reading did: `ipnetwork` accepts `/+8`, accepts `/0128`, and
accepts a bare address as a full-length prefix. None occurs in a MaxMind
archive, and all three would have been silent — the row dropped, not rejected.
A hand-written assertion would have encoded the same misunderstanding as the
parser it was checking.

**O-002 — parallel gzip in `backup`, 95.2% of `backup`.** The tar is built in
memory, split, and each part deflated into its own gzip member; concatenated
members are one valid gzip stream. No new dependency. `backup` **245.4 →
149.5 ms, 1.64×** (acceptance ≥ 1.6×) with the archive **0.074% smaller** —
splitting does not cost size on this data, the same non-monotonic behaviour #99
found in the level sweep. Decompressed tar streams are byte-identical, all 508
entries, `gzip -t` clean, and `tar xzf` reproduces the source tree exactly.

Chunk count is derived (`current_num_threads() × 2`), not fixed. The sweep is
why: three members was *consistently* the worst of every count ≥ 2 across two
independent runs, because with two workers it costs two rounds and idles one
worker through the second. Any multiple of the width fills every round. A
buffered-but-serial variant was measured too — 1.04× — so the win is the
parallelism and not the `Vec`.

One real compatibility cost, and it is a silent one: `flate2::read::GzDecoder`
stops after the first member and *reports success*. On the real archive it
returns 2,838,145 of 11,352,576 bytes. The only in-repo reader was the test
helper in `backup.rs`, now `MultiGzDecoder`; a test pins the truncation so the
hazard cannot be reintroduced unnoticed. External `gzip` and `tar` are
unaffected. `gzip -l` now reports the last member's size rather than the total.

**Two things the A/B nearly missed.** The first `abbuild` driver hard-coded
`legacy: false`, so the one path where the country key set *differs* — legacy
maps geonames 6255148/6255147 to a continent code, adding an `EU` key and
colliding `AS` — went unmeasured, and that is precisely the path where every
dense slot shifts. Re-run: byte-identical, 510 files each, and the flag
verifiably changes the output. The second was a doc comment claiming a chunk
count "was the measured shape" when the number had been inherited from the
report and never measured here. Swept: 1/2/4/8/16 per thread give
300.0/287.4/260.6/269.7/255.9 ms, all emitting byte-identical trees, so four
is a knee rather than an optimum and the comment now says so.

**Not taken.** O-003 through O-007 remain untouched — the ask was these two.

---

---

## MAINTENANCE / SUPPLY CHAIN

### Dependency advisories — six live, unnoticed for four and a half months ✅ BUMPED (2026-09-03)

The 2026-09-02 toolchain work closed *compiler* drift and left a matching hole
open: `HOUSEKEEPING` predicted that nothing reported whether crate updates or
advisories had gone stale. Checking that prediction turned it from a process
gap into a live exposure. `cargo audit` against the lockfile at `ec1691a`:

| Crate | Advisory | Reachable here? |
|---|---|---|
| `rustls-webpki` 0.103.10 | RUSTSEC-2026-0098 — URI name constraints incorrectly accepted | **yes** — TLS to MaxMind |
| `rustls-webpki` 0.103.10 | RUSTSEC-2026-0099 — name constraints accepted for wildcard certs | **yes** |
| `rustls-webpki` 0.103.10 | RUSTSEC-2026-0104 — reachable panic in CRL parsing | **yes** |
| `h2` 0.4.13 | RUSTSEC-2026-0258 — unbounded empty DATA frames | **yes** — via `hyper`/`reqwest` |
| `crossbeam-epoch` 0.9.18 | RUSTSEC-2026-0204 — invalid pointer deref in `fmt::Pointer` | in tree (Rayon), needs a `Debug`-print of an `Atomic` we never do |
| `quinn-proto` 0.11.14 | RUSTSEC-2026-0185 — remote memory exhaustion (7.5 high) | **no** — lockfile-only |

Plus four non-fatal warnings: `anyhow`, `memmap2` and `rand` unsound,
`chacha20` yanked.

**The one rated `high` is the one that does not apply.** `cargo tree -i
quinn-proto -e normal --target all` returns no edge — it is a lockfile entry
from a `reqwest` feature that is not enabled, so it is never compiled in. The
three that matter are the `rustls-webpki` ones, two of which are certificate
name-constraint validation accepting inputs it should reject, directly on the
path that fetches from MaxMind. Recorded because the severity column would
otherwise point at the wrong row.

**Fixed by `cargo update` alone** — lockfile only, no `Cargo.toml` change:

```
cargo audit on the lock at ec1691a  -> exit 1  (6 vulnerabilities, 4 warnings)
cargo audit after cargo update      -> exit 0
```

**The exact pins are untouched, and that is not luck — it is the reason the
yanked crate hid.** `argon2`, `chacha20poly1305`, `secrecy`, `zeroize`,
`toml_edit` and `serde-saphyr` are all pinned `=`, and all six hold their
versions across the update. Only the transitive `chacha20` moves, 0.10.1 →
0.10.2 — which is the yanked one. **An `=` pin constrains the crate named, not
its subtree**, so a frozen `chacha20poly1305 =0.11.0` never protected the
`chacha20` beneath it. Worth keeping: the pins are a deliberate policy for the
credential path, and it would be easy to read them as freezing more than they
do.

**What verified the bump, and what did not.** `cargo test` (188), clippy,
rustfmt, docgen-check and a release build all pass, but none of them exercise
a real ZIP decode or a real TLS handshake — and `zip` moved three minors
(8.3.1 → 8.6.0) on the crate that parses untrusted MaxMind input. So the real
decode was driven directly, through `extract_archive_to_temp` (which includes
`scan_zip_entries`' traversal/absolute-path/exec-bit checks and the
`MAX_EXTRACT_BYTES` cap), against the five real archives in
`/var/lib/xt_geoip`: all five decoded to 12 entries and ~45 MB each. That run
costs **no MaxMind budget and no root** — the archives are world-readable, and
this is the local half of the observation in #89 that the `build -l` cycle is
network-free. It was a throwaway probe, removed afterwards; making it a
permanent test would need a decision about depending on machine state that is
not in the repo.

**Still unverified: the live fetch and the TLS handshake.** That needs
`xtgeoip-tests --rebuild` — root, a release build, and part of the
rate-capped budget.

### The gate — built 2026-09-03, removed 2026-09-04 ❌ WITHDRAWN

The bump above was remediation and **stands**: six real advisories, cleared by
a lockfile update. What was built alongside it — `.cargo/audit.toml`, a CI
`audit` job, a weekly schedule, and `_check_advisories()` in `sync.py` — has
been removed in full at the maintainer's direction.

**The reasoning, because it is more useful than the gate was.** Dependency
updates get applied on the maintainer's terms. "Always use the latest version"
is a fallacy that carries its own baggage, and this project pins six crates
exact for the credential path precisely because a bump there is a decision
rather than a chore. A gate that blocks a commit until an advisory published
by somebody else is resolved inverts that: it hands the schedule to the
advisory feed. The weekly cron went first (2026-09-04), then the rest.

**What was actually wrong with it**, beyond the preference:

- The `sync.py` pre-flight **blocked commits** on a finding about somebody
  else's crate, which may have no fix available at all.
- The CI job failed the build for the same reason, on a lockfile that had not
  changed.
- Neither answered a question about *this* code. They answered a question
  about the world, on the world's timetable.

**What remains, and it is enough.** `cargo audit` is one command with a
meaningful exit code. Running it by hand takes five seconds and is a perfectly
good habit. Nothing now runs it automatically, nothing blocks on it, and
nothing reports it — by design.

**Two facts from the schedule argument, kept because they generalise.**

- **The test worth applying to any check**: can its answer change while the
  repository sits untouched? For `build`, `lint`, `test` and `docgen-check`,
  no — same input, same answer, so `push` covers them completely and a cron
  could only repeat what the last push said. Only the advisory check differed,
  and that difference is precisely what made it unwelcome: it answered on
  somebody else's timetable.
- ⚠ **GitHub disables scheduled workflows after 60 days of repository
  inactivity, silently.** So a cron evaporates exactly in the long-idle case
  that would motivate one, and eight weeks of green followed by silence is
  indistinguishable from all-clear. Worth knowing before anyone reaches for
  `schedule:` here for an unrelated reason. This repository's push history has
  gaps of 32 and 12 days in two months, so it is not a hypothetical.

**Do not re-propose**: the CI job, `.cargo/audit.toml`, the `sync.py`
pre-flight, a `schedule:` trigger, or Dependabot. All five were considered on
2026-09-03/04 and all five were declined.

**Contrast with the toolchain check** (TOOLCHAIN MAINTENANCE), which was kept:
it *reports* and never gates, it is throttled, and the thing it reports on —
a pin this project chose — is the maintainer's own artifact rather than an
external feed. The distinction is not "advisories bad, compilers good". It is
that one of them tells you something about a decision you made, and the other
would have made the decision for you.

---

## LOW PRIORITY / LARGE SCOPE

### #24 — pipelines: no rollback on mid-pipeline failure ✅ CLOSED (2026-07-18) — stage 1 done, stages 2–3 rejected

**Nothing actionable remains.** Stage 1 (reorder `Clean` after `Fetch`) and
the ephemeral-cleanup half both shipped; stages 2 (rollback) and 3 (atomic
swap) are rejected, the latter twice — it was implemented once as `d2bce08`
and caused data loss. Closed 2026-09-01 after a cross-check found the
heading still flagged open while every part of the body had resolved.

Design note [`98-state-ownership-recovery.md`](design/98-state-ownership-recovery.md) **REJECTED** (§0). **Stage 2 (rollback) is rejected with it** — it was `restore` under another name, and restoring a backup means adopting responsibility for a problem you have not diagnosed. **Stage 3** (atomic swap) stays rejected.

**Stage 1 ✅ DONE (2026-07-18).** `Clean` moved from `pre` to `mid` in both pipeline arms, so it runs *after* `Fetch`:

```
run -c    [Backup?, Fetch{Remote}, Clean?, PruneCsv?, Build]
build -c  [Backup?, PruneBin?, Fetch{Local}, Clean?, Build]
```

`Backup` deliberately stays in `pre` — it is the one step that must happen before anything is disturbed.

Ratified on the grounds that the primary request in `run -c` is *run*; `-c` is a modifier to it. The user already knows `run` fetches and builds over existing data, and that cleaning after building would leave an empty directory — so clean-before-build was never in question. The fetch/clean order only matters when something goes wrong, and the thing that can go wrong is exactly emptying the directory and then failing to produce a replacement.

Exactly 2 of the 11 goldens changed — the two arms containing both `Clean` and `Fetch` — which is the confirmation the change is scoped correctly. Added `clean_never_precedes_fetch`, sweeping every flag combination and asserting the invariant rather than a sequence.

**Cached-archive fallback: considered and rejected.** The initial instinct was to fall back to the cached archive on a failed remote fetch. Rejected on better reasoning: if the objective is to apply *new* data and new data is unavailable while the existing install is intact, rebuilding the *same version* over itself achieves nothing — real risk for a guaranteed no-op. The correct response to a failed fetch is error, early exit, and cleanup of ephemeral data. Note this only became true *because* of stage 1: pre-reorder, the install had already been destroyed by the time the fetch failed, which is what made a fallback look necessary. If the operator does want the cached version applied, that command already exists and is spelled `build`.

**Ephemeral cleanup ✅ DONE (2026-07-18)** — the second half of "error and early exit, cleaning up ephemeral data on the way out". `acquire_remote_archive` cleaned up its `.part` file on 2 error paths and leaked it on **6**: a dropped connection mid-copy, a failed or non-success checksum request, an unreadable or malformed checksum body, and a failed rename. Leaked files were **inert but immortal** — `find_latest_local_csv_archive` requires `.zip` so they were never mistaken for an archive, but `prune_csv_archives` matches only `.zip`/`.zip.sha256`, so they were never reclaimed either, accumulating unboundedly in `archive_dir` at up to ~5 MB per failed attempt.

Replaced both manual cleanups with a `PartialDownload` RAII guard (`Drop` removes, `disarm()` after a successful rename), so new error paths are covered by construction rather than by remembering. 3 unit tests: removes on drop, keeps when disarmed, silent when the file was never created.

⚠ **`src/fetch.rs.sig` is now BAD** — expected, since `fetch.rs` changed. Deliberately not re-signed: the signature attests to a security *audit*, not to file contents, so re-signing without a re-audit would make it assert something untrue. Needs a guardian re-audit.

`backup → clean → fetch → build` has no rollback. A failure mid-way leaves system in partially-destroyed state. Future improvement: write to temp output directory, atomic swap on success. Execution planner (#17) is the right place to manage temp directory as a pipeline-level concern.

**⚠ See #1 PRIORITY.** This exact idea was implemented early (`d2bce08`) and caused a data-loss bug: the atomic swap `remove_dir_all`s the whole `output_dir`, deleting files build never created. It has been reverted. If revisited, the temp/swap MUST respect manifest ownership — never delete unowned files, force-delete only build-created types (`.iv4`/`.iv6`).

### #38 [also build.rs] — CSV materialisation: high memory risk ❌ CLOSED — premise invalidated (2026-07-18)

Measured: peak transient is **35.7 MB**, not a risk. The "high memory" framing assumed `Vec<(String, ...)>`; the code uses a `Copy` `CountryCode` with no heap allocation. DashMap streaming rejected on invariant #5 (564k contended inserts on 2 cores would trade away working parallelism). Full measurement under **ARCHITECTURE: build.rs RESTRUCTURING → #38**.

### #54 [also fetch.rs] — parallel ZIP writes ❌ CLOSED — WONTFIX (2026-07-18)

Benchmarked: saves 1.3% of extraction (1.57 ms of 124 ms), and extraction is itself dwarfed by the network download it follows. See the full measurement under **ARCHITECTURE: fetch.rs RESTRUCTURING → #54**.

### #99 — backup.rs: gzip compression level is the backup bottleneck ✅ DONE (2026-07-18)

Found 2026-07-18 while measuring #71. `create_tarball` uses flate2's default compression (level 6), which is **96–98.5% of a backup's wall time**. Measured on the real data directory (509 files / 10.95 MB), mean of 5 runs:

| Level | Time | Output |
|-------|------|--------|
| **1** | **152 ms** | 3.88 MB |
| **6** (current) | 959 ms | 3.30 MB |
| 9 | 3.29 s | 3.32 MB |

Two findings:

- **Level 1 cuts total backup time by ~84%** (950 ms → 152 ms) for 0.58 MB more per archive — on files that `archive_prune = 3` discards anyway. That is a ~6× improvement on the operation, versus the 0.6–3.3% #71 offered.
- **Level 9 is strictly worse than level 6 here**: 3.4× slower for output that is marginally *larger* (3.32 vs 3.30 MB). The extra search finds nothing on this data. Nobody should raise it; recorded so it isn't tried.

**Full sweep (levels 0-9) changed the answer.** The 1/6/9 sample suggested a speed-vs-size trade. There isn't one — level 6 is *strictly dominated*:

| level | time | size | |
|-------|--------|----------|---|
| 0 | 197 ms | 11.38 MB | slower than L1: writing 11 MB costs more than compressing it |
| **1** | 131 ms | 3.89 MB | fastest |
| 2 | 276 ms | 3.57 MB | on frontier |
| 3 | 416 ms | 3.38 MB | dominated by L4 |
| **4** | **360 ms** | **3.27 MB** | **2.2x faster than L6 AND smaller** |
| 5 | 463 ms | 3.27 MB | dominated by L4 |
| 6 | 807 ms | 3.31 MB | *current default — dominated* |
| 7-9 | 1.2-2.6 s | 3.31 MB | pure waste |

Pareto frontier is only L1, L2, L4. Sizes are deterministic per level+input, so the size figure is not noise; zlib varies both search depth and lazy-matching strategy by level, and past 4 the extra effort buys nothing here.

**Resolved: hardcoded level 4** (`COMPRESSION_LEVEL` in `backup.rs`). Chosen because it is a strict improvement over the previous default with no trade to argue about, and it adds no config surface. Backup wall time drops ~807 ms -> ~360 ms; since compression was 96-98.5% of a backup, that is essentially the whole operation halving.

Caveat recorded at the constant: the *speed* win holds for any input, but "also smaller" is a property of this dataset and may not generalise. Even if it didn't, 2.2x faster stands.

Config key rejected: no trade left to expose once the default is on the frontier, and it would mean threading a parameter through `backup()` -> `create_tarball()` -> `write_tarball()` plus validation and spec work.

Unmeasured alternative, left open: a parallel gzip implementation could use both cores on the dominant step — larger than a level tweak, and worth measuring against L4 before assuming it wins.

**Gave `backup.rs` its first tests** (it had none): archive round-trips contents byte-exact, entries are flat (no leading paths), missing files are skipped, `create_tarball` leaves no stale `.part`, and the level constant stays inside the measured frontier. The round-trip helper decodes an archive in *test code only* — `xtgeoip` still has no restore, per `98-state-ownership-recovery.md` §0.

### #71 [also backup.rs] — parallel manifest verification ❌ CLOSED — WONTFIX (2026-07-18)

Measured: parallelises well (4.61× cold) but verification is only 1.5–4% of a backup, so it saves 0.6–3.3%; and it would make integrity-failure reporting nondeterministic. The real bottleneck is gzip → **#99**. Full measurement under **ARCHITECTURE: ANALYSIS AND SMALL REFACTORS → #71**.

### #88 — unit testing: mock the HTTP layer in fetch.rs ✅ DONE (2026-07-18)

*(Retitled 2026-07-18. Was: "unit testing: no unit tests exist ⚑ HIGH PRIORITY (next after spec-driven architecture)". The original gap is closed — 93 unit tests exist; what remains is the network path alone, so the HIGH PRIORITY flag was dropped with it.)*

**Remaining scope.** Nothing exercises `fetch()`'s network path — `resolve_version`, `check_download_size`, `acquire_remote_archive`. Everything downstream of the download is already covered from fixtures. Needs a mock HTTP server or an injected transport; #12/#18 configurability is the enabler. Note `fetch.rs` is guardian-signed, so any change to it requires a re-sign.

✅ **DONE (2026-07-18).** No injected transport was needed, and no production seam either — an earlier assessment of mine was wrong about that. `fetch()` takes its URL from `config.maxmind.url` and enforces no scheme, so pointing that at a local listener drives the **real** code path with nothing stubbed.

Mock server is hand-rolled on `std::net::TcpListener` (~120 lines in the test module): non-blocking accept with a 5 ms poll and an `AtomicBool` stop flag, so it cannot hang when the client makes fewer requests than expected; replies close the connection, so no keep-alive handling. **No dev-dependency added** — the request shapes are trivial (GET, no body) and this project is deliberately conservative about dependency surface.

Eight tests over the previously-untested path:

- `remote_fetch_sends_basic_auth` — credentials reach the endpoint as HTTP basic auth
- `non_success_status_is_reported`, `rate_limit_is_not_retried` — 429 is a *client* error so `send_with_retry` does not retry it, which is correct: hammering a rate limit is the wrong response, and MaxMind's cap is this project's real constraint
- `missing_content_disposition_is_rejected`, `hostile_content_disposition_is_rejected` — the traversal shapes the guardian audit reasoned about statically are now *executed* (`../../etc/passwd`, `/etc/shadow`, `..`, empty)
- `checksum_mismatch_leaves_no_partial_download` — end-to-end proof of the `PartialDownload` guard: fails *and* leaves no `.part`
- `credentials_are_not_forwarded_across_origin_redirect` — see #101
- `redirect_loop_is_bounded` — see #101

5xx is deliberately avoided in tests: `send_with_retry` backs off 2s/4s/8s, so a persistent server error would cost 14 s per test. 4xx returns immediately.

Note the tests live *in* `fetch.rs` — there is no `lib` target, so nothing under `tests/` can import it. That is why this still required a guardian re-sign despite the production delta being one line.

---

**Reassessment that led to the retitle (2026-07-18).** The title and the "no unit tests exist" claim were stale. As of 2026-07-18 there are **93 unit tests** running under plain `cargo test` (root-free, no network), across `action.rs`, `build.rs`, `cli.rs`, `fetch.rs`, `version.rs` and `xtgeoip-tests.rs`. They are enforced by `sync.py` and by CI's `test` job (see #96). The deliberate ordering the ticket describes — "tackle immediately after the spec-driven architecture lands" — has happened, and the work landed incrementally alongside it rather than as one push.

Delivered against the original acceptance list:

- ✅ **Sandboxed** — no sudo, no network, no interaction; all 93 run under `cargo test`.
- ✅ **CI/CD compatible** — GitHub Actions `test` job plus `sync.py`.
- ✅ **Semantics layer oracle** — `cli::snapshot` pins all 136 flag combinations byte-for-byte; `cli::contradiction` (#92) cross-checks the spec's 51 `CLI_MATRIX` examples against the parser and proves every guard reachable.
- ✅ **Fixtures over live dependencies** — `fetch.rs` tests synthesise ZIPs in-process (traversal, absolute paths, exec bits, prefix detection, extraction cap) and validate CSVs from fixtures; `version.rs` parses tokens; `build.rs` covers its helpers.
- ✅ **Execution planning** — `action.rs` goldens pin every `Action`'s step sequence.

Genuinely remaining, and smaller than the original scope implies:

- **Mock HTTP.** No test exercises the network path of `fetch()` (`resolve_version`, `check_download_size`, `acquire_remote_archive`). Everything downstream of the download is covered from fixtures. This is the one real gap; it needs a mock server or an injected transport, and it is what #12/#18 configurability would enable.
- **Setup/teardown lifecycle** — only relevant to the integration suite, which is #87/#89 territory, not unit tests.

✅ Retitled and de-flagged 2026-07-18 on that basis; the remaining scope is stated at the top of this entry.

*(Historical text below, kept for provenance.)*

**Priority raised by the user 2026-06-07.** No unit tests exist — a major gap. The
project's only automated tests are the user-owned integration suite (`xtgeoip-tests`,
root-only), which is NOT a substitute for unit coverage and is outside the dev
workflow. To be tackled immediately after the spec-driven architecture work lands
(the deliberate ordering: architecture is still in flux, so unit tests written now
would be rewritten — see the spec-driven overview).

When implemented: sandboxed (no sudo, no network, no interaction), full logging,
CI/CD compatible (GitHub Actions, distro buildsystems), virtualise all external
dependencies (CSV fixtures, mock HTTP, temp paths), setup/teardown lifecycle. All
production paths configurable via #12 and #18. The generated CLI matrix
(`CLI_MATRIX` / `testcases.yaml`) is a ready-made, root-free oracle for unit-testing
the semantics layer (see #92).
