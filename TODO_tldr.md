# TODO — TL;DR

## INVARIANTS

- All changes assessed in precedence order: hard errors → soft errors → unsafety → security → parallelism → method consistency → style → everything else
- A higher-priority constraint blocks a lower-priority benefit, always

---

## WIP

- Packaging and deployment — in progress
- CLI codegen from spec / structure-errors — in progress

---

## OVERVIEW: Spec-Driven Architecture [#9, #26, #27, #34]

- Three sources of truth (clap struct, `normalize_cli_to_action`, `cli.yaml`) will drift; target: one data-driven semantics layer between CLI and Action
- `Action` enum and construction blocks are the right shape — keep them; derive their content from the spec, not hand-written control flow
- Remaining structural enablers: #22, #27, #29
- [#32] Preserve `Action` construction pattern; change the source, not the shape

---

## CONFIG AND CONF SUBCOMMAND

- [#1] messages.rs/config.rs: make file logging optional via TOML config and CLI flag override

---

## MIGRATION

- [#2] Cargo.toml/docgen: replace deprecated `serde-yaml` with `serde-saphyr`; do alongside #77 and #79

---

## ARCHITECTURE: ANALYSIS AND SMALL REFACTORS

- [#8] all modules: audit separation of concerns before larger refactoring; prerequisite step

---

## ARCHITECTURE: build.rs RESTRUCTURING

- [#38] build.rs: stream CSV rows into `DashMap` grouping instead of materialising all rows first; check invariant #5

---

## ARCHITECTURE: fetch.rs RESTRUCTURING

- [#57] fetch.rs: split `fetch()` into `resolve_version` / `acquire_archive` / `verify_archive` / `extract_archive`; must not break existing parallelism
- [#54] fetch.rs: parallel ZIP entry writes via Rayon after sequential decompression; benchmark first
- [#71] backup.rs: parallel manifest verification via Rayon; measure overhead vs benefit first

---

## ARCHITECTURE: action.rs / EXECUTION PLANNER

- [#22] action.rs: bring `FetchMode::Remote | Local` into spec YAML; depends on spec-driven direction
- [#29] cli.rs: remove ad hoc ambiguity checks; let planner inability to produce a `Vec<Step>` be the rejection signal

---

## SPEC-DRIVEN ARCHITECTURE: SPECIFIC TASKS

- [#92] docgen/tests: `unique_maps_to` now enforced; remaining: catch logical contradictions; use `CLI_MATRIX` for fuzzing and property tests

---

## DOCGEN (xtgeoip-docgen.rs)

- [#75] docgen: split `resolve_outcome` into typed semantic resolution and format-specific rendering
- [#76] docgen: replace silent fallbacks (`unwrap_or("OK")`) with hard errors for missing required spec fields; ties into #61
- [#77] docgen: add stable ordering, `schema_version`, and round-trip validation to testcase YAML output; do with #2
- [#79] docgen: verify `BTreeMap` iteration order preserved by YAML deserialiser; add round-trip assertion; do with #2

---

## TEST INFRASTRUCTURE (xtgeoip-tests.rs)

- [#87] tests: document integration test nature (root required, order-dependent, release build); add setup/teardown phase
- [#81] tests: replace hardcoded `target/release/` path with `--bin` flag or `XTGEOIP_BIN` env var
- [#82] tests: replace string-scraped rebuild condition with `rebuild: true` field in testcase YAML
- [#83] tests: add per-test timeout (60s default); kill on timeout, mark TIMED OUT, continue suite
- [#80] tests: replace `split_whitespace` command parsing with structured YAML arrays (`cmd: [...]`)
- [#89] tests: add Scenario A (orphan detection) and Scenario B (orphan cleanup) with `requires:` and `rebuild:` annotations
- [#90+84] tests: add `expected_stdout` / `expected_stderr` fields to testcase YAML; capture both streams in runner; foundation for #85+91 and #86
- [#85+91] tests/cli: emit `Error [reason_template_key]: ...` on stderr from CLI; assert bracket token in runner when `maps_to` is set; depends on #90+84
- [#86] tests: evolve `key: p/f` into structured expectation (exit code, error class, `maps_to` assertion); depends on #90+84 and #85+91

---

## LOW PRIORITY / LARGE SCOPE

- [#24] pipelines: no rollback on mid-pipeline failure; address via execution planner managing temp dir
- [#38] build.rs: CSV streaming — benchmark memory savings before implementing
- [#54] fetch.rs: parallel ZIP writes — benchmark before committing
- [#71] backup.rs: parallel manifest verification — measure before committing
- [#88] unit tests: large undertaking; defer until architecture refactoring stabilises; requires sandboxing, mock HTTP, CSV fixtures, setup/teardown
