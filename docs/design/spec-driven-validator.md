# Design: Spec-Driven CLI Validator

Status: **APPROVED** (design-first; reviewed by advisor + user 2026-06-08)
Date: 2026-06-08
Related: TODO #9/#26/#27/#34 (spec-driven arc), #32 (preserve Action shape),
#92 (proof-model blind spot), #22 (FetchMode→spec), #29 (ambiguity).

---

## 1. Problem

CLI semantics live in **two unsynchronised places**:

| Source | Role | Authority |
|---|---|---|
| `cli.rs::normalize_cli_to_action` | what flags *mean* (hand-written if-chain) | what the binary actually runs |
| `cli.yaml` `examples` | documented samples of meaning | what docgen checks |

The examples are *samples*, not a *function*. Worse, `proof.unique_maps_to`
forbids a second example per error case, so a wrongly-accepted combination
simply has **no example that could catch it**. This is exactly how the p⊕f leak
(`build -b -p -f` accepted) and the top-level `-b -c -p` over-rejection survived
(fixed in `a6db27b`; #92).

**Goal:** make `cli.yaml` the single source of truth for CLI *semantics* (not
just samples), generate the rule table, and rewrite the guard chains as a generic
data-driven evaluator — so the rules cannot drift from the code, and docgen can
*prove* the rules reproduce every documented example.

---

## 2. Key finding: every guard is a pure conjunction

I extracted all 17 guards from the current `normalize_cli_to_action`. **Every one
is a conjunction of flag literals** — a set of flags that must be PRESENT and a
set that must be ABSENT. No disjunction is needed in predicate position (the one
apparent "or", `f ∧ ¬(b∨c)`, is `f ∧ ¬b ∧ ¬c` by De Morgan — still a
conjunction).

| Context | Guard predicate | → error case |
|---|---|---|
| top_level | `l` | top_level_legacy |
| top_level | `p ∧ ¬b ∧ ¬c` | top_level_prune_no_target |
| top_level | `f ∧ ¬b ∧ ¬c` | top_level_force_no_target |
| top_level | `c ∧ p ∧ f ∧ ¬b` | top_level_prune_clean_force |
| top_level | `c ∧ p ∧ ¬b` | top_level_prune_with_clean |
| top_level | `b ∧ p ∧ f` | top_level_prune_force |
| fetch | `l` | fetch_no_legacy |
| fetch | `b` | fetch_no_backup |
| fetch | `c` | fetch_no_clean |
| fetch | `f` | fetch_no_force |
| build | `f ∧ ¬b ∧ ¬c` | build_force_no_target |
| build | `p ∧ ¬b` | build_prune_no_backup |
| build | `p ∧ f` | build_prune_force |
| run | `f ∧ ¬b ∧ ¬c` | run_force_no_target |
| run | `p ∧ f` | run_prune_force |
| run | `b ∧ c ∧ p` | run_prune_ambiguous |
| conf | `¬d ∧ ¬s ∧ ¬e` | conf_missing_flag |

Consequence: the entire validator reduces to **per-context ordered lists of
(require-set, forbid-set, error-key)**, evaluated first-match. The evaluator is
~3 lines. The TODO's worry that "the evaluator becomes as complex as the if-chain
it replaces" does not materialise: the complexity moves into declarative data,
and the runtime is a trivial set-membership scan.

Precedence is **list order**, made explicit by the YAML sequence. Order is
load-bearing: e.g. `top_level_prune_clean_force` (`c∧p∧f∧¬b`) MUST precede
`top_level_prune_with_clean` (`c∧p∧¬b`), since the latter subsumes the former.

The table above is the faithful *extraction* of the current code. In the spec
surface (§3.1/§3.2) it is refined: the single-flag "flag-not-allowed" rows
(top_level `l`; all four fetch rows) become `reject:` entries, the rest stay as
combination `guards:`, and the conf row leaves the guard model entirely (§3.2).
All of these still lower to one ordered first-match table at runtime.

---

## 3. Vocabulary decision (the main review question)

Two candidate vocabularies. The TODO anticipated **B** (relational
requires/conflicts). My analysis favours **A** (conjunction guards). This is the
key decision to confirm with the reviewer.

### Option A — ordered conjunction guards (RECOMMENDED)

```yaml
top_level:
  kind: FlagCommand
  allowed_flags: [b, c, p, f]
  reject:                         # ordered list; flag-set MUST == complement
    - { flag: l, error: top_level_legacy }   #   of allowed_flags (see §3.1)
  guards:                         # ordered; first match wins (= precedence)
    - require: [p]   forbid: [b, c]      error: top_level_prune_no_target
    - require: [f]   forbid: [b, c]      error: top_level_force_no_target
    - require: [c, p, f] forbid: [b]     error: top_level_prune_clean_force
    - require: [c, p]    forbid: [b]     error: top_level_prune_with_clean
    - require: [b, p, f]                 error: top_level_prune_force
  examples: [ ... unchanged ... ]
```

Pros:
- **Faithful**: one guard ↔ one current if-branch ↔ one error message. Compound
  errors (`prune_clean_force`) map naturally — no message re-derivation, no risk
  of drifting from the production `xtgeoip-tests` message strings.
- **Trivial evaluator** and trivial codegen. Auditable by eye against the table
  in §2.
- Precedence is explicit and local (sequence order), no separate priority field.

Cons:
- Encodes the rules at the granularity of *messages*, not *intent* ("force needs
  a target" is implicit in several guards rather than stated once). This is the
  honest granularity, though: the messages ARE the contract.

### Option B — relational rules (requires / conflicts / rejects)

```yaml
top_level:
  rejects: { l: top_level_legacy }
  requires:
    - { flag: f, any_of: [b, c], error: top_level_force_no_target }
    - { flag: p, any_of: [b],    error: ... }
  conflicts:
    - { flags: [p, f], error: top_level_prune_force }
```

Pros: reads like intent; "p conflicts f" stated once.
Cons (decisive): (1) cannot express compound errors without extra machinery;
(2) needs a **separate, explicit precedence declaration** because multiple
relational rules fire on one input and we must pick the same one the snapshot
expects; (3) rule→message mapping is indirect → higher drift risk. It looks
cleaner but is strictly more machinery for the same (or worse) fidelity.

**Recommendation: Option A.** It is the minimal faithful representation. Option B
optimises for a readability that the compound top-level errors defeat anyway.

### 3.1 `reject` vs `allowed_flags` — no second source of truth

Several "guards" in §2 are not combination constraints at all — they are
"this flag isn't valid in this context": fetch rejects `{l,b,c,f}` (exactly the
complement of `allowed_flags: [p]`), top-level rejects `{l}` (complement of
`[b,c,p,f]`). Listing them as single-flag `guards` would **re-encode
`allowed_flags` inside `guards`** — the very two-sources bug this effort removes,
now moved *inside the spec*.

Resolution: split the two guard kinds.
- **`reject:`** — a per-context *ordered list* of `{flag, error_case}`, ONLY for
  flags outside `allowed_flags`. It carries solely the *message identity* (which
  can't be derived: `fetch_no_legacy` ≠ `top_level_legacy` for the same flag
  `l`). It is an ordered list, not a map, because precedence among
  simultaneously-present disallowed flags is load-bearing: the snapshot locks
  `fetch -b -l → fetch_no_legacy` (legacy checked first), which alphabetical map
  iteration would break. docgen validates that the *set* of `flag`s equals the
  complement of `allowed_flags`; order is preserved as precedence.
- **`guards:`** — genuine combination constraints over *allowed* flags only.

docgen then:
1. **asserts `reject` keys == complement of `allowed_flags`** (closes the
   intra-spec drift: a newly-disallowed flag with no message, or a stale reject,
   fails codegen);
2. **lowers** each `reject` entry into a leading single-flag guard
   (`require:[flag]`, no forbid) ordered *before* the combination guards, then
   appends `guards`, emitting one ordered table.

Precedence "rejections first" is correct everywhere: top-level checks `l` before
any combination (current code does too); fetch is rejections-only; build/run have
empty reject sets. Runtime stays a single uniform flat first-match scan (§4) —
the split exists only in the *spec surface*, not the evaluator.

### 3.2 conf is out of scope (SelectorCommand, not a guard context)

`conf` is a `SelectorCommand` (positional `mode`, `exactly_one_positional`), not a
`FlagCommand`. Its two constraints are already enforced *without* `normalize`:
"at most one of d/s/e" by clap's `ArgGroup(multiple(false))` at parse time (→
`PARSE_ERR`), "at least one" is the `required` positional semantic
(→ `conf_missing_flag`, the lone `normalize` check). It therefore does **not**
enter the `reject`/`guards` vocabulary; the validator skips it and the existing
hand-written conf branch stays. Forcing conf into the guard model would be a third
encoding of a rule clap already owns.

> Pre-existing inconsistency to log separately (NOT this task): the spec *models*
> conf as a positional command, but `cli.rs` parses `-d/-s/-e` as flags, and even
> the spec's own `usage: "xtgeoip conf <-s|-d|-e>"` is flag-style. The model and
> the code disagree on surface syntax. This is the known run_conf special-case;
> it deserves its own reconciliation item, out of scope here.

---

## 4. Codegen vs runtime

- **docgen** parses the new `guards:` blocks, validates them (§5), and emits
  `src/generated/cli_rules.rs`: a const table per context of
  `Guard { require: &[Flag], forbid: &[Flag], error_key, message }`.
  - Flag representation: **flag-name atoms compiled to a small bitmask** in the
    generated source, e.g. `Guard { require: B|P|F, forbid: 0, key: "...", msg: NO_PRUNE_FORCE }`.
    Bitmask = trivial/fast runtime; the generated source stays readable because
    we emit the symbolic `B|P|F` form, not raw integers.
  - `message` references the existing generated `error_text.rs` const for that
    error case (no duplication of message text).
- **runtime** (`cli.rs`): a generic evaluator
  ```rust
  fn first_guard(flags: u8, guards: &[Guard]) -> Option<&Guard> {
      guards.iter().find(|g| flags & g.require == g.require
                          && flags & g.forbid == 0)
  }
  ```
  `normalize_cli_to_action` per context becomes:
  1. collect present flags into a `u8` bitset;
  2. `if let Some(g) = first_guard(bits, CONTEXT_GUARDS) { return Err(keyed_err(g.key, g.msg)); }`
  3. **construct the Action by hand, unchanged** (preserves #32).

### Scope discipline
Replace **only the guard chains** (the drift-prone, bug-prone part). Leave the
valid-path Action construction (`TopLevelBackup { clean, force, prune }`, etc.)
hand-written in Rust. Mapping flags→variant+fields data-drivenly is a separate,
harder problem the TODO explicitly defers (#32 = preserve the shape). Keeping it
out keeps this change small and the diff auditable.

---

## 5. Cross-check (closes #92)

Two layers, because each owns a different oracle:

1. **docgen-level — rules vs examples (NEW).** For every example in every
   context, evaluate the guards against that example's flag set and assert:
   - valid example → no guard fires;
   - invalid example → first firing guard's `error` == example's `maps_to`.
   A mismatch fails codegen. This keeps the rules and the documented examples
   provably consistent. **It does NOT, by itself, close #92** — examples are a
   subset of inputs, so this check only keeps examples *honest*. What actually
   makes "no example exists" irrelevant is the exhaustive snapshot below (all 136
   combos). Attribution: docgen-vs-examples = examples can't lie about the rules;
   snapshot = the rules are checked over the *entire* input space (#92).

2. **test-level — evaluator vs snapshot (EXISTING).**
   `cli::snapshot::cli_semantics_snapshot` already locks all 136 combos including
   full `Action` Debug output. After the rewrite it exercises the
   evaluator-backed `normalize_cli_to_action` and must stay **green
   byte-for-byte**. This is the behavioural proof of the refactor.

### Why docgen can't own the 136-combo check
The snapshot oracle includes `Action` Debug strings
(`TopLevelBackup { clean: true, ... }`). docgen does not link the `Action` type
(no `[lib]` target — see §6), so it cannot reproduce those strings. Therefore the
full-combo proof stays in the cargo test; docgen owns only the rules-vs-examples
proof. This split is intentional and sufficient: examples pin the error taxonomy,
the snapshot pins the complete input→Action function.

---

## 6. Structural constraint: no shared evaluator (yet)

The crate has **no `[lib]` target**. `src/main.rs` and each `src/bin/*.rs`
compile as independent binaries (docgen even re-declares its own `Spec` types).
So docgen's rules-vs-examples eval (§5.1) and the runtime evaluator (§4) cannot
*share* one function today.

Options:
- **(a) Introduce a minimal `[lib]`** exposing `Guard`, the `Flag` bitset, and
  `first_guard`. Both `main.rs` and docgen depend on it → one evaluator, zero
  divergence risk.
- **(b) Duplicate the ~3-line evaluator** in docgen. Both copies are pinned: a
  runtime-eval bug fails the snapshot, a docgen-eval bug fails the
  rules-vs-examples check. Identical-divergence in 3 lines is implausible.

**Decision: (b) for this change; decide the lib on #88, not on sharing 3 lines.**
A `[lib]` is a build-structure change that should be justified by the testing
architecture (#88, HIGH), not by deduplicating a trivial evaluator. Note: unit
tests already run *inside the bin* via `#[cfg(test)]` — `cli::snapshot` is the
proof — so a lib is **not required** for #88 either; it would only enable
external/integration test crates. Therefore: duplicate now (snapshot-pinned),
and treat the lib as an independent #88 decision.

---

## 7. Implementation order (after design sign-off)

1. (If 6a) add `src/lib.rs` exposing `Flag`/`Guard`/`first_guard`; no behaviour
   change; snapshot stays green.
2. Add `guards:` to `cli.yaml` for all five contexts, transcribed from §2.
   Run docgen with the NEW rules-vs-examples check ENABLED but the runtime still
   on the old if-chain → proves the transcription matches the examples before any
   runtime swap.
3. docgen emits `src/generated/cli_rules.rs`.
4. Rewrite `normalize_cli_to_action` to use `first_guard` + unchanged Action
   construction. Run `cargo test cli_semantics_snapshot` → MUST be green
   byte-for-byte. Any intended diff = reviewed `regenerate_snapshot`, never
   silent.
5. Delete the now-dead `NO_*` imports / if-branches. Re-run snapshot + docgen.
6. `cargo +nightly fmt`, clippy, then sync (note: sync.py does NOT run cargo test
   — run it manually; #96).

## 8. Outside the guard model (owned by the construction tail)
- **ShowHelp is two-layer.** At the `normalize` layer, bare `xtgeoip` returns
  `Ok(CliOutcome::ShowHelp)` (what the snapshot records). At the `main` layer it
  prints help, emits `Error [top_level_no_args]`, and exits 1 — which is what
  example `TL-001` (`maps_to: top_level_no_args`) documents. So `top_level_no_args`
  is NOT a guard; it is the empty-top-level case the construction tail owns. The
  docgen cross-check special-cases it: at top_level, empty flag set →
  `top_level_no_args`; empty flag set in any other context → valid.
- The current `"unsupported flag combination"` else is unreachable given the
  guards; keep it as a defensive `unreachable!`-style fallback, not a guard.
- The guard `Flag` bitset spans only **b, c, p, f, l**. conf's d/s/e are NOT in
  it (conf is out of scope, §3.2).

## 9. Resolved decisions (was: open questions)
- **Q1 (vocabulary): Option A** — conjunction guards, with the `reject`/`guards`
  split of §3.1. (Reviewed: confirmed over the TODO's anticipated Option B.)
- **Q2 (lib target): defer** — duplicate the evaluator (6b); decide `[lib]` on
  #88 grounds (§6).
- **Q3 (flag repr): symbolic bitmask** (`B|P|F`) in generated source.
- **Q4 (messages): faithful transcription is the hard rule** — messages are the
  contract with `xtgeoip-tests`; no rewording in this task.

Reviewer sign-off (2026-06-08): §3.1 (`reject`/`guards` split) and §3.2 (conf
scoped out, surface-syntax mismatch logged separately) **approved**. Cleared to
implement per §7.
