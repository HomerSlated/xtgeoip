# 98 — Test isolation: temp-dir redirection, per-case disk footprint, and the WAN floor

Status: **analysis, not a plan**. Written 2026-09-05. Nothing here is
implemented. Three of the findings below change what #98 actually is, and one
of them needs a decision that is not mine to make (§5).

The motivating instruction: *testing should not modify production files*, and
*there is no justifiable reason to clobber MaxMind's server at all*.

---

## 0. Summary of findings

1. **The suite cannot run on a clean system today.** It is green only because
   it inherits populated production directories. `TL-006` (`xtgeoip -b`) is the
   sixth case and the first that needs a populated `output_dir`, and no case
   before it builds anything. §3.
2. **41 of 51 cases already reach no network.** 21 are rejected at argument
   validation before any I/O; the entire `build` context uses
   `fetch_mode: local`; `top_level` has no fetch step at all. Only **10** cases
   reach the WAN. §2.
3. **Redirecting `output_dir` / `archive_dir` is a config change, not a code
   change** — both are `[paths]` keys. But the config *path* is hardcoded, so
   reaching those keys is the whole problem. §4.
4. **Redirecting the paths removes the root requirement at the filesystem
   level but not at the program level.** `Action::requires_root()` is a blanket
   euid test on everything except `conf`, independent of where the paths point,
   so a temp tree alone is not enough. §4(d).
5. **A local https stub needs no production change.** `reqwest` here resolves
   to `rustls-platform-verifier` → `rustls-native-certs`, so the binary
   verifies against the *system* trust store. §5.

---

## 1. What each step does on disk

Six steps, ranked; every case's sequence is a subsequence of this order
(`docs/spec/cli.yaml`, `plan.steps`).

| Rank | Step | Reads | Creates | Deletes / overwrites |
|-----:|------|-------|---------|----------------------|
| 10 | `backup` | `output_dir` + its manifest | tarball in `archive_dir` | — |
| 20 | `prune_bin` | `archive_dir` | — | binary tarballs beyond `archive_prune` |
| 30 | `fetch` (remote) | network | `GeoLite2-Country-CSV_<v>.zip` + `.sha256` in `archive_dir`; extracts to a `TempDir` | re-downloads on checksum mismatch |
| 30 | `fetch` (local) | `archive_dir` | extracts to a `TempDir` | — |
| 40 | `clean` | `output_dir` manifest | — | the owned `.iv4`/`.iv6` files it lists |
| 50 | `prune_csv` | `archive_dir` | — | CSV zips beyond `archive_prune` |
| 60 | `build` | the `TempDir` from fetch | `.iv4`/`.iv6` + manifest in `output_dir` | overwrites the previous generation |

Two steps have a **hard precondition** and fail loudly without it:

- `backup` → `backup.rs:381`, `bail!("Nothing to back up")` when
  `gather_files` returns empty. Needs a populated `output_dir`.
- `fetch` (local) → `find_latest_local_csv_archive` ends
  `"No valid local GeoLite2 Country CSV archive found in …"`. Needs a
  `GeoLite2-Country-CSV_*.zip` already in `archive_dir`.

Everything else degrades to a no-op on an empty tree: both prunes succeed with
nothing to prune, and `clean` succeeds with an empty manifest.

**So the entire ordering coupling reduces to those two preconditions.** That is
a much smaller thing than "the suite is stateful", and it is the reason this is
tractable at all.

---

## 2. Per-case footprint, derived from the spec

Derived mechanically from `plan.contexts` in `cli.yaml` crossed with
`docs/generated/testcases.yaml`, not from prose. `fetch_mode` per context:
`top_level` has no fetch, `build` is **local**, `fetch` and `run` are
**remote**, `conf` has no fetch.

| Group | Cases | Network | Notes |
|-------|-------|---------|-------|
| Rejected at argument validation | 21 | none | exit before any I/O; need no fixture and no config beyond one that loads |
| `-h`, `conf -s/-d/-e/-c` | 5 | none | read/write config only |
| `top_level` (`-b`/`-c`/`-p`) | 5 | none | `backup` needs a populated `output_dir` |
| `build` context | 9 | **none** — `fetch_mode: local` | needs a CSV zip in `archive_dir` |
| `fetch` + `run` contexts | **10** | **remote** | `F-001 F-002 R-001…R-005 R-009…R-011` |

The ten WAN cases are the whole of the network exposure. The other 41 are
already offline by construction — that is a property of the spec, not an
accident, and it was invisible while the suite ran as one undifferentiated
root-only run.

### The WAN floor, stated exactly

In `FetchMode::Remote`, the version is **header-derived**: `fetch()` issues
`GET {url}?suffix=zip` and reads the version out of `Content-Disposition`
(`resolve_version`) *before* it can decide whether the local copy is current.
There is no local shortcut on that path.

So with a real MaxMind URL the floor is **ten requests, not one** — one archive
download plus nine header-only round-trips that hit `verify_cached_archive` and
return `"Reusing verified local copy"`. Redirecting the URL is the only way to
get below ten. That is the honest floor, and it is why §5 matters.

---

## 3. The suite requires production state as an implicit fixture

Case order is pinned (`emission_order_is_stable`, #77) and runs
`TL-*` → `B-*` → `C-*` → `F-*` → `R-*`. Consequences on a genuinely clean
system:

- `TL-006` (`xtgeoip -b`) is the **first** case needing a populated
  `output_dir`. Nothing before it builds. It fails with `Nothing to back up`.
- `--rebuild` cannot repair this. It runs `xtgeoip build`, which is the `build`
  context, which is `fetch_mode: local`, which needs a CSV zip in
  `archive_dir` — and on a clean system there is none. `--rebuild` fails too.
- The first case that could produce either fixture is `B-001`, at position 16,
  and it needs the CSV fixture itself. The first case that can produce anything
  from nothing is `F-001`, at position 34.

The suite is therefore not merely order-dependent; it is **dependent on state
it cannot itself create**, and the docs' framing (`--rebuild` "keeps
`output_dir` populated between cases") describes maintenance of a fixture whose
*origin* is the operator's real installation.

This reframes #98. "A known-good initial state" is not a nicety on top of a
working suite — it is the thing that makes the suite self-contained for the
first time. It also explains why every previous attempt tangled with `restore`:
without a setup phase, the only available source of a known-good state was the
production system.

### What a setup phase has to produce

Exactly two artefacts, and one operation produces both:

1. a `GeoLite2-Country-CSV_<v>.zip` (+ `.sha256`) in the temp `archive_dir`;
2. a populated temp `output_dir` with a valid manifest.

`xtgeoip run` does both in one invocation: remote fetch writes (1), build
writes (2). **One command, one WAN hit, both fixtures.** Everything downstream
of that is either offline already or made offline by §5.

Teardown then becomes trivial rather than a lifecycle problem: the temp tree is
removed. A mid-run failure poisons nothing, because the next run builds a new
tree. This is the half of #98 that was left "unresolved" — and under temp-dir
redirection it stops being a design problem and becomes `drop(TempDir)`.

---

## 4. Reaching `[paths]` — the real obstacle, and the fork

`archive_dir` and `output_dir` are ordinary config keys. Pointing them at a
temp tree needs no code change *if the binary can be made to read a different
config*. It cannot today: `config.rs:10` is
`pub(crate) const SYSTEM_CONFIG: &str = "/etc/xtgeoip.conf";` with no override.

Note the comment at `config.rs:252` is **not** an argument for that being
hardcoded — it says the hardcoding is what makes the
`errors_never_echo_config_source` invariant directly testable. It constrains
error text, not configurability. I found no recorded decision that the path
*must* stay fixed.

Three routes, and this is a decision I am explicitly not taking:

**(a) A `--config PATH` global option.** *Corrected 2026-09-05 — the first
draft of this section costed this wrongly.* It claimed the option would widen
the flag space and regenerate `testcases.yaml`, "the corpus this work exists to
fix". It would not. `cli.yaml` already has a **`global_options:`** section,
deliberately separate from `flags:`, for exactly this shape — options that
"apply to every command and carry no combination semantics". The comment there
states the reason: `flags:` is the universe the guard bitmask is built from, and
`every_flag_is_referenced_by_some_guard` requires each bit to be mentioned by
some guard, so a non-constraining option cannot live there.

`--log-file` and `--no-log` are the existing precedent. Measured: they appear
**zero** times in `docs/generated/testcases.yaml`, `src/generated/cli_matrix.rs`
and `src/generated/cli_rules.rs`. A third entry would behave the same — the
5-bit space stays 32 combinations per context, the 136-case snapshot is
untouched, and the corpus is not regenerated. The cost is one spec entry plus
the man-page documentation that `cli::tests::global_options_are_documented`
already enforces.

Decisively, an **argument passes through `sudo` unchanged**, which is the
property (b) lacks.

**(b) An environment-variable override**, e.g. `XTGEOIP_CONFIG`, read by
`load_config`. Small and needs no spec change — but it does not survive `sudo`,
and cases are spawned via `sudo` today. `sudo` runs with `env_reset` on by
default; passing `VAR=value` inline requires the sudoers `setenv` option, and
`sudo -E` requires the `SETENV` tag — neither is on by default on Debian or
Ubuntu. So this route needs **a sudoers change on every host that runs the
suite**, which is a far worse prerequisite than the one it removes. (Not
verified on this host: `sudo -n` requires a password here, so it could not be
tested without one. Stated from sudo's documented behaviour, not observation.)

It becomes viable only if `sudo` leaves the spawn path entirely — see (d).

**(c) A bind mount** over `/etc/xtgeoip.conf` in a private mount namespace —
zero production change, already recorded at `TODO.md:506`. Blocked here:
`kernel.apparmor_restrict_unprivileged_userns = 1` means it needs root, and
needing root is one of the things we are trying to remove.

**(d) Make the root check reflect what it actually guards.** Added after (b)'s
`sudo` problem surfaced the reason `sudo` cannot simply be dropped:
`Action::requires_root()` is `!matches!(self, Action::Conf(_))` — a **blanket
euid test on every command except `conf`, independent of where the paths
point**. So even with a temp tree owned by the invoking user, `xtgeoip build`
refuses to run. §4's premise that redirecting `[paths]` removes the root
requirement is therefore true of the *filesystem* and false of the *program*,
until this check changes.

The check asks "am I root?" where the question is "can I write to the
configured `output_dir` and `archive_dir`?". On a default install the two
coincide; on any other configuration the tool refuses work it could do. This
repo already has the right pattern: `conf.rs::check_system_config_writable`
probes writability by actually attempting `NamedTempFile::new_in(dir)` and
treats root as *advice* in the error text ("Re-run as root (e.g. with sudo)")
rather than as a gate. `requires_root` has **no test coverage** — one call site,
no tests — so it is also the least-pinned thing being proposed here.

This is a small correctness improvement on its own merits, independent of
testing. It is also the only route that removes `sudo`, and removing `sudo` is
what makes the suite CI-runnable.

**Recommendation: (a), and (d) on its own merits.** (a) is immune to sudo
policy, costs one `global_options` entry, and is the only route that works
without touching either host configuration or the root check. (d) is worth
doing regardless — it fixes a real if minor defect and would let the suite drop
`sudo` — but it is a behaviour change to a security-relevant check and should
be decided separately, not smuggled in as test infrastructure. (b) is dead
unless (d) lands first. The maintainer's call stands on whether a config
override belongs in the shipped tool at all; the earlier draft overstated its
cost, and this is the correction.

### Root does *not* fall out for free

An earlier draft said it did. It does not. `/usr/share/xt_geoip` and
`/var/lib/xt_geoip` being root-owned is only half the reason root is required;
the other half is `Action::requires_root()`, which tests euid and nothing else
(§4(d)). Under a temp tree the *filesystem* no longer needs root and the
*program* still insists on it.

With (d) as well, the cases write as the invoking user, `sudo` leaves the spawn
path, and `check_preconditions`' root check becomes conditional rather than
absolute — which removes the passwordless-sudo requirement and makes the suite
runnable in CI, which it has never been. Worth noting that requirement is not
hypothetical: on the maintainer's own machine `sudo -n` requires a password, so
`sudo_is_passwordless()` returns false and `check_preconditions` refuses the
run. The suite cannot execute there today without an interactive password.

---

## 5. Simulating the ten remote cases

`Config::validate` rejects any non-https URL, **including loopback**, and this
is a decided position with a test behind it
(`http_loopback_is_also_rejected`, `config.rs`): *"the decision was 'no
exception', so a local http mirror must be fronted with https rather than
special-cased."* Pointing `maxmind.url` at `http://127.0.0.1:PORT` is therefore
already rejected here, and I am not proposing to weaken it.

The comment tells us what is wanted instead: **front the stub with https.** The
dependency chain makes that cheap:

    reqwest 0.13.4 → rustls-platform-verifier 0.7.0 → rustls-native-certs 0.8.4

`verification/others.rs` (the non-Apple, non-Windows, non-Android path — i.e.
Linux) calls `rustls_native_certs::load_native_certs()`, and that crate resolves
roots from **`SSL_CERT_FILE` / `SSL_CERT_DIR` when either is set**, falling back
to the platform store otherwise (`rustls-native-certs-0.8.4/src/lib.rs:8`,
`CertPaths::from_env`).

That is better than modifying the system trust store, and the earlier draft of
this section had it wrong. Nothing needs installing: the runner generates a CA
into the temp tree and sets `SSL_CERT_FILE` for the spawned process only. It is
**per-process, reversible, needs no root, and leaves the host's trust
configuration untouched** — and the no-exception https rule stays fully intact,
because the stub really is serving https and really is being verified.

The same `sudo`-strips-the-environment trap from §4(b) applies to
`SSL_CERT_FILE`: under the temp-dir design `sudo` leaves the spawn path, so it
does not bite — but if `sudo` is retained for any reason, this variable must be
passed inline or it silently falls back to the real trust store, and the stub
then fails to verify.

**Not yet verified empirically.** The above is read off the dependency source,
not observed. Before anyone builds on it, confirm end to end: generate a CA,
serve https on loopback, point a release `xtgeoip` at it with `SSL_CERT_FILE`
set, and check the fetch completes. That is a five-minute check and it is the
difference between "the code path exists" and "it works here".

The stub serves two endpoints, which is the entire protocol:

- `GET {url}?suffix=zip` → the canned archive, with `Content-Disposition`
  carrying the version and `Content-Length` for the size guard
- `GET {url}?suffix=zip.sha256` → its checksum

A mock server of exactly this shape already exists in `src/fetch/tests.rs`
(`TcpListener::bind("127.0.0.1:0")`, ~893 lines, used by the #88/#101/#102
tests). It runs over plain http because `fetch()` is deliberately
scheme-agnostic — the https rule lives in `Config::validate`, not in the client
(`config.rs:87`). So the reusable part is the request/response shaping; the
new part is the TLS front and the trust setup.

### The resulting exposure

| | Today | Proposed |
|---|---|---|
| Cases hitting MaxMind | 10 | **1** (setup only) |
| Archive downloads | 1 | 1 |
| Header-only round-trips | 9 | 0 |
| Production dirs written | yes | none |
| Root required | yes | no |

The one remaining hit is the setup fetch in §3, which is also the thing that
proves the download path genuinely works — the user's stated requirement. Once
the canned archive exists it can be cached between runs, at which point a
`--offline` run is possible with **zero** WAN contact; the real fetch then
becomes a deliberate, occasional check rather than a per-run cost.

---

## 6. What this does not settle

- **Which of §4's three routes.** Decision required before any code.
- **Whether the canned archive is committed or cached.** It is MaxMind's data;
  redistribution terms need checking before anything is committed, and
  `.gitignore`'s `*.pdf`-style privacy guards suggest this repo takes that
  seriously. A cached fixture outside the tree avoids the question entirely.
- **Whether `--rebuild` survives.** Under a setup phase its purpose largely
  evaporates, but the cases marked `rebuild: true` encode a real
  "`output_dir` was just emptied" fact that something must still handle.
- **Corpus order.** Nothing here proposes reordering; the pinned order
  (#77) is preserved, and the fixture is established *before* case 1 rather
  than by reordering cases.
- **Verification.** The first full run of any of this still costs one real
  `xtgeoip-tests` pass against live MaxMind to prove the temp tree behaves as
  the production tree did.
