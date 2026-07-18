# Design note: state, ownership, and recovery (#98, #24, #89)

Status: **DRAFT — awaiting ratification**
Date: 2026-07-18
Covers: #98 (test setup/teardown), #24 (no rollback on mid-pipeline failure),
#89 (orphan scenarios), plus the "reject unknown flags" item carried into #98.
Related: #87 (integration nature, done), the reverted atomic swap (`4909da4`),
[`29-ambiguity-planner-vs-guards.md`](29-ambiguity-planner-vs-guards.md).

---

## 1. Why these are one problem

Three tickets describe the same gap at different levels:

| Ticket | Symptom |
|---|---|
| #24 | A pipeline failure mid-way leaves the system partially destroyed. |
| #98 | A suite failure mid-run leaves arbitrary state; the next run inherits it. |
| #89 | Orphan scenarios can't be tested because state transitions aren't deterministic. |

Each proposes its own machinery — rollback, setup/teardown, `requires:`
annotations. All three presuppose the same missing capability.

## 2. The finding: there is no restore

**`xtgeoip` cannot read its own backups.** `backup.rs` creates `.tar.gz`
archives (`create_tarball`) and prunes them (`prune_archives`), and that is
all. There is no `restore`, no `tar::Archive`, no `GzDecoder` anywhere in the
crate outside `fetch.rs`'s ZIP handling. Recovery today means the operator
running `tar -xzf` by hand.

This is the load-bearing fact:

- **#24's "rollback"** has nothing to roll back *to*. A backup was taken, but
  the program cannot apply it.
- **#98's "teardown restoring a known-good state"** has no mechanism to
  restore with.
- **#89's deterministic transitions** need a way to return to a baseline
  between scenarios.

So the cluster is not three features. It is **one missing primitive plus three
consumers of it**. Designing them separately would produce three ad-hoc
recovery paths — most likely the test suite shelling out to `tar`, which is
both untested and divergent from what the tool itself would do.

## 3. Governing principles

Stated by the user, 2026-07-18, and binding on everything below:

1. **"Do not pop what you did not push."** `build` writes what it was asked to
   write. Files it did not create are not its business to delete.
2. **Never guess intent when the result is data loss.** This is why
   `build -b -c -f` is rejected rather than resolved: `-f` has no unambiguous
   referent with two forceable targets, and `-b -c -f` ≠ `-b -f -c -f`.
   Positional binding was considered and rejected — it relocates the ambiguity
   into "does `-f` bind leftward or globally?", which is worse for looking
   unambiguous.
3. **`build` is not a cleanup function.** `-c` exists for that. The mechanism
   is already invocable two ways; that is a documentation matter, not a design
   gap.

Principle 1 is exactly what the reverted atomic swap violated: `b4ec1db`
`remove_dir_all`'d the whole `output_dir`, destroying files `build` never
created. See §6.

## 4. The ownership model (already implemented — write it down)

The manifest is the ownership record. Three categories, verified empirically
on 2026-07-18:

| Category | Definition | Example | Treatment |
|---|---|---|---|
| **Owned** | listed in the current manifest | 506 `.iv4`/`.iv6`, `version`, the manifest | backed up, cleaned, overwritten |
| **Unowned** | everything else | `xtgeoip.conf.example` | **never touched** |
| **Stale-owned** | in a *previous* manifest, not the current one | `EU.iv4` after a legacy→default flip | detected, listed to `orphaned`, **left for the user** |

`iv_files` enforces this structurally: extension `iv4`/`iv6` **and** a
two-character `[A-Z0-9]` stem. `xtgeoip.conf.example` cannot match.

Two clean paths already exist for stale-owned files, and which one applies
depends on *when* you act — a fact that is currently undocumented and which
the author of this note initially got wrong from reading the code:

- **During the switch back — `build -c`.** `Clean` runs in `pre`, *before*
  `Build` regenerates the manifest, so the manifest on disk still lists `EU`.
  Verified clean removes it. No `-f` required.
- **After the fact — `build -c -f`.** The manifest no longer lists `EU`, so
  the glob is needed. This is the form `detect_orphans` advises, because by
  the time it prints you are already in this case.

**Action:** document this in the `-l` help text and the man page. #89's
premise ("not covered") is wrong about the mechanism — detection exists with
6 unit tests and was demonstrated end-to-end. What is missing is an
*integration* test and the docs above.

## 5. Proposed primitive: `restore`

A first-class restore, respecting the ownership model.

```
xtgeoip restore [--version <v>] [-f]
```

- Default: most recent bin archive; `--version` selects one.
- **Extracts only what the archive owns.** Never deletes unowned files —
  principle 1 applies identically in reverse.
- **Verified** (default): archive must carry a manifest; checksums are
  verified before anything is written. **Force** (`-f`): skip verification,
  consistent with `backup`/`clean` semantics.
- Refuses to run if the archive is absent or fails verification, leaving the
  system untouched.

Open question for ratification: should `restore` *remove* owned-but-not-in-
archive files (i.e. return exactly to the archived state), or only add/
overwrite? Removing is what "restore" implies, but it is deletion, so under
principle 2 it should require explicit opt-in rather than being the default.
Recommendation: add/overwrite by default; exact-state restore behind a flag.

This is a new user-facing command, so it is a `cli.yaml` change: summary,
`allowed_flags`, guards, examples — and therefore generated docs, `CLI_MATRIX`
and testcases come free. The machinery hardened today (#76 `deny_unknown_
fields`, #77 schema version, #92 contradiction checks) makes that safe.

## 6. #24 — reorder before rollback

**Stage 1 — reorder (cheap, no new machinery).** Today:

```
run  -c → [Clean, Fetch(Remote), Build]
```

`Clean` destroys the working data *before* the network fetch — the single most
failure-prone step. A MaxMind outage therefore leaves `output_dir` empty. Move
`Clean` from `pre` to `mid` (after fetch, before build) and a fetch failure
leaves the system untouched.

This is a `plan()` change, unit-testable against the 11 goldens, and verifiable
end-to-end with one `build -c` run — which uses `FetchMode::Local` and so costs
**no MaxMind request**.

It is user-visible (log ordering changes), so it needs ratification. Note it
does not change what was asked for: the user requested a clean and a run; the
relative order of the two was never theirs to specify.

**Stage 2 — rollback (needs §5).** On pipeline failure, restore the backup
taken in `pre`. Only available when `-b` was given; otherwise there is no
snapshot and the honest answer is that recovery is impossible. Do **not**
silently take a backup the user didn't ask for.

**Stage 3 — atomic swap: still rejected.** `#24` records the constraint from
the `b4ec1db` data-loss incident: a swap must respect manifest ownership and
never delete unowned files. But an install step that removed stale-owned files
would be doing cleanup, which principle 3 forbids `build` from doing. Stages 1
and 2 capture the value without that conflict.

## 7. #98 — setup/teardown built on `restore`

With §5 in place:

- **Setup:** `xtgeoip -b -f` at suite start — snapshot whatever is there.
- **Teardown:** `restore` that snapshot, returning the machine to its
  pre-suite state whether the run passed, failed, or aborted.
- **`--rebuild`** then becomes a mitigation for a problem teardown solves
  properly. Whether it can be retired depends on whether individual cases
  still need intermediate rebuilds; decide after teardown exists, not before.

**Independent and cheap — do first:** reject unknown CLI flags. A typo'd
`--rebuil` is currently silent and produces exactly the false
"Nothing to back up" failures the #87 docs now warn about. Root-free,
testable, no dependency on anything above.

## 8. #89 — concrete cases

Now that the cycle is confirmed working, the scenarios are precise:

1. `build -l` → 254 countries; `EU.iv4`/`EU.iv6` present.
2. `build` → orphan warning naming both files; they remain; `orphaned` written.
3. `build -c -f` → 506 `.iv4`/`.iv6`; `EU` gone; `orphaned` gone;
   `xtgeoip.conf.example` **still present** (the unowned-file guarantee).

Case 3's final assertion is the important one: it is the regression test for
the `b4ec1db` data-loss bug.

Requires `requires:`/`rebuild:` support in `testcases.yaml` per #89.

## 9. Sequencing and verification cost

Every test-side change costs a full rate-capped `xtgeoip-tests --rebuild` run
to validate. Therefore: design fully on paper, implement in one pass, validate
**once**.

| # | Work | Verification | Cost |
|---|---|---|---|
| 1 | Reject unknown flags | unit tests | free |
| 2 | Document the two orphan paths (§4) | docs | free |
| 3 | #24 stage 1 reorder | goldens + one `build -c` | free (Local) |
| 4 | `restore` primitive | unit tests on temp dirs | free |
| 5 | #98 setup/teardown | needs 4 | — |
| 6 | #89 cases | needs 4, 5 | — |
| 7 | **Single live validation run** | `xtgeoip-tests --rebuild` | **1 MaxMind fetch** |

Items 1–4 are individually verifiable without touching the server. Only 5–7
require the live run, and they should be batched into it.

## 10. Rejected

- **Teardown shelling out to `tar`.** Duplicates recovery logic outside the
  tool, untested, and diverges from what `xtgeoip` itself would do.
- **Auto-deleting orphans during `build`.** Violates principles 1 and 3.
- **Atomic swap** (§6 stage 3).
- **Implicit backup before destructive steps.** Taking a snapshot the user did
  not request is its own surprise, and silently consumes disk.

## 11. Decisions needed before implementation

1. Ratify the `restore` primitive, and its default: add/overwrite, with
   exact-state restore behind a flag? (§5)
2. Ratify #24 stage 1 (moving `Clean` after `Fetch`) — it changes observable
   step order. (§6)
3. Confirm the sequencing in §9, in particular batching 5–7 into one live run.
