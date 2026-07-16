# Design note: #29 — "ambiguity checks have no formal basis"

Status: **RATIFIED** (user, 2026-07-16) — #29 is **CLOSED** on recommendation (a).
Date: 2026-07-16
Related: #9/#26/#27/#34 (spec-driven arc), #22 (FetchMode→spec),
[`spec-driven-validator.md`](spec-driven-validator.md) (the validator this note
concludes against).

---

## 1. What #29 asked

> cli.rs: ad hoc ambiguity checks (`if *prune && *force && *clean`, etc.) have no
> formal basis. "Ambiguous" is undefined. A combination is ambiguous iff the
> execution planner cannot produce a deterministic `Vec<Step>`. Remove the current
> checks once the planner exists; let inability to plan be the rejection signal.

Two claims: (1) ambiguity is defined ad hoc; (2) the fix is to make the *planner*
the arbiter — reject a combination exactly when `plan()` can't produce steps.

## 2. What changed since #29 was filed

The spec-driven validator (v0.2.0, `spec-driven-validator.md`) shipped. It deleted
the ad-hoc `if`-chains and replaced them with **declarative guards** in
`cli.yaml` (per-context ordered `require`/`forbid`/`error`), lowered to a `u8`
bitmask table in `src/generated/cli_rules.rs` and evaluated by a 3-line
`first_guard` at runtime. "Ambiguous" now has a precise, single definition: **a
flag-set is invalid iff the first matching guard fires**, and the
`cli::snapshot` test proves the flags→`Action` function over all 136 combinations,
byte-for-byte.

So claim (1) — "no formal basis" — is **answered**. It just wasn't answered the
way #29 imagined (planner-inability); it was answered by declarative guards.

## 3. The fork

- **(a) Close #29.** The guards are the formal basis #29 wanted. Done.
- **(b) Planner-as-arbiter.** Make `plan()` partial (`Result<Vec<Step>>`),
  operate on raw flags, move ambiguity detection into it, retire the guard layer.

## 4. Recommendation: (a)

**(b) moves in the wrong direction — twice over.**

- It moves validity *backward*: from declarative spec (`cli.yaml`) into imperative
  Rust (`plan()`'s `match`). The validator's whole point was to make validity
  declarative and drift-proof; (b) undoes that.
- It isn't the destination either. The north star (#26/#27) is **spec-derived
  planning** — the step sequence declared in `cli.yaml` and generated, declarative
  all the way down. Planner-as-arbiter is imperative, so it's not a step toward
  #26/#27; it's a step away.

`plan()` also runs *after* `Action` construction, on an already-valid `Action`
(there is no `Action` for an invalid combo — the guards saw to that). Making it the
arbiter would require inverting that: feeding it raw flags and having it re-derive
validity. That re-creates the two-sources problem the validator removed, now
between guards and planner.

(a) is the consistent close: the guards are declarative, spec-owned, and
exhaustively proven; #29's literal complaint is satisfied.

## 5. The residual #29 was gesturing at (redirect, don't drop)

#29's instinct — "one thing should decide validity" — points at something real,
just not what it named. There are two *independent* hand-maintained semantics
layers: guards (flags→`Action`) and `plan()` (`Action`→`Vec<Step>`). The validator
+ snapshot pin the first. The second is exercised end-to-end by the integration
suite (every TL/B/R case runs flags→Action→**plan→execute**), so it demonstrably
works — but:

- **No unit assertion pins `plan()`'s `Vec<Step>` per `Action`.** A future edit
  could reorder or drop a step silently; only a live root+MaxMind integration run
  would catch it. A cheap golden-per-Action unit test would close this.
- **`action.rs:212` `.expect("Build step requires prior Fetch")` is an invariant
  maintained by construction** (every `plan()` arm that emits `Build` emits
  `Fetch` first). It is unreachable today — a maintainability note, not a latent
  bug. It could be a type/construction guarantee instead of a runtime assertion.

Neither is an argument for (b); both are small hardening tasks that stand on their
own. The *proper* version of #29's "one source" instinct is the #26/#27 endpoint:
derive the plan from the spec too, so guards and steps share one declarative
origin and cannot drift.

## 6. Disposition (ratified 2026-07-16)

1. **#29 CLOSED** on recommendation (a). ✅
2. Redirected follow-ups:
   - ✅ **DONE** — unit-pin `plan()`'s step sequence per `Action`. 11 golden
     tests in `action.rs` assert each plan's `Debug` form, plus
     `build_is_always_preceded_by_fetch`, which sweeps every flag combination to
     pin the invariant behind `execute_step`'s `.expect(...)` (§5).
   - **OPEN** — make Fetch-before-Build a construction/type guarantee rather
     than a runtime `expect`. Lower priority now that the sweep guards it.
3. Spec-derived planning (the declarative unification of validity and steps)
   remains the #26/#27 endpoint — the direction #29 should have pointed.
