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

The `Action` enum is explicit, type-safe, and easy to extend — keep this shape. The Action construction blocks (e.g. `Ok(Some(Action::Build { legacy, backup, ... }))`) are the right pattern; the change needed is that they should be generated from the semantics layer rather than hand-written. The individual items in this TODO are stepping stones toward this architecture; items #5, #17, #19, #20, #22, #27/#31, #28, #29 are the key structural enablers.

Note [#32]: Preserve the `Action` construction pattern — the change is in the source of the construction logic, not its shape.

---

## IMMEDIATE FIXES: SILENT FAILURES AND ERROR CONTEXT

### #35 — build.rs: discard of write errors

`Err(_) => write_errors += 1` discards which file failed and why. Collect errors:
```rust
let mut errors = Vec::new();
for result in write_results {
    match result {
        Ok(entry) => checksums.push(entry),
        Err(e)    => errors.push(e),
    }
}
if !errors.is_empty() {
    for e in &errors { messages::error(&format!("{e:#}")); }
    bail!("{} file write(s) failed during build", errors.len());
}
```

### #63 — backup.rs: discard of delete errors

`.filter(|f| fs::remove_file(f).is_err())` discards error path and reason. Collect:
```rust
let mut failed = vec![];
for f in &files {
    if let Err(e) = fs::remove_file(f) {
        failed.push((f, e));
    }
}
if !failed.is_empty() {
    for (f, e) in &failed {
        messages::error(&format!("Failed to delete {}: {e:#}", f.display()));
    }
    bail!("{} file(s) could not be deleted", failed.len());
}
```

### #66 — build.rs: silent ignore of orphan file list write

`let _ = fs::write(...)` silently ignores write failure for orphaned file list. Log it. Also: the orphaned file list is one of the few cases where printing every item is required — emit the full list to both stdout and log regardless of whether the disk write succeeds.

### #13 — config.rs: no context on config copy failure

`fs::copy(DEFAULT_CONFIG, SYSTEM_CONFIG)?` gives no context on failure. Wrap with `.with_context(...)`. Two specific modes: missing source (no hint what to do) and permission denied (no actionable guidance). Pre-check source existence with a dedicated error message.

### #14 — config.rs: editor exit status ignored

`Command::new(editor).arg(SYSTEM_CONFIG).status()` ignores editor exit status. Check it:
```rust
let status = Command::new(editor).arg(SYSTEM_CONFIG).status()?;
if !status.success() {
    return Err(anyhow!("Editor exited with status {}", status));
}
```
Also handle `$EDITOR` being unset or empty before spawning.

### #72 — backup.rs / build.rs: bulk delete log messages missing count

"Deleted old binary data files" gives no count. Include it: `"Deleted 42 files from /usr/share/xt_geoip"`. Apply to all bulk delete operations.

---

## IMMEDIATE FIXES: CORRECTNESS

### #36 — build.rs: O(n²) orphan detection

`written_files.contains(&p)` inside `.filter()` is O(n²). Use HashSet:
```rust
let written_set: HashSet<_> = files_to_write.iter()
    .chain(std::iter::once(&manifest_path)).cloned().collect();
let orphaned: Vec<_> = all_existing.into_iter()
    .filter(|p| !written_set.contains(p)).collect();
```

### #37 — build.rs / fetch.rs: duplicated mmap safety comments

`unsafe { Mmap::map(file) }` repeated at multiple sites with duplicated safety comments. Wrap once:
```rust
fn mmap_file(file: &File) -> anyhow::Result<Mmap> {
    // Safety: caller guarantees the file is not mutated while the map is live
    Ok(unsafe { Mmap::map(file)? })
}
```

### #60 — backup.rs: manifest parser accepts single-space separators

BLAKE3 manifests use double-space canonically. Enforce it:
```rust
let (hash, file) = line.split_once("  ")
    .ok_or_else(|| anyhow!("Invalid manifest format at line {}", idx + 1))?;
```

### #4 — main.rs: Rayon pool init failure silently discarded

`.build_global().ok()` silently discards Rayon pool initialisation failure. Use `std::sync::OnceLock` so initialisation is attempted exactly once and genuine failures are surfaced.

### #7 — main.rs: exit codes are magic numbers

Exit codes are implicit magic numbers (`exit(2)` = CLI error, `exit(1)` = runtime error). Define explicitly:
```rust
const EXIT_RUNTIME_ERROR: i32 = 1;
const EXIT_CLI_ERROR: i32 = 2;
```
Or better, tie to a typed error taxonomy where each variant carries its own exit code. Makes exit codes auditable and testable.

### #33 — cli.rs: `normalize_cli_to_action` returns `Result<Option<Action>>`

The `None` state means "show help" — a distinct outcome, not a real absence. Replace:
```rust
enum CliOutcome { Action(Action), ShowHelp }
```
Return `Result<CliOutcome>`. Eliminates the Option unwrap at the call site.

### #59 — backup.rs: glob is fragile for path enumeration

`glob(&format!("{}/*...", data_dir.display()))` is fragile when `data_dir` contains spaces or glob metacharacters. Replace with `fs::read_dir` + explicit predicate:
```rust
fs::read_dir(data_dir)?
    .filter_map(|e| e.ok())
    .map(|e| e.path())
    .filter(|p| {
        matches!(p.extension().and_then(|e| e.to_str()), Some("iv4" | "iv6"))
        && p.file_stem().and_then(|s| s.to_str())
            .map(|s| s.len() == 2 && s.chars().all(|c| c.is_ascii_uppercase()))
            .unwrap_or(false)
    })
```
Drop the `glob` crate dependency if this is its only use.

---

## ATOMICITY: WRITE-TO-TEMP PATTERN [#48, #64, #65]

All three apply the same discipline: write to a `.part`/temp location, rename atomically on success.

### #48 — fetch.rs: partial download left at final path

Failed download leaves partial `.zip` at final path. Download to `.part` file:
```rust
let tmp_path = archive_path.with_extension("zip.part");
let mut archive_file = File::create(&tmp_path)?;
// stream into tmp_path ...
fs::rename(&tmp_path, &archive_path)?;
```

### #64 — backup.rs: backup written directly to final path

`fs::File::create(output_path)?` writes `.tar.gz` directly. An interruption leaves corrupt partial backup. Same pattern:
```rust
let tmp = output_path.with_extension("tar.gz.part");
// write into tmp ...
fs::rename(&tmp, output_path)?;
```

### #65 — backup.rs: unconditional file list push can produce duplicates

`files.push(version_path(...))` and `files.push(manifest_path)` are unconditional. Future changes could cause duplicates in the tar. Deduplicate before use:
```rust
files.sort(); files.dedup();
```

---

## FETCH RESILIENCE

### #47 — fetch.rs: no timeout on HTTP client

`Client::builder().build()?` has no timeout. A stalled download hangs indefinitely:
```rust
let client = Client::builder()
    .timeout(std::time::Duration::from_secs(300))
    .build()?;
```
Consider making it configurable via `[maxmind]` in TOML config.

### #46 — fetch.rs: Content-Disposition parsing failures not handled

`resp.headers().get(CONTENT_DISPOSITION)` can fail in several ways (header absent, invalid UTF-8, unparseable filename). Audit the full chain and ensure each failure produces a clear error or safe fallback — not a panic or silent `None` propagation.

### #49 — fetch.rs: cached archive reused without re-verification

When `archive_path.exists() && checksum_path.exists()`, the archive is reused without re-verification. Re-verify before trusting:
```rust
if archive_path.exists() && checksum_path.exists() {
    if verify_checksum(&archive_path, &checksum_path)? {
        // reuse
    } else {
        messages::warn("Local archive checksum mismatch, re-downloading");
    }
}
```

### #55 — fetch.rs: no retry on transient network errors

`.send()?` fails immediately on any network error. Even one retry would improve resilience. Implement exponential backoff (MAX_RETRIES = 3, BASE_DELAY = 2s). Retry only transient errors (connection reset, timeout, 5xx); do not retry 4xx or checksum mismatch.

### #50 + #56 — fetch.rs: download size issues [merged]

Two related download size issues:

**Cap enforcement (#56)**: `resp.take(10GB)` silently truncates; the failure surfaces later as a checksum mismatch. Track bytes written and fail explicitly on breach:
```rust
let written = io::copy(&mut limited, &mut file)?;
if written >= MAX_DOWNLOAD_BYTES {
    bail!("Download exceeded size limit — refusing to use truncated archive");
}
```

**Sanity check (#50)**: No lower bound and no comparison against historical sizes. Read `Content-Length` before downloading; compare against the most recent local archive size with a configurable tolerance (e.g. ±50%). If outside range: warn, refuse, reuse local or fail. Both fixes use `MAX_DOWNLOAD_BYTES` constant.

---

## CONFIG AND CONF SUBCOMMAND

### #12 — config.rs: SYSTEM_CONFIG path not centralised

`Path::new(SYSTEM_CONFIG)` repeated at multiple call sites. Centralise:
```rust
fn system_config_path() -> &'static Path { Path::new(SYSTEM_CONFIG) }
```
Single place to evolve path strategy (env override, XDG paths).

### #11 — config.rs: config not validated after deserialisation

`toml::from_str(&contents)?` deserialises but does not validate. Add:
```rust
let cfg: Config = toml::from_str(&contents)?;
cfg.validate()?;
```
`validate()` checks invariants TOML cannot express: non-empty required fields, path existence, value ranges, conflicting settings.

### #10 — config.rs: conf subcommand assumes interactive terminal

`xtgeoip conf` assumes interactive terminal. Guard with TTY check:
```rust
if atty::is(atty::Stream::Stdin) {
    // prompt / open $EDITOR
} else {
    println!("Config missing. Run `xtgeoip conf -d` to create it.");
}
```
Use `std::io::IsTerminal` (Rust ≥ 1.70) to avoid the `atty` dependency.

### #16 — config.rs: `ensure_system_config_exists` mixes concerns

`ensure_system_config_exists()` mixes filesystem logic, user interaction, and policy decisions. Split into:
```rust
fn config_exists() -> bool
fn create_default_config() -> Result<()>
fn prompt_create_config() -> bool  // TTY-gated (see #10)
```
Depends on #10. Call site then orchestrates policy explicitly.

### #23 — main.rs / action.rs: `Action::Conf` handled in two places

`Action::Conf` is handled in both `main.rs` and `action.rs`. Pick one: handle in `main.rs` (before `run_action`) and remove from `action.rs`. `conf` is a meta-operation, not a pipeline step.

### #6 — main.rs: permission errors detected after the fact

Permission errors are detected after the fact via `downcast_ref::<io::Error>()`. Check upfront:
```rust
if action.requires_root() && !is_root() {
    return Err(anyhow!("This operation requires root"));
}
```
Add `requires_root() -> bool` to `Action`.

### #15 — config.rs: `ConfAction` missing preconditions and error taxonomy

`ConfAction` is a clean enum but missing explicit preconditions (e.g. `Edit` requires config to exist — currently buried in control flow) and error taxonomy integration. Each variant should carry/derive its own precondition check and error type. Depends on #7 (exit codes) and the spec-driven direction.

### #1 — messages.rs / config.rs: file logging not optional

Logging to file should be optional. Configurable via `[logging]` in TOML config, overridable with a CLI flag (flag takes precedence). When disabled, log output goes to stderr only (or is suppressed, TBD).

---

## LOGGING AND STEP VISIBILITY

### #25 — action.rs: no logging at dispatch points

`action.rs` does zero logging. Add lightweight log calls at each dispatch point:
```rust
messages::info("Starting backup");
messages::info("Running fetch (remote)");
messages::info("Building binary database");
```
Each `Step` (see #17) should emit at least one log line when it begins.

---

## SECURITY HARDENING

### #61 — global: parse-then-validate principle

No parser, loader, `File::open()`, download handler, or deserialiser should be trusted for validation unless it explicitly enforces it. Rule: **Parse, then validate. Never assume parsing implies validity.** Apply everywhere — CSV reader, TOML deserialiser, ZIP extractor, `reqwest`, manifest parser, `serde`. Where a crate claims to validate, audit whether its validation is strict enough for a hostile input scenario.

### #51 — fetch.rs: remote content not treated as hostile

Treat all remote content as hostile. Layers of defence:

**Before extraction**: verify ZIP magic bytes (`PK\x03\x04`); check central directory for path traversal (`../`), absolute paths, unexpected file types (executables, symlinks), entry count sanity.

**After extraction**: verify each file has expected extension (`.csv`) and plausible size; check CSV header row matches expected column schema.

**Parser hardening**: guard against empty required fields, semantically invalid values (negative geoname_ids, country codes > 2 chars, IP ranges where start > end), excessively long field values.

Crate options: `infer` or `content_inspector` for magic detection; `zip` crate already exposes central directory metadata.

### #52 — fetch.rs / backup.rs: archive name parsed in multiple places

`GeoLite2-Country-CSV_YYYYMMDD.zip` is parsed in multiple places. Centralise:
```rust
fn parse_archive_name(name: &str) -> Option<String>  // returns version token
```
Also harden: treat the remainder after the stable prefix as an opaque version token rather than assuming date format. Log a warning if it doesn't look like a date but still proceed — don't reject a valid archive because MaxMind changed versioning.

Note: #52 is also a dependency for the Version type chain (#69 → #70).

### #53 — fetch.rs: `flatten_to_temp_root` assumes single top-level directory

`flatten_to_temp_root` assumes exactly one top-level directory in the ZIP. Harden: only strip the first component if it is common to all entries (detect dynamically); warn if archive structure doesn't match expected shape.

---

## MIGRATION

### #2 — Cargo.toml / docgen: migrate from `serde-yaml` to `serde-saphyr`

`serde-yaml` is deprecated. Migrate to `serde-saphyr` (maintained successor, compatible API). Do alongside #77 and #79 to avoid touching the YAML serialisation path twice.

---

## TYPED ENUMS: ELIMINATING BOOLEAN TRAPS

### #21 — action.rs / build.rs: `prune_archives` takes opaque booleans

`prune_archives(cfg, bool, bool)` — two opaque positional booleans. Replace:
```rust
enum PruneMode { CsvOnly, BinOnly, Both }
fn prune_archives(cfg: &Config, mode: PruneMode) -> Result<()>
```
Inconsistencies across call sites become visible. `Step::Prune` can carry its `PruneMode` as data.

### #67 — backup.rs: `backup` takes opaque force bool

`fn backup(..., force: bool)` — the Verified/Force distinction is good design but opaque. Replace:
```rust
enum BackupMode { Verified, Force }
fn backup(cfg: &Config, mode: BackupMode) -> Result<()>
```

### #68 — backup.rs: `gather_files_force` and `gather_files_verified` share structure

Once `BackupMode` exists (#67), merge into:
```rust
fn gather_files(data_dir: &Path, mode: BackupMode) -> Result<Vec<PathBuf>>
```
Depends on #67.

### #39 — build.rs: country codes are heap-allocated `String`

Country codes are `String` everywhere — always exactly 2 ASCII chars, `String` is expensive. Replace with:
```rust
type CountryCode = [u8; 2];
```
Or richer:
```rust
enum CountryCode { Iso([u8; 2]), O1, A1, A2 }
```
Stack-allocated, `Copy`, `PartialEq` by value, no cloning.

*Note [#40]*: Once this enum exists, O1 fallback logic duplicated in `load_countries` and `resolve_country_code` is replaced by `CountryCode::O1` — centralised by the type itself.

*Note [#44]*: Calls like `"O1".to_string()` that allocate on every use become enum variants with no allocation.

---

## ARCHITECTURE: ANALYSIS AND SMALL REFACTORS

### #8 — all modules: re-analyse separation of concerns

Re-analyse the distribution of responsibilities across all modules (including `main.rs`) to verify clean separation of concerns. Likely problem areas: overlap between `main.rs`, `cli.rs`, and `action.rs`; config loading touching runtime concerns; logic that has drifted into the wrong layer. This is a prerequisite analysis before larger refactoring.

### #3 — main.rs: redundant local wrapper around cli module function

A local `normalize_cli_to_action` does nothing but delegate to `crate::cli::normalize_cli_to_action`. Either call the cli module function directly from `main`, or consolidate ownership into one place.

### #5 — main.rs: `run()` mixes multiple concerns

`run()` mixes command dispatch, config loading, and runtime setup. Split into explicit sequential phases:
1. Parse (CLI → raw args)
2. Resolve action (args → `Action`)
3. Load config
4. Init runtime (Rayon pool, logger)
5. Execute

### #28 — cli.rs: flags duplicated across subcommands

The same flags (`backup`, `clean`, `force`, `prune`, `legacy`) are declared independently in `Cli`, `Run`, `Build`, and `Fetch` (hidden). Consolidate via `#[command(flatten)]`:
```rust
#[derive(Args)]
pub struct CommonFlags {
    #[arg(short, long)] pub backup: bool,
    #[arg(short, long)] pub clean:  bool,
    #[arg(short, long)] pub force:  bool,
    #[arg(short, long)] pub prune:  bool,
    #[arg(short = 'l', long)] pub legacy: bool,
}
```

### #18 — all: config paths reconstructed at every call site

`Path::new(&cfg.paths.output_dir)` and `Path::new(&cfg.paths.archive_dir)` repeated everywhere. Build once after config load:
```rust
struct ResolvedPaths<'a> { output: &'a Path, archive: &'a Path }
```

### #43 — build.rs: HashMaps not pre-sized

HashMaps are default capacity and rehash as they grow. Pre-size where upper bound is known: `HashMap::with_capacity(country_count)`.

### #42 — build.rs: `merge_ranges_v4` and `merge_ranges_v6` are duplicates

`merge_ranges_v4` and `merge_ranges_v6` are identical except for address type. Consolidate:
```rust
fn merge_ranges<T>(ranges: &[(T, T)]) -> Vec<(T, T)>
where T: Copy + Ord + num_traits::One + std::ops::Add<Output = T>
```

### #62 — build.rs: file hashing loads entire file into memory

`fs::read(&file_path)?` loads entire file for hashing. Stream instead:
```rust
let mut file = File::open(&file_path)?;
let mut hasher = blake3::Hasher::new();
std::io::copy(&mut file, &mut hasher)?;
let hash = hasher.finalize().to_string();
```
Verify vs invariant #5 — confirm no sequential bottleneck introduced.

---

## ARCHITECTURE: build.rs RESTRUCTURING

### #41 — build.rs: `build()` does too many things

`build()` does too many things. Split into focused phases:
```
load_data()          → CSV parsing
transform()          → group ranges by country code
write_outputs()      → write binary files, return paths + checksums
generate_manifest()  → write manifest from checksums
detect_orphans()     → compare written paths against existing files
```
`build()` becomes an orchestrator.

### #45 — build.rs: interrupted build leaves inconsistent state

Data files, version file, and manifest are written independently. Interrupted build leaves partial inconsistent state. Apply atomic swap at the build level (#41 first):
1. Write all output to a temporary directory
2. Write version and manifest into same temp dir last
3. `rename()` temp dir into place (atomic on same filesystem)
Discard temp on any failure; previous output untouched.

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
extract_archive()  → unpack to temp, flatten, move into place (#53, #54)
```
**Constraint: must not break any existing parallelism inside `fetch()`.**

### #54 — fetch.rs: ZIP entry writes are sequential

ZIP entry enumeration is sequential but file writes after decompression are independent. Decompress to buffer sequentially, then spawn parallel write tasks via Rayon. Not critical now; worthwhile if archive grows. **Benchmark before committing.**

### #71 — backup.rs: manifest verification is sequential

Consider Rayon `.par_lines()` or `.par_iter()`. On small datasets, overhead may exceed benefit. On NVMe with many files, likely a win. **Measure first.**

---

## ARCHITECTURE: action.rs / EXECUTION PLANNER

### #17 — action.rs: execution order is hardcoded and implicit

Execution order is hardcoded and implicit in each handler. Make it explicit:
```rust
enum Step { Backup, Clean, Fetch, Prune, Build }
fn plan(action: &Action) -> Vec<Step>
```
`plan()` becomes single source of truth. Testable, documentable, spec-alignable.

### #19 — action.rs: backup/clean pattern duplicated across handlers

The `if do_backup { backup(...)? } / if do_clean { delete(...)? }` pattern is duplicated across `TopLevelBackup`, `Run`, and `Build`. Consolidate:
```rust
fn run_backup(cfg: &Config, enabled: bool, force: bool) -> Result<()>
fn run_clean(cfg: &Config, enabled: bool, force: bool) -> Result<()>
```
Depends on #17. Each `Step` dispatches to exactly one function.

### #22 — action.rs: FetchMode semantics exist only in code

`FetchMode::Remote` and `FetchMode::Local` are a clean abstraction but their semantics exist only in code. Bring into spec:
```yaml
fetch:
  mode: remote | local
```
Depends on #17 and spec-driven direction.

### #20 + #30 — cli.rs / action.rs: semantics duplicated across two layers [merged]

Semantics are duplicated across two layers. CLI validates (e.g. `--prune` requires `--backup`) but Action layer also partially re-checks. If CLI changes, Action can silently behave incorrectly.

Specific example (#30): the same "prune requires backup" constraint has two different implementations:
```rust
// Top-level: if p && !b && !c { ... }
// Build:     if *prune && !*backup { ... }
```
Same rule, two expressions, can drift independently.

Pick one defensible position consistently: either trust CLI completely (Action has no re-checks) or validate defensively at Action (self-contained). Long-term: a semantics layer between CLI and Action that produces validated plans from spec constraints. Until then, at minimum consolidate into a single shared predicate.

### #29 — cli.rs: ambiguity checks have no formal basis

Ad hoc ambiguity checks (`if *prune && *force && *clean`, etc.) have no formal basis. "Ambiguous" is undefined. A combination is ambiguous if and only if the execution planner (#17) cannot produce a deterministic `Vec<Step>`. Remove current checks once planner exists; let inability to plan be the rejection signal.

---

## VERSION HANDLING CHAIN [#52 → #69 → #70]

Note: #52 is listed in Security Hardening (archive name parsing). The Version type work follows from it.

### #69 — fetch.rs / backup.rs / build.rs: version strings are untyped

Version strings are raw `String` everywhere. Introduce a typed wrapper (depends on #52):
```rust
struct Version(String);
impl Version {
    fn parse(s: &str) -> Option<Self> { ... }
    fn as_str(&self) -> &str { &self.0 }
    fn archive_name(&self) -> String { ... }
}
```
Validation and formatting helpers replace ad hoc `format!()` scattered across three files.

### #70 — backup.rs: version ordering is implicit in string sort

`BTreeMap<String, Vec<PathBuf>>` relies on lexicographic ordering of `YYYYMMDD` strings. This is correct but implicit. Once #69 exists, use `BTreeMap<Version, _>` with `Ord` derived on numeric value — sort order becomes a compile-time guarantee.

---

## SPEC-DRIVEN ARCHITECTURE: SPECIFIC TASKS

### #31 — cli.rs: validation error strings are hand-written and inconsistent

Error strings are hand-written and inconsistent. The spec defines `reason_templates` (e.g. `prune_requires_backup`) for exactly this purpose. Wire cli.rs validation errors to spec templates. Once the semantics layer drives validation from the spec, this follows naturally.

### #92 — docgen / tests: expand spec validation and utilise CLI matrix

Expand spec validation to catch logical contradictions (declared but never used flags, undeclared mutual exclusions, unreachable valid states). Also: `pub const CLI_MATRIX: &[CliExample]` is generated but underutilised. Use for fuzzing (seed corpus), property testing (`proptest`/`quickcheck`), and exhaustive parser validation.

---

## DOCGEN (xtgeoip-docgen.rs)

### #78 — docgen: schema version not checked

`pub version: String` is deserialised but never checked. Add explicit compatibility check:
```rust
const SUPPORTED_SCHEMA_VERSION: &str = "3.1";
if spec.version != SUPPORTED_SCHEMA_VERSION {
    bail!("Unsupported spec schema version '{}' (expected '{}')", spec.version, SUPPORTED_SCHEMA_VERSION);
}
```

### #73 — docgen: hardcoded command lookups defeat spec-driven model

Hardcoded `spec.commands.get("fetch")`, `get("build")`, etc. defeats the spec-driven model. Adding a new command in YAML would be silently missed. Replace with generic iteration:
```rust
for (name, cmd) in &spec.commands { visit(name, cmd, &mut used_error_cases)?; }
```

### #74 — docgen: validators and generators cover different command sets

Validators cover named commands explicitly while generators iterate `spec.commands.values()` generically. These paths don't cover the same set. A command generated into docs but not validated can have silently incomplete constraints. Fix: validation must iterate the same set as generation. Depends on #73.

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
Error [build_prune_without_backup]: --prune cannot be used without --backup
```
The bracket token is the `reason_template` key.

**In the test runner (#85)**: when `maps_to` is set, parse stderr for the bracket token:
```rust
if let Some(expected_code) = &tc.maps_to {
    // assert stderr contains format!("[{}]", expected_code)
}
```
Turns `maps_to` from documentation annotation into a live assertion. At minimum (before #90 is implemented), validate that `maps_to` values name real spec keys. Depends on #90+84.

### #86 — tests: `key: p/f` is too coarse

`key: p/f` is too coarse. Candidates in increasing scope: expected exit codes (`key: f2` = exit exactly 2), error class (`error_class: cli`), reason template match (`maps_to: prune_requires_backup` asserts that template was triggered). `key` should evolve into a structured expectation. Depends on #90+84 and #85+91.

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
