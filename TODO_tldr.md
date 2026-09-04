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

- **[#92]** ✅ DONE — docgen: spec validator on the *generation* side — catch contradictions at codegen time, not just test time. Test-time checks (`cli::contradiction`, 4 tests) landed 2026-07-18. The motivating case is **closed** (2026-09-02): examples carry a `steps:` list, checked against the real `plan()` by `action::tests::spec_steps_agree_with_plan`, which also rejects a plan-bearing example that omits the field. **Closed 2026-09-02**: `validate_plan()` runs before any output is written and rejects duplicate ranks, a context that builds without fetching, dead steps, unknown flags, empty `why:`, and undeclared step names. The boundary it settled: docgen links the library built from the *previously generated* sources, so spec-vs-program checks are inherently one generation behind and must stay at test time — **generation time owns spec-internal contradictions, test time owns spec-versus-program agreement**
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

**Stages 1-3 landed 2026-09-02** — design:
`docs/design/26-spec-derived-planning.md`. `cli.yaml` now carries a `plan:`
section (rank per step, membership per context, mandatory `why:`); docgen emits
`src/generated/plan.rs` with `plan_generated()`; and
`generated_planner_matches_the_hand_written_one` proves the two agree exactly
across all 76 `Action` values, *including* step parameters. Teeth verified:
moving `clean`'s rank before `fetch` fails 32 of 76 with the exact diff.

**Stage 4 done 2026-09-02** — `action::plan()` is deleted; the generated
planner drives execution. A perturbed `rank:` is now caught by five independent
tests (two goldens, `clean_never_precedes_fetch`, the canonical-order
invariant, and `spec_steps_agree_with_plan`). **The spec-driven arc is
complete: validity and ordering both derive from `cli.yaml`.**

**Background** (2026-09-02 finding): all 76 `Action` values yield
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
- **No automated dependency-advisory tooling. Removed entirely 2026-09-04** — the CI job, `.cargo/audit.toml` and the `sync.py` pre-flight. Dependabot rejected the same day. Updates are applied on the maintainer's terms: "always use the latest version" is a fallacy, and six crates are pinned exact for the credential path where a bump is a decision, not a chore. `cargo audit` remains a perfectly good command to run by hand; what was rejected is anything that runs it *for* you and blocks on the answer. Do not re-propose the job, the config, the pre-flight, a schedule, or Dependabot
- **Do not add `rustfmt` to the stable toolchain.** A stable rustfmt discards all five nightly-only options in `rustfmt.toml` — including `ignore` — and so rewrites `src/generated/`, failing docgen-check rather than the lint job. There is no stable escape: file-level `#![rustfmt::skip]` does not compile (E0658). Measured: stable and nightly agree on every hand-written file, and differ only on generated ones

---

## HOUSEKEEPING

- ✅ **Pin staleness is reported as of 2026-09-04.** `rustup check` is the whole check — local, half a second, installs nothing — and it is wired into `sync.py` as a **report, never a gate**, throttled to weekly. Pinning (2026-09-02) fixed CI/local divergence and created a second problem: nothing said the *pin itself* had aged, and a pin that is never revisited is the old stale toolchain with better paperwork. Bumping stays deliberate: read the new lints, because `-D warnings` turns a fresh style lint into a hard CI error — which is exactly what `clippy::byte_char_slices` did over 30 red runs. Documented in `rust-toolchain.toml` and `CLAUDE.md`, so it survives `sync.py` being gitignored
- ⚠ **The pin protects only this repo.** rustup's *default* toolchain is still `stable` at 1.94.0, so outside `xtgeoip` a bare `cargo` on this machine is the six-month-stale compiler that started the whole episode. Every other Rust project here is on it. Not fixed — changing a machine-wide default is the maintainer's call, recorded in `private/OUTSTANDING.md`
- **Dependency advisories went unnoticed for four and a half months (2026-09-03).** The predicted staleness turned out to be a live exposure: `cargo audit` reported **six vulnerabilities**, the oldest published 2026-04-14. Cleared by `cargo update` — lockfile only, no `Cargo.toml` change, and that fix stands. The gate built alongside it was removed on 2026-09-04 (see DECIDED); the *finding* was real, the automation around it was not wanted. Detail in `TODO.md`
- Guardian coverage is thin: only `fetch.rs` and `secrets.rs` are signed. `config.rs` and `conf.rs` are unsigned and changed substantially in #103/#104 — the credential-handling path. **`fetch.rs`'s signature is now stale too** (#100, 2026-09-02): the `.sig` was left in place so the next pre-flight raises it, and a row is in `private/guardian/needs_reverification.md`
- **`src/lib.rs` exists as of 2026-09-02.** A plain `rlib` — compile-time only, statically linked, nothing new deployed (`ldd` unchanged, zero dynamic references). It unblocks the three things that kept hitting the missing target: docgen can now reach the program's own semantics (#92), `tests/` can hold external tests, and `is_root()` need not be duplicated. Step 1 required **no visibility changes at all** — the existing `pub` markings sufficed
- **Tests are out of the guardian-signed files (2026-09-02).** `fetch.rs`'s 39 and `secrets.rs`'s 9 unit tests now live in `src/fetch/tests.rs` and `src/secrets/tests.rs`. A *child module* sees its parent's private items, so this needed no `pub` at all — the alternative, external `tests/` crates, would have forced 7 private items in `fetch.rs` public, including `redirect_policy`, which two guardian findings (#101, #102) exist about. Public API is unchanged in both files (2 and 3 items, before and after). One-time cost: `secrets.rs.sig` was valid and is now stale
- ✅ **Man-page prose is checked as of 2026-09-03.** Five test-time checks: EXECUTION ORDER's four documented orderings against the real planner, plus both directions of the CONFIGURATION key set, the documented defaults against the shipped example, and the `[maxmind]` strictness claim against the parser. Teeth verified by reinstating each historical defect. A fourth defect surfaced in the process — `[logging] log_file` documented no default — and was fixed. Detail in `TODO.md`; the generation-time half (template vs `cli.yaml` command/flag names) is deliberately not done
- `[logging]` and `[paths]` accept unknown keys; `[maxmind]` does not (`deny_unknown_fields`). That asymmetry is why `logging.verbose` sat in the shipped example unread. Tightening it would reject configs copied from that example, so it needs a migration story rather than a one-line attribute

---

## RECENTLY CLOSED

2026-07-18/19 — #2, #22, #24, #29, #38, #54, #57, #71, #75, #76, #77, #79,
#81, #87, #88, #92 (test-time part), #93, #94, #95, #96, #97, #99, #101,
#102. 2026-07-20 — #103 (`c2be6a3`). 2026-09-01 — #104 (`b804fa2`), #89
(closed unimplemented — guarded by a structural invariant already, see
`TODO.md`), #98 documentation half. 2026-09-02 — #1 (residual: `--log-file`/`--no-log`), #100, #98 preconditions, #92's motivating case; man-page corrections
(step ordering vs `plan()`; `conf -c` / encrypted-credential workflow, a
#103 documentation residual; the config-section list), the `#27` trace, and
CI unblocked after 30 red runs (`clippy::byte_char_slices` under a stable
four releases newer than the local one; toolchain now pinned).

Several were closed as **premise-invalidated** after checking against
source: #38, #54, #88 and #96 described code that no longer existed.
