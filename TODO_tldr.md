# TODO — TL;DR

Open work only. Full history, reasoning, and closed entries are in
[`TODO.md`](TODO.md). Rewritten 2026-09-01 after an audit found this file
listing thirteen already-closed items as open.

## INVARIANTS

- All changes assessed in precedence order: hard errors → soft errors → unsafety → security → parallelism → method consistency → style → everything else
- A higher-priority constraint blocks a lower-priority benefit, always

---

## WIP

- Packaging and deployment — early; staging exists (`conf/etc`, `conf/usr`, `extra/dkms`, `extra/ufw`), no `debian/` or spec file yet
- CLI codegen from spec (`xtgeoip-docgen`) — in progress

---

## OPEN

- **[#1 residual]** messages.rs/config.rs: CLI flag to override `[logging]` (flag takes precedence). Core of #1 is done — small, self-contained
- **[#92 remainder]** docgen: spec validator on the *generation* side — catch contradictions at codegen time, not just test time. Test-time checks (`cli::contradiction`, 4 tests) landed 2026-07-18. **Concrete case now on file:** `cli.yaml`'s `outcome:` strings are unchecked free text that ships into the man page; three had been wrong since 2026-07-18 (fixed 2026-09-02). Asserting them against `action.rs`'s `steps()` helper closes the class
- **[#98 residual]** tests: the setup/teardown lifecycle — a known-good initial state, and a teardown that survives a mid-run failure. Two of three halves are now done: documentation (2026-09-01, man-page `FILE OWNERSHIP` + the `build -c` vs `build -c -f` timing distinction) and **fail-fast preconditions** (2026-09-02: `HELP`'s REQUIREMENTS were enforced by nothing; now checked before the first case, all faults reported at once). The `restore`-based plan is **rejected** — see below

---

## ARCHITECTURE (large; described, not scoped)

Spec-Driven Architecture [#9, #26, #27, #34] — collapse the three sources of
CLI truth (clap struct, `normalize_cli_to_action`, `cli.yaml`) into one
data-driven semantics layer. `Action`'s shape is right; generate its
construction from the spec rather than hand-writing it.

**Half of it has landed.** *Validity* is spec-driven: `cli.yaml` `guards:` →
`src/generated/cli_rules.rs` → `first_guard(flags, …)`. *Ordering* is not:
`action.rs::plan()` is hand-written and the spec says nothing about steps or
`FetchMode`.

**Not blocked — undecided.** #27 was never a ticket. It is the orphaned half
of `#27/#31`, trimmed at `2baa194` when #31 landed (full trace in
`TODO.md`); #26 is in the same state. The remaining work is fully described
in the `TODO.md` OVERVIEW and the two design notes, so the next step is a
decision, not an investigation.

**If it is ever taken up** (2026-09-02 finding): all 76 `Action` values yield
plans that are subsequences of one fixed order — Backup → PruneBin → Fetch →
Clean → PruneCsv → Build — with a single data dependency (Fetch → Build). So
it needs a rank per step, not a dependency graph. Caveats: that order is an
observation about today's six steps, not an invariant; and docgen must emit
*Rust constructing `Plan`*, not a flat step list, or the type-enforced
Fetch-before-Build guarantee degrades to a runtime check.

Worth doing regardless of that decision: assert `outcome:` against `plan()`
(the #92 remainder), and make the canonical-order enumeration a permanent
test.

---

## DECIDED — do not re-propose

- **`restore` primitive: REJECTED.** Backups are context-free; restores are not. Restoring means adopting responsibility for a problem you have not diagnosed. `docs/design/98-state-ownership-recovery.md` §0. General test: **if an operation is only correct given knowledge of *why* it is being performed, it does not belong in this tool**
- **Rollback and atomic swap (#24 stages 2–3): REJECTED.** Stage 3 was implemented once (`b4ec1db`) and caused data loss
- **Cached-archive fallback on failed fetch: REJECTED.** Rebuilding the same version over an intact install is a guaranteed no-op with real risk. `build` already spells that request
- **Unattended cron: removed by design (#103).** Do not restore it by stashing the passphrase anywhere
- **Fuzzing/proptest for CLI semantics: dropped.** 136 total combinations; `cli::snapshot` already enumerates all of them exhaustively
- **Both toolchains are pinned (2026-09-02), and `sync.py` refuses to run unless the local ones match.** Stable in `rust-toolchain.toml`; the rustfmt nightly, by date, in `rustfmt-toolchain` — CI and `sync.py` both read that file. Do not reintroduce `dtolnay/rust-toolchain@stable`, `@nightly`, `cargo +stable` or `cargo +nightly`: all four float and reopen the drift. Bump deliberately and read the new lints then
- **Do not add `rustfmt` to the stable toolchain.** A stable rustfmt discards all five nightly-only options in `rustfmt.toml` — including `ignore` — and so rewrites `src/generated/`, failing docgen-check rather than the lint job. There is no stable escape: file-level `#![rustfmt::skip]` does not compile (E0658). Measured: stable and nightly agree on every hand-written file, and differ only on generated ones

---

## HOUSEKEEPING

- **Nothing keeps Rust current on the dev machine.** `/usr/bin/{cargo,rustc}` are symlinks to `/usr/bin/rustup` (Ubuntu's apt `rustup` package); the toolchains live in `~/.rustup` and move *only* when `rustup update` is run by hand. No timer, no cron, no auto-update setting. That is why `stable` sat at 1.94.0 for six months. `sync.py` now catches the divergence from the pin, but nothing yet reports that the pin itself, `rustup` (1.26.0 vs 1.29.1 upstream), the 153 pending crate updates, or dependency advisories have gone stale — see the maintenance question in `TODO.md`
- Guardian coverage is thin: only `fetch.rs` and `secrets.rs` are signed. `config.rs` and `conf.rs` are unsigned and changed substantially in #103/#104 — the credential-handling path. **`fetch.rs`'s signature is now stale too** (#100, 2026-09-02): the `.sig` was left in place so the next pre-flight raises it, and a row is in `private/guardian/needs_reverification.md`
- `tests/` is an empty directory; there is no `lib` target, so nothing can live there
- Man-page prose in `docs/spec/manpage-template.toml` is hand-written and unchecked against the code. Three drifts found on 2026-09-02 (step ordering; the whole `conf -c` credential workflow missing since #103; a `[maxmind]` `timeout` key that does not exist and that `deny_unknown_fields` would reject). Nothing prevents a fourth
- `logging.verbose` is in the shipped `xtgeoip.conf.example` but **no code reads it**. `Logging` has no such field and no `deny_unknown_fields`, so it is silently ignored. Either a dead key to delete or an unimplemented feature — likely related to the #1 residual

---

## RECENTLY CLOSED

2026-07-18/19 — #2, #22, #24, #29, #38, #54, #57, #71, #75, #76, #77, #79,
#81, #87, #88, #92 (test-time part), #93, #94, #95, #96, #97, #99, #101,
#102. 2026-07-20 — #103 (`c2be6a3`). 2026-09-01 — #104 (`b804fa2`), #89
(closed unimplemented — guarded by a structural invariant already, see
`TODO.md`), #98 documentation half. 2026-09-02 — man-page corrections
(step ordering vs `plan()`; `conf -c` / encrypted-credential workflow, a
#103 documentation residual; the config-section list), the `#27` trace, and
CI unblocked after 30 red runs (`clippy::byte_char_slices` under a stable
four releases newer than the local one; toolchain now pinned).

Several were closed as **premise-invalidated** after checking against
source: #38, #54, #88 and #96 described code that no longer existed.
