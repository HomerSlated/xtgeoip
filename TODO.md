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

## ✅ build: reverted atomic swap ✅ DONE (2026-06-13, `4909da4`)

`build.rs::atomic_swap` removed; write-in-place + `detect_orphans` reinstated.
`CountryCode` enum and incremental hasher retained (behaviour-neutral). Proven:
`sudo xtgeoip build` no longer touches foreign files in `output_dir`. See #24
for the constraint that must hold if an atomic swap is ever revisited.

---

## ✅ Spec-driven validator — COMPLETE (v0.2.0, 2026-06-09)

Design of record: `docs/design/spec-driven-validator.md` (approved 2026-06-08).
Gate 1 (`23c4375`): CLI rules declared in `cli.yaml`; docgen validates + cross-checks.
Gate 2 (`dfc14a9`): `cli.rs` drives generated `cli_rules.rs` guard tables (u8 bitmask,
`first_guard` evaluator); snapshot green byte-for-byte across all 136 combos.
Proven live (`2c090bd`): `-b -c -f` → `force_ambiguous` added purely through `cli.yaml`.

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

### #100 — fetch.rs: shared `.part` path allows concurrent-fetch interference

Guardian finding **F-1**, audit `guardian_report_20260718_214129.md`. **LOW — CVSS 3.3** (`CVSS:3.1/AV:L/AC:H/PR:L/UI:N/S:U/C:N/I:N/A:L`). Pre-existing; *not* introduced by the `PartialDownload` guard.

`fetch.rs` derives the temp path as `archive_path.with_extension("zip.part")`, which is deterministic per version. Two concurrent `xtgeoip fetch` processes resolving the same version therefore share one temp file: interleaved `io::copy` writes could corrupt it, and one process's guard could remove a path the other is still writing.

**Fails closed.** Any corruption is caught by SHA-256 verification before the archive is trusted, so a bad archive is rejected rather than consumed. The guard does not worsen it — the shared path predates it, and `remove_file` tolerates `NotFound`. Worst case is a spurious failed fetch that a retry resolves.

Optional hardening: a unique suffix (PID, or `tempfile::NamedTempFile` in `archive_dir`), or an advisory lock on `archive_dir` for the duration of a fetch. Weigh against the fact that concurrent fetches are not an expected usage pattern for this tool.

⚠ Touching `fetch.rs` invalidates its guardian signature and needs a re-audit.

### #101 — fetch.rs: no explicit HTTP redirect policy

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

⚠ Touching `fetch.rs` invalidates its guardian signature and needs a re-audit.

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

Still open under #92: expanding the *docgen-side* spec validator (contradictions detectable at generation time rather than test time).

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

### #89 — tests: orphaned file scenarios not covered ⚑ SEE DESIGN NOTE #98

**Premise partly wrong.** The *detection* exists (`detect_orphans`, called after every build, 6 unit tests) and the full legacy→default→clean cycle was demonstrated working on 2026-07-18. What is missing is an *integration* case and documentation of which clean form applies when (`build -c` during the switch back, `build -c -f` after the fact — see §4 of the design note). Concrete scenarios in §8; the final assertion (`xtgeoip.conf.example` survives) is the regression test for the `b4ec1db` data-loss bug.

Orphaned files from legacy/default mode switching are not covered by the rebuild logic. Add two explicit test scenarios:

**Scenario A (orphan detection)**: produce orphans → do not clean → run detection command → assert orphans identified.

**Scenario B (orphan cleanup)**: produce orphans → clean → run same detection → assert no orphans. Requires `requires:` dependencies and `rebuild:` annotations in YAML. Further analysis needed to establish if all state transitions are covered.

### #96 — CI / sync: run `cargo test` so the snapshot guard is enforced ✅ DONE (2026-07-18)

Original complaint: `scripts/sync.py` ran docgen → clippy → `+nightly fmt --check` → `build --release`, but **not** `cargo test`, so the CLI-semantics snapshot (`cli::snapshot::cli_semantics_snapshot`, golden at `src/cli_snapshot.golden`, commit `33ddeaa`) and any future `#[cfg(test)]` unit tests (#88) weren't enforced automatically.

**Stale as written (found 2026-07-18).** `cargo test` was wired in at some point after this was filed and the ticket was never updated: `scripts/sync.py:87` and the `test` job in `.github/workflows/rust.yml` both run it. The snapshot guard has in fact been enforced.

The real residual was narrower: both gates ran `cargo clippy --` **without** `--all-targets`, so lints in `#[cfg(test)]` code were never gated — test code compiles under `cargo test`, so this was lint coverage, not correctness. It let the `build.rs` `items_after_test_module` lint sit undetected until a manual `--all-targets` run caught it (fixed `22b3645`). Both gates now pass `--all-targets`, matching the `build` job, which already used it.

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

### #95 (original) — import generic agents from private/agents-out/

The seven project-agnostic agent role descriptions in `private/agents-out/` (bug-hunter, data-flow-tracer, deep-research-collector, docs-auditor, flow-doc-generator, optimisation-advisor, guardian-security) are to be imported as actual project agents, adapting each by filling its `[bracketed]` placeholders for xtgeoip (`[language]` = Rust, `[source-dir]` = `src/`, `[output-dir]` under `private/`, etc.).

Priority / notes:
- **docs-auditor** first — `private/WORKFLOW.md` already delegates its documentation-check step to this agent. Audit set: `README.md`, `CLAUDE.md`, `TODO.md` / `TODO_tldr.md`, `docs/design.md`, `docs/legacy.md`. Mark `docs/generated/` and `src/generated/` as docgen-owned (off-limits — change `docs/spec/cli.yaml`, not the output).
- **guardian-security** — GPG key already provisioned (ed25519, fpr `227E5FE6EB2D3E9EE5883CB1F4BF35E6DC8029B0`; public key `docs/guardian_public.asc`; keyring `private/guardian/gnupg/`; setup script `private/guardian/guardian-security-pre.sh`). Set `[signable-dirs]` to the tracked source dirs (note: anything under `private/` is gitignored, so per-file `.sig` signatures only make sense for files outside it).
- Remaining (bug-hunter, optimisation-advisor, data-flow-tracer, flow-doc-generator, deep-research-collector): adapt as needed when wanted.

---

## LOW PRIORITY / LARGE SCOPE

### #24 — pipelines: no rollback on mid-pipeline failure ⚑ SEE DESIGN NOTE #98

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

**⚠ See #1 PRIORITY.** This exact idea was implemented early (`b4ec1db`) and caused a data-loss bug: the atomic swap `remove_dir_all`s the whole `output_dir`, deleting files build never created. It has been reverted. If revisited, the temp/swap MUST respect manifest ownership — never delete unowned files, force-delete only build-created types (`.iv4`/`.iv6`).

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

### #88 — unit testing: mock the HTTP layer in fetch.rs

*(Retitled 2026-07-18. Was: "unit testing: no unit tests exist ⚑ HIGH PRIORITY (next after spec-driven architecture)". The original gap is closed — 93 unit tests exist; what remains is the network path alone, so the HIGH PRIORITY flag was dropped with it.)*

**Remaining scope.** Nothing exercises `fetch()`'s network path — `resolve_version`, `check_download_size`, `acquire_remote_archive`. Everything downstream of the download is already covered from fixtures. Needs a mock HTTP server or an injected transport; #12/#18 configurability is the enabler. Note `fetch.rs` is guardian-signed, so any change to it requires a re-sign.

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
