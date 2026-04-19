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
- Key enablers: #5, #17, #19, #20, #22, #27/#31, #28, #29
- [#32] Preserve `Action` construction pattern; change the source, not the shape

---

## IMMEDIATE FIXES: SILENT FAILURES AND ERROR CONTEXT

- [#35] build.rs: collect write errors with context instead of `write_errors += 1`
- [#63] backup.rs: collect delete errors with context instead of `.is_err()` filter
- [#66] build.rs: log orphan file list write failure; always emit full list to stdout and log
- [#13] config.rs: wrap `fs::copy` failure with `.with_context`; pre-check source existence
- [#14] config.rs: check editor exit status; handle unset/empty `$EDITOR` before spawn
- [#72] backup.rs/build.rs: include file count in all bulk-delete log messages

---

## IMMEDIATE FIXES: CORRECTNESS

- [#36] build.rs: replace O(n²) `contains` in orphan filter with `HashSet`
- [#37] build.rs/fetch.rs: deduplicate `unsafe { Mmap::map }` into one `mmap_file` helper
- [#60] backup.rs: enforce double-space separator in manifest parser; reject single-space
- [#4] main.rs: surface Rayon pool init failures via `OnceLock` instead of `.ok()`
- [#7] main.rs: define `EXIT_RUNTIME_ERROR`/`EXIT_CLI_ERROR` constants; consider typed error taxonomy
- [#33] cli.rs: replace `Result<Option<Action>>` with `Result<CliOutcome>` where `CliOutcome` is `Action | ShowHelp`
- [#59] backup.rs: replace fragile `glob()` with `fs::read_dir` + explicit predicate; drop `glob` crate if unused

---

## ATOMICITY: WRITE-TO-TEMP PATTERN [#48, #64, #65]

- [#48] fetch.rs: download to `.zip.part`, rename atomically on success
- [#64] backup.rs: write backup to `.tar.gz.part`, rename atomically on success
- [#65] backup.rs: deduplicate file list with `sort(); dedup()` before tar construction

---

## FETCH RESILIENCE

- [#47] fetch.rs: add 300s timeout to `Client::builder()`; consider making it configurable
- [#46] fetch.rs: audit `Content-Disposition` parsing chain for panic/silent-None paths
- [#49] fetch.rs: re-verify cached archive checksum before reusing; warn and re-download on mismatch
- [#55] fetch.rs: add exponential backoff retry (3 retries, 2s base); transient errors only
- [#50+56] fetch.rs: enforce `MAX_DOWNLOAD_BYTES` with explicit failure on breach; check `Content-Length` against historical size with ±50% tolerance before download

---

## CONFIG AND CONF SUBCOMMAND

- [#12] config.rs: centralise `SYSTEM_CONFIG` path into one `system_config_path()` function
- [#11] config.rs: add `cfg.validate()` after `toml::from_str` for semantic invariants
- [#10] config.rs: guard interactive prompts behind `IsTerminal` check; print actionable message in non-TTY mode
- [#16] config.rs: split `ensure_system_config_exists` into `config_exists`, `create_default_config`, `prompt_create_config`; depends on #10
- [#23] main.rs/action.rs: handle `Action::Conf` in one place only (prefer `main.rs`, before `run_action`)
- [#6] main.rs: check `requires_root()` upfront before execution; avoid post-hoc `downcast_ref`
- [#15] config.rs: give each `ConfAction` variant explicit preconditions and error type; depends on #7
- [#1] messages.rs/config.rs: make file logging optional via TOML config and CLI flag override

---

## LOGGING AND STEP VISIBILITY

- [#25] action.rs: add at least one `messages::info` log call per dispatch point / Step

---

## SECURITY HARDENING

- [#61] global: parse-then-validate everywhere; never assume parsing implies validity; audit all crates
- [#51] fetch.rs: validate ZIP magic bytes, reject path traversal/absolute paths/executables before extraction; check CSV headers and field ranges after extraction
- [#52] fetch.rs/backup.rs: centralise archive name parsing into `parse_archive_name`; treat version token as opaque; warn if format unexpected but don't reject
- [#53] fetch.rs: detect common top-level prefix dynamically instead of assuming single directory; warn on unexpected shape

---

## MIGRATION

- [#2] Cargo.toml/docgen: replace deprecated `serde-yaml` with `serde-saphyr`; do alongside #77 and #79

---

## TYPED ENUMS: ELIMINATING BOOLEAN TRAPS

- [#21] action.rs/build.rs: replace `prune_archives(cfg, bool, bool)` with `PruneMode` enum
- [#67] backup.rs: replace `backup(..., force: bool)` with `BackupMode { Verified, Force }`
- [#68] backup.rs: merge `gather_files_force` / `gather_files_verified` into one `gather_files(mode)`; depends on #67
- [#39] build.rs: replace `String` country codes with `[u8; 2]` or `enum CountryCode { Iso, O1, A1, A2 }`
  - [#40] O1 fallback logic centralised by `CountryCode::O1`; no more duplication
  - [#44] `"O1".to_string()` allocations eliminated by enum variants

---

## ARCHITECTURE: ANALYSIS AND SMALL REFACTORS

- [#8] all modules: audit separation of concerns before larger refactoring; prerequisite step
- [#3] main.rs: remove redundant local `normalize_cli_to_action` wrapper
- [#5] main.rs: split `run()` into five explicit phases: parse → resolve → config → init → execute
- [#28] cli.rs: consolidate repeated flags into `CommonFlags` struct with `#[command(flatten)]`
- [#18] all: build `ResolvedPaths` once after config load instead of reconstructing at every call site
- [#43] build.rs: pre-size HashMaps with `HashMap::with_capacity(country_count)`
- [#42] build.rs: merge `merge_ranges_v4` / `merge_ranges_v6` into one generic function
- [#62] build.rs: stream file hashing via `io::copy` instead of `fs::read` into memory; verify invariant #5

---

## ARCHITECTURE: build.rs RESTRUCTURING

- [#41] build.rs: split `build()` into `load_data` / `transform` / `write_outputs` / `generate_manifest` / `detect_orphans`
- [#45] build.rs: atomic build swap — write to temp dir, rename on success, discard on failure; depends on #41
- [#38] build.rs: stream CSV rows into `DashMap` grouping instead of materialising all rows first; check invariant #5

---

## ARCHITECTURE: fetch.rs RESTRUCTURING

- [#57] fetch.rs: split `fetch()` into `resolve_version` / `acquire_archive` / `verify_archive` / `extract_archive`; must not break existing parallelism
- [#54] fetch.rs: parallel ZIP entry writes via Rayon after sequential decompression; benchmark first
- [#71] backup.rs: parallel manifest verification via Rayon; measure overhead vs benefit first

---

## ARCHITECTURE: action.rs / EXECUTION PLANNER

- [#17] action.rs: make execution order explicit via `plan(action) -> Vec<Step>`; single source of truth
- [#19] action.rs: consolidate backup/clean if-blocks into `run_backup` / `run_clean`; depends on #17
- [#22] action.rs: bring `FetchMode::Remote | Local` into spec YAML; depends on #17
- [#20+30] cli.rs/action.rs: consolidate duplicated "prune requires backup" (and similar) into one shared predicate; long-term: spec-driven semantics layer
- [#29] cli.rs: remove ad hoc ambiguity checks; let planner inability to produce a `Vec<Step>` be the rejection signal

---

## VERSION HANDLING CHAIN [#52 → #69 → #70]

- [#69] all: introduce `struct Version(String)` with `parse`, `as_str`, `archive_name`; depends on #52
- [#70] backup.rs: replace `BTreeMap<String, _>` with `BTreeMap<Version, _>` for explicit sort semantics; depends on #69

---

## SPEC-DRIVEN ARCHITECTURE: SPECIFIC TASKS

- [#31] cli.rs: wire validation error strings to `reason_templates` from spec; follows from semantics layer
- [#92] docgen/tests: expand spec validation to catch contradictions; use `CLI_MATRIX` for fuzzing and property tests

---

## DOCGEN (xtgeoip-docgen.rs)

- [#78] docgen: check `spec.version` against `SUPPORTED_SCHEMA_VERSION` at load time; bail on mismatch
- [#73] docgen: replace hardcoded `spec.commands.get("fetch")` etc. with generic iteration
- [#74] docgen: make validators iterate same command set as generators; depends on #73
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

- [#24] pipelines: no rollback on mid-pipeline failure; address via execution planner (#17) managing temp dir
- [#38] build.rs: CSV streaming — benchmark memory savings before implementing
- [#54] fetch.rs: parallel ZIP writes — benchmark before committing
- [#71] backup.rs: parallel manifest verification — measure before committing
- [#88] unit tests: large undertaking; defer until architecture refactoring (#3–#34) stabilises; requires sandboxing, mock HTTP, CSV fixtures, setup/teardown
