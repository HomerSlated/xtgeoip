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

The `Action` enum is explicit, type-safe, and easy to extend — keep this shape. The Action construction blocks (e.g. `Ok(Some(Action::Build { legacy, backup, ... }))`) are the right pattern; the change needed is that they should be generated from the semantics layer rather than hand-written. The individual items in this TODO are stepping stones toward this architecture; items #22, #27, #29, #93 are the remaining structural enablers.

Note [#32]: Preserve the `Action` construction pattern — the change is in the source of the construction logic, not its shape.

---

## CONFIG AND CONF SUBCOMMAND

### #1 — messages.rs / config.rs: file logging not optional ✅ CORE DONE

**Root cause found:** terminal output was welded to file logging. `init_logger` built
the stdout/stderr *and* file dispatches together and was only called when `[logging]`
provided a log-file path — so with no `[logging]` section, no logger was installed at
all, and the `log` facade silently no-op'd *every* message (not just file output).

Fixed: `init_logger` now always installs stdout+stderr; the file sink is added only
when a path is configured. `main` calls `init_logger(cfg.logging…map(log_file))`
unconditionally (and `init_logger(None)` on the `conf` path). Resolves the "TBD":
when file logging is disabled, output still goes to stdout/stderr. Done with #94.

**Remaining (smaller follow-up):** CLI flag to override `[logging]` (flag takes
precedence). Not yet implemented.

---

## MIGRATION

### #2 — Cargo.toml / docgen: migrate from `serde-yaml` to `serde-saphyr`

`serde-yaml` is deprecated. Migrate to `serde-saphyr` (maintained successor, compatible API). Do alongside #77 and #79 to avoid touching the YAML serialisation path twice.

---

---

## ARCHITECTURE: ANALYSIS AND SMALL REFACTORS

### #93 — config.rs: split into config.rs (data/load) and conf.rs (command handler)

`config.rs` currently hosts three distinct concerns: (1) `Config` struct + `load_config()` — pure data/loading, (2) `ConfAction` enum — a CLI-originated user choice, not a config concept, and (3) `run_conf()` + precondition checks + interactive prompting (`prompt_create_config()`). Split into:

- `config.rs` — `Config` struct, `load_config()`, `validate()`; no output, no subprocesses, no prompts
- `conf.rs` — `ConfAction`, `run_conf()`, precondition checks, `prompt_create_config()`

This removes the backwards dependency `cli.rs → config.rs` for a CLI type. After the split, `cli.rs` only imports action-layer types. **Prerequisite for spec-driven architecture (#9/#26/#27/#34).**

### #94 — backup.rs / fetch.rs: remove double-error reporting ✅ DONE

**Original premise was stale and the fix inverted it.** The double-print the entry
described only existed when `main` did `eprintln!("Error: {e}")`; that print was
removed in commit `926a335`, after which the `error()`+`bail!()` pairs were *not*
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

### #89 — tests: orphaned file scenarios not covered

Orphaned files from legacy/default mode switching are not covered by the rebuild logic. Add two explicit test scenarios:

**Scenario A (orphan detection)**: produce orphans → do not clean → run detection command → assert orphans identified.

**Scenario B (orphan cleanup)**: produce orphans → clean → run same detection → assert no orphans. Requires `requires:` dependencies and `rebuild:` annotations in YAML. Further analysis needed to establish if all state transitions are covered.

---

## TOOLING / AGENTS

### #95 — import generic agents from private/agents-out/

The seven project-agnostic agent role descriptions in `private/agents-out/` (bug-hunter, data-flow-tracer, deep-research-collector, docs-auditor, flow-doc-generator, optimisation-advisor, guardian-security) are to be imported as actual project agents, adapting each by filling its `[bracketed]` placeholders for xtgeoip (`[language]` = Rust, `[source-dir]` = `src/`, `[output-dir]` under `private/`, etc.).

Priority / notes:
- **docs-auditor** first — `private/WORKFLOW.md` already delegates its documentation-check step to this agent. Audit set: `README.md`, `CLAUDE.md`, `TODO.md` / `TODO_tldr.md`, `docs/design.md`, `docs/legacy.md`. Mark `docs/generated/` and `src/generated/` as docgen-owned (off-limits — change `docs/spec/cli.yaml`, not the output).
- **guardian-security** — GPG key already provisioned (ed25519, fpr `227E5FE6EB2D3E9EE5883CB1F4BF35E6DC8029B0`; public key `docs/guardian_public.asc`; keyring `private/guardian/gnupg/`; setup script `private/guardian/guardian-security-pre.sh`). Set `[signable-dirs]` to the tracked source dirs (note: anything under `private/` is gitignored, so per-file `.sig` signatures only make sense for files outside it).
- Remaining (bug-hunter, optimisation-advisor, data-flow-tracer, flow-doc-generator, deep-research-collector): adapt as needed when wanted.

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
