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
- **[#92 remainder]** docgen: spec validator on the *generation* side — catch contradictions at codegen time, not just test time. Test-time checks (`cli::contradiction`, 4 tests) landed 2026-07-18
- **[#98 residual]** tests: precondition checks that fail fast rather than grinding to a confusing failure. The documentation half is **done** (2026-09-01: man-page `FILE OWNERSHIP` section + the `build -c` vs `build -c -f` timing distinction in LEGACY MODE and `--help`). The `restore`-based plan is **rejected** — see below
- **[#100]** fetch.rs: shared `.part` path lets concurrent fetches collide. LOW (CVSS 3.3) and **fails closed** — SHA-256 rejects any corruption. Costs a `fetch.rs` guardian re-audit; concurrent fetches are not an expected usage pattern

---

## ARCHITECTURE (large, and currently undefined)

Spec-Driven Architecture [#9, #26, #27, #34] — collapse the three sources of
CLI truth (clap struct, `normalize_cli_to_action`, `cli.yaml`) into one
data-driven semantics layer. `Action`'s shape is right; generate its
construction from the spec rather than hand-writing it.

**⚑ Blocked on an undefined ticket.** Of the four named enablers, #22, #29
and #93 are all closed — leaving **#27**, which has no entry in `TODO.md`.
Nine numbers are cited as dependencies with no entry at all: **#9, #12,
#17, #18, #26, #27, #32, #34, #61**. Scoping #27 is the next real step here.

---

## DECIDED — do not re-propose

- **`restore` primitive: REJECTED.** Backups are context-free; restores are not. Restoring means adopting responsibility for a problem you have not diagnosed. `docs/design/98-state-ownership-recovery.md` §0. General test: **if an operation is only correct given knowledge of *why* it is being performed, it does not belong in this tool**
- **Rollback and atomic swap (#24 stages 2–3): REJECTED.** Stage 3 was implemented once (`b4ec1db`) and caused data loss
- **Cached-archive fallback on failed fetch: REJECTED.** Rebuilding the same version over an intact install is a guaranteed no-op with real risk. `build` already spells that request
- **Unattended cron: removed by design (#103).** Do not restore it by stashing the passphrase anywhere
- **Fuzzing/proptest for CLI semantics: dropped.** 136 total combinations; `cli::snapshot` already enumerates all of them exhaustively

---

## HOUSEKEEPING

- Guardian coverage is thin: only `fetch.rs` and `secrets.rs` are signed. `config.rs` and `conf.rs` are unsigned and changed substantially in #103/#104 — the credential-handling path
- `tests/` is an empty directory; there is no `lib` target, so nothing can live there

---

## RECENTLY CLOSED

2026-07-18/19 — #2, #22, #24, #29, #38, #54, #57, #71, #75, #76, #77, #79,
#81, #87, #88, #92 (test-time part), #93, #94, #95, #96, #97, #99, #101,
#102. 2026-07-20 — #103 (`c2be6a3`). 2026-09-01 — #104 (`b804fa2`), #89
(closed unimplemented — guarded by a structural invariant already, see
`TODO.md`), #98 documentation half.

Several were closed as **premise-invalidated** after checking against
source: #38, #54, #88 and #96 described code that no longer existed.
