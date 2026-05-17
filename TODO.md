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

## WIP

### Packaging and deployment

In progress. No further detail recorded here.

### CLI codegen from spec / structure-errors

In progress. No further detail recorded here.

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

The `Action` enum is explicit, type-safe, and easy to extend — keep this shape. The Action construction blocks (e.g. `Ok(Some(Action::Build { legacy, backup, ... }))`) are the right pattern; the change needed is that they should be generated from the semantics layer rather than hand-written. The individual items in this TODO are stepping stones toward this architecture; items #22, #27, #29 are the remaining structural enablers.

Note [#32]: Preserve the `Action` construction pattern — the change is in the source of the construction logic, not its shape.

---

## CONFIG AND CONF SUBCOMMAND

### #1 — messages.rs / config.rs: file logging not optional

Logging to file should be optional. Configurable via `[logging]` in TOML config, overridable with a CLI flag (flag takes precedence). When disabled, log output goes to stderr only (or is suppressed, TBD).

---

## MIGRATION

### #2 — Cargo.toml / docgen: migrate from `serde-yaml` to `serde-saphyr`

`serde-yaml` is deprecated. Migrate to `serde-saphyr` (maintained successor, compatible API). Do alongside #77 and #79 to avoid touching the YAML serialisation path twice.

---

---

## ARCHITECTURE: ANALYSIS AND SMALL REFACTORS

### #8 — all modules: re-analyse separation of concerns

Re-analyse the distribution of responsibilities across all modules (including `main.rs`) to verify clean separation of concerns. Likely problem areas: overlap between `main.rs`, `cli.rs`, and `action.rs`; config loading touching runtime concerns; logic that has drifted into the wrong layer. This is a prerequisite analysis before larger refactoring.

---

## ARCHITECTURE: build.rs RESTRUCTURING

### #38 — build.rs: CSV parsing materialises all rows before grouping

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

### #57 — fetch.rs: `fetch()` mixes version resolution, acquisition, and extraction

`fetch()` mixes version resolution, acquisition, and extraction. Split:
```
resolve_version()  → determine version (remote HEAD or local)
acquire_archive()  → download or confirm local archive valid
verify_archive()   → checksum + size + magic checks (#49, #50, #51)
extract_archive()  → unpack to temp, flatten, move into place (#54)
```
**Constraint: must not break any existing parallelism inside `fetch()`.**

### #54 — fetch.rs: ZIP entry writes are sequential

ZIP entry enumeration is sequential but file writes after decompression are independent. Decompress to buffer sequentially, then spawn parallel write tasks via Rayon. Not critical now; worthwhile if archive grows. **Benchmark before committing.**

### #71 — backup.rs: manifest verification is sequential

Consider Rayon `.par_lines()` or `.par_iter()`. On small datasets, overhead may exceed benefit. On NVMe with many files, likely a win. **Measure first.**

---

## ARCHITECTURE: action.rs / EXECUTION PLANNER

### #22 — action.rs: FetchMode semantics exist only in code

`FetchMode::Remote` and `FetchMode::Local` are a clean abstraction but their semantics exist only in code. Bring into spec:
```yaml
fetch:
  mode: remote | local
```
Depends on #17 and spec-driven direction.

### #29 — cli.rs: ambiguity checks have no formal basis

Ad hoc ambiguity checks (`if *prune && *force && *clean`, etc.) have no formal basis. "Ambiguous" is undefined. A combination is ambiguous if and only if the execution planner (#17) cannot produce a deterministic `Vec<Step>`. Remove current checks once planner exists; let inability to plan be the rejection signal.

---

## SPEC-DRIVEN ARCHITECTURE: SPECIFIC TASKS


### #92 — docgen / tests: expand spec validation and utilise CLI matrix

`proof.unique_maps_to` is now enforced by the validator. Remaining: expand validation to catch logical contradictions (declared but never used flags, undeclared mutual exclusions, unreachable valid states). Also: `pub const CLI_MATRIX: &[CliExample]` is generated but underutilised — use for fuzzing (seed corpus), property testing (`proptest`/`quickcheck`), and exhaustive parser validation.

---

## DOCGEN (xtgeoip-docgen.rs)

### #75 — docgen: `resolve_outcome` conflates resolution and presentation

`resolve_outcome` conflates template resolution, fallback logic, and user-facing output strings — a mini templating engine inside business logic. Split into semantic resolution (typed `ResolvedOutcome`, no strings) and presentation rendering (format-specific, no logic). Each generator renders a `ResolvedOutcome` independently.

### #76 — docgen: silent fallbacks mask missing spec data

Silent fallbacks like `.unwrap_or_else(|| "OK".into())` and `"ERROR".into()` let missing spec data silently become valid-looking output. Distinguish explicit defaults (optional field, spec-defined meaning) from missing required fields (hard error at spec-load time). Enforce required fields via `deny_unknown_fields` or explicit validation. A spec with missing data should not produce output. Ties into #61.

### #77 — docgen: testcase YAML output has no ordering or schema guarantees

`serde_yaml::to_string(&testcases)?` has no ordering guarantees, no schema enforcement, no versioning metadata. Improvements: stable ordering (by `case_id`), top-level `schema_version` field, post-generation round-trip validation. Do alongside #2 migration.

### #79 — docgen: BTreeMap ordering not verified for YAML deserialisation

`BTreeMap<String, CommandSpec>` gives deterministic alphabetical ordering at Rust level. Verify the YAML deserialiser preserves stable iteration order when deserialising into `BTreeMap`. Test with round-trip assertion. Do alongside #2 migration.

---

## TEST INFRASTRUCTURE (xtgeoip-tests.rs)

### #87 — tests: system integration nature not documented

Explicitly document that `xtgeoip-tests` is a system integration test suite (not unit tests): tests are order-dependent, require root, require a real release build, and depend on prior test execution. Add to comments and `--help`. Consider a setup/teardown phase for known-good initial state.

### #81 — tests: binary path hardcoded

`format!("target/release/{}", program)` hardcodes release build path. Two options: (1) `env!("CARGO_BIN_EXE_xtgeoip")` if restructured to Cargo integration tests, (2) accept `--bin <path>` flag or `XTGEOIP_BIN` env var. Option 2 is the simpler near-term fix.

### #82 — tests: rebuild condition uses weak string scraping

Rebuild condition `cmd_args.contains(&"-c") && cmd_args.first().map(|a| a.starts_with('-'))...` is logically weak. Intent is "after a test that cleans, rebuild for subsequent tests." Express in spec:
```yaml
case_id: TL-007
rebuild: true
```
Test runner reads `tc.rebuild` directly — no string-scraping.

### #83 — tests: no timeout on test commands

No timeout. One hanging command freezes the entire suite. Add per-test timeout (`DEFAULT_TEST_TIMEOUT: Duration = Duration::from_secs(60)`). On timeout: kill child, mark TIMED OUT, continue. Allow `timeout_secs` override in testcase YAML.

### #80 — tests: command parsing breaks on spaces in args

`tc.cmd.split_whitespace()` breaks on quoted args, paths with spaces. Preferred fix: store commands as structured YAML arrays (`cmd: [xtgeoip, build, ...]`) — no parsing ambiguity, more machine-readable. Avoids new crate dependency.

### #89 — tests: orphaned file scenarios not covered

Orphaned files from legacy/default mode switching are not covered by the rebuild logic. Add two explicit test scenarios:

**Scenario A (orphan detection)**: produce orphans → do not clean → run detection command → assert orphans identified.

**Scenario B (orphan cleanup)**: produce orphans → clean → run same detection → assert no orphans. Requires `requires:` dependencies and `rebuild:` annotations in YAML. Further analysis needed to establish if all state transitions are covered.

### #90 + #84 — tests: add output assertions to testcase YAML [merged]

Exit code only tests success vs failure — a smoke test, not spec compliance. A build that exits 0 but does nothing still passes. Add optional output assertions to testcase YAML:
```yaml
case_id: B-001
key: p
cmd: xtgeoip build
expected_stdout: "build"   # substring or regex
expected_stderr: ""        # must be empty
```
In runner, capture both streams and assert. Support: exact match, substring match, regex match. Omitted field = not checked (backwards compatible). This is the concrete mechanism that #85+91, #86 all depend on.

### #85 + #91 — tests / cli: `maps_to` never verified [merged]

`maps_to` appears in test output on failure but is never verified. Two changes needed:

**In the CLI (#91)**: emit machine-readable error code prefix on stderr:
```
Error [build_prune_no_backup]: you must specify --backup, for the --prune option
```
The bracket token is the `maps_to` key (error case identifier).

**In the test runner (#85)**: when `maps_to` is set, parse stderr for the bracket token:
```rust
if let Some(expected_code) = &tc.maps_to {
    // assert stderr contains format!("[{}]", expected_code)
}
```
Turns `maps_to` from documentation annotation into a live assertion. At minimum (before #90 is implemented), validate that `maps_to` values name real spec keys. Depends on #90+84.

### #86 — tests: `key: p/f` is too coarse

`key: p/f` is too coarse. Candidates in increasing scope: expected exit codes (`key: f2` = exit exactly 2), error class (`error_class: cli`), reason template match (`maps_to: build_prune_no_backup` asserts that template was triggered). `key` should evolve into a structured expectation. Depends on #90+84 and #85+91.

---

## LOW PRIORITY / LARGE SCOPE

### #24 — pipelines: no rollback on mid-pipeline failure

`backup → clean → fetch → build` has no rollback. A failure mid-way leaves system in partially-destroyed state. Future improvement: write to temp output directory, atomic swap on success. Execution planner (#17) is the right place to manage temp directory as a pipeline-level concern.

### #38 [also build.rs] — CSV materialisation: high memory risk

High memory risk from CSV materialisation; benchmark before implementing the streaming approach.

### #54 [also fetch.rs] — parallel ZIP writes

Parallel ZIP writes; benchmark before committing.

### #71 [also backup.rs] — parallel manifest verification

Parallel manifest verification; measure before committing.

### #88 — unit testing: no unit tests exist

No unit tests exist — intentional at this stage. When implemented: sandboxed (no sudo, no network, no interaction), full logging, CI/CD compatible (GitHub Actions, distro buildsystems), virtualise all external dependencies (CSV fixtures, mock HTTP, temp paths), setup/teardown lifecycle. All production paths configurable via #12 and #18. Large undertaking — schedule separately after architecture refactoring (#3–#34) stabilises.
