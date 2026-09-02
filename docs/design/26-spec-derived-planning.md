# Design: Spec-Derived Planning (#26/#27)

Status: **IMPLEMENTED** (all four stages; stage 4 signed off 2026-09-02)
Date: 2026-09-02
Related: TODO.md OVERVIEW (Spec-Driven Architecture), `spec-driven-validator.md`
(the *validity* half, APPROVED 2026-06-08 and landed), `29-ambiguity-planner-vs-guards.md`
§5–6 (which names this the "#26/#27 endpoint"), #22 (FetchMode→spec), #92.

---

## 0. What is actually left

The arc has two halves. **Validity** — which flag combinations are legal — is
done: `cli.yaml` `guards:` compile to `src/generated/cli_rules.rs` and
`cli.rs` evaluates them generically. **Ordering** — what a legal combination
*does* — is not: `action.rs::plan()` is a hand-written `match`.

As of `31fa77a` the spec *declares* the order for all 30 plan-bearing
documented invocations (`steps:`), and `spec_steps_agree_with_plan` enforces
agreement. So the remaining question is narrow:

> Generate `plan()` from the spec, or keep declaring-and-checking?

This note proposes generating it, by the same prove-then-replace method §7 of
the validator note used.

## 1. The finding that makes it tractable

Enumerating all 76 `Action` values shows every plan is a **subsequence of one
fixed order**:

    backup → prune_bin → fetch → clean → prune_csv → build

Ten of the eleven step pairs co-occur in some command and all agree with it;
the one exception, `prune_bin`/`prune_csv`, never co-occurs. There is exactly
one data dependency, `fetch → build`.

So the vocabulary is a **rank per step plus membership per context** — not a
dependency graph. A topological sort over one edge is machinery in search of a
problem. This is the ordering analogue of the validator's finding that all
seventeen guards were pure conjunctions.

**This is an observation about today's six steps, not an invariant.** Nothing
makes the total order hold. A future step that runs at different points in
different commands breaks the rank model and forces the graph. Recorded so a
later maintainer does not mistake a measurement for a law.

## 2. Vocabulary

```yaml
plan:
  steps:
    clean:
      rank: 40
      param: backup_mode
      why: >-
        Runs after the fetch (#24 stage 1). Cleaning first meant a network
        outage left output_dir empty with no replacement to install.
  contexts:
    build:
      always: [fetch, build]
      fetch_mode: local
      selects: { b: backup, p: prune_bin, c: clean }
```

Three deliberate choices:

- **`why:` is mandatory on every step.** A bare `rank: 40` keeps the conclusion
  and discards the reasoning that `action.rs:150-155` currently carries. The
  generator emits it as a comment in the generated source, so the rationale
  survives the migration instead of evaporating into an integer.
- **Contexts key on (context, flags), not on `Action` variants.** `top_level`
  covers both `TopLevelBackup` and `TopLevelClean`; the rank model unifies them
  because "backup when `b`, clean when `c`" reproduces both. Nothing needs to
  know the variant split.
- **`Plan::Simple` vs `Plan::Pipeline` is derived, never declared.** The variant
  follows from whether `build` is selected. If the spec could state it
  independently, a spec could declare a `Pipeline` with no fetch — and the
  Fetch-before-Build guarantee would fall back to a runtime check.

## 3. Why generated Rust, not a data table

`Plan::Pipeline` cannot be constructed without naming the fetch that feeds the
build; `Step` has no `Build` variant. That invariant is enforced by the type
system, and the old runtime `.expect("Build step requires prior Fetch")` was
deleted because it became unreachable by construction.

A flat `steps: [...]` table evaluated at runtime can express `[build]` with no
fetch, which trades a compile-time guarantee for a runtime check — a regression
under the INVARIANTS precedence order (hard errors above consistency).

So docgen emits **Rust that constructs `Plan`**, exactly as `cli_rules.rs`
emits Rust referencing `error_text::` constants by name. A spec that violates
the invariant produces output that does not compile. The guarantee is preserved
because codegen output must build.

## 4. Known limit: the Action↔context mapping is in the emitter

The generator hardcodes how each `Action` variant yields a context and flag
values (`Action::Build { backup, clean, prune, .. }` → context `build`).  That
is knowledge about the Rust type, not about the spec, and it lives in docgen's
emitter rather than in `cli.yaml`. If `Action` changes shape, the generated
code fails to compile — loudly, at build time, which is the acceptable failure.

## 5. Staging (prove, then replace)

1. `plan:` vocabulary in `cli.yaml`.
2. docgen emits `src/generated/plan.rs` with `plan_generated()`, **alongside**
   the hand-written `plan()`.
3. A differential test over all 76 `Action` values asserting the two agree
   **exactly**, including step parameters (`BackupMode`, `FetchMode`, `legacy`)
   that the `steps:` examples deliberately omit.
4. **Sign-off point.** Only once 3 is green: delete `plan()`, rename.
   ✅ Done 2026-09-02. `action.rs` lost 98 lines and gained
   `use crate::generated::plan::plan_generated as plan;`, so every call site
   and every test kept working unchanged. The differential test was migration
   scaffolding and went with the code it compared against; it was replaced by
   `every_plan_is_a_subsequence_of_one_canonical_order`, which pins the
   property §1 says the rank model rests on — the thing that has to keep
   holding once the proof is gone.

Stages 1–3 are reversible and add no risk to the execution path — the running
code is still the hand-written planner. Stage 4 is the irreversible one and is
where a human should look at the evidence.

## 6. Verification standard

The differential test must be shown to have teeth: perturb one `rank:` in
`cli.yaml`, regenerate, confirm the test fails naming the offending `Action`.
A test that has only ever passed is not evidence.

`action.rs` is unsigned, so none of this touches a guardian signature.

## 7. Resolved: the question that was open at sign-off

Is generating the planner worth it, given that stage 3's differential test
already makes drift a build failure? The honest case against: declaring and
checking gets most of the value, and the residue is "impossible" versus
"caught loudly". The case for: one artefact instead of two, and `FetchMode`
enters the spec, which closes #22's documentation carry-forward.
