# CLI Spec Audit — Findings

## Section 1: Falsified assertion — `proof.unique_maps_to: true`

The validator never checks this field. The spec also violates it: `top_level_invalid_combo` is referenced by TL-005 and TL-012; `fetch_invalid_flags` is referenced by F-003, F-004, F-005, F-006.

---

## Section 2: Spec-code message drift (present tense)

`reason_templates` text does not match runtime strings. `invalid_flag` template says "unsupported option -{flag} for {command}" but code emits "Unsupported: --legacy only valid with build or run". Templates currently only drive docgen, not runtime. #31 will fix this.

---

## Section 3: Semantic inconsistency — `-f` alone

`xtgeoip -f` is rejected (TL-004). `xtgeoip build -f` and `xtgeoip run -f` are silently accepted (force is a no-op). Decision needed before codegen: apply rule uniformly or explicitly document subcommand looseness.

---

## Section 4: F-003 to F-006 mechanism changed after #28

These are now clap-rejected (unexpected argument), not code-rejected. The `maps_to: fetch_invalid_flags` annotations are unenforceable. When #85+#91 lands, tests will fail. Two options: drop from spec (leave to clap) or restore custom rejection code.

---

## Section 5: Coverage gaps — valid combinations missing from spec

Top-level: `-b -f`, `-b -c -f`. Build: `-b`, `-c`, `-b -c`, `-b -f`, `-b -c -f`. Run: `-b`, `-b -c`, `-b -f`, `-b -p`, `-b -c -f`.

**Superseded, 2026-09-05.** `run -b -p` was wrong in this list. It is not a
valid combination: `run` fetches remotely (`contexts.run.fetch_mode: remote`),
so a new CSV archive exists whether or not a flag asked for it, and `-b` adds a
new binary tarball beside it in the same `archive_dir`. `-p` therefore has two
candidate targets, exactly as `manpage-template.toml` has always said. Adding it
here as *valid* is how R-012 came to contradict the man page. The `run` guard is
now `[b, p]`; see Section 6.

Note: `-b -p -f` was initially listed here for both build and run, but is invalid under the `no_prune_force` rule (`--prune does not support the --force option`). Including it as an invalid case would duplicate the `maps_to` values already used by B-007 (`build_prune_force`) and R-006 (`run_prune_force`), violating `proof.unique_maps_to`. Omitted.

---

## Section 6: Redundant prohibitions — two categories

- Clap-redundant: F-003 to F-006 (post-#28), C-001 (ArgGroup).
- Planner-redundant: R-006 (`run -c -p -f`) and B-007 (`build -b -c -p -f`) — planner produces deterministic plans. R-007 (`run -b -c -p`) is genuinely semantically ambiguous (prune target) and should stay.

**Amended, 2026-09-05.** R-007's ambiguity was real but its *predicate* was
wrong. `-c` plays no part in choosing a prune target; `-b` does. The guard was
copied from the `-f` pattern (`b ∧ c ∧ f`), where both targets are named by
flags — but `run`'s second prune target is implicit in `always: [fetch]`, so the
correct predicate is `b ∧ p`. R-007 (`run -b -c -p`) is now a strict superset of
R-012 (`run -b -p`) firing the identical guard, and `proof.unique_maps_to`
permits one canonical example per error case. R-007 retired; R-012 carries
`maps_to: run_prune_ambiguous` and is the case the man page names. All 136
combinations remain covered by `cli::snapshot`.

---

## Section 7: TODO items touching the spec

cli.yaml content: #22, #29, #31. Spec validation: #92. Testcase YAML schema: #80, #82, #83, #86, #89, #90+#84.

---

## Decisions needed before codegen

1. `unique_maps_to` — enforce (split error cases) or drop claim
2. `-f` alone in subcommands — reject uniformly or accept as valid no-op
3. F-series and C-001 — drop from spec or restore custom code
4. R-006 and B-007 — unblock (per #29) or keep as documented prohibitions
5. Coverage — add missing positive examples
