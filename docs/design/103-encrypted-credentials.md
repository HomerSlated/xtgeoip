# Design note: encrypted MaxMind credentials at rest (#103)

Status: **IMPLEMENTED (2026-07-20)** — all design decisions ratified (§8)
and built. One call-site correction made during implementation: §7 said
`secrets::decrypt` would be called from `fetch.rs`; it is actually called
from `action.rs`, one level up, because `fetch.rs`'s existing mock-HTTP
unit test suite constructs plaintext credentials directly and has no
terminal to prompt on — decrypting inside `fetch()` itself would have
broken all of those tests. `fetch()`'s signature changed to take
already-decrypted `account_id`/`license_key` as plain `&str` instead of
reading `Config` directly; everything else in §7 (module boundary,
`secrets.rs` owning the crypto) is unchanged. Guardian audit of
`fetch.rs`/`secrets.rs` still pending — see TODO.md.
Date: 2026-07-20
Covers: #103 (new — plaintext MaxMind credentials in `/etc/xtgeoip.conf`).
Related: #102 (https-only `maxmind.url`, done — that closed the *transport*
leak; this closes the *storage* leak), [[credential-handling]] (the
`conf --show` workaround this note is meant to make unnecessary),
[[testing-boundaries]] (MaxMind rate cap — no live fetch needed to validate
this design; it is entirely local crypto plus one interactive prompt path),
#93 (`config.rs`/`conf.rs` split — the direct precedent for §7's module
boundary).

---

## 1. The problem, precisely

`/etc/xtgeoip.conf` holds `maxmind.account_id` and `maxmind.license_key` as
plaintext TOML strings. `xtgeoip` must run as root, so the usual answer —
"restrict file permissions" — only defends against a *co-resident,
lower-privileged* attacker. It does nothing against an attacker with root on
a **different** host who can mount this VPS's disk directly: a stolen
snapshot, a hypervisor-level neighbour with block access, a backup that
leaves the machine. On unencrypted storage, `0600` is not a security boundary
against that adversary — it is not even friction.

`conf --show` compounds this today: it is the direct route by which the
plaintext key ends up somewhere it shouldn't (see
[[credential-handling]] — this happened once already, in this project, to
the person building it).

## 2. The proposal, and the correction it needed

The user's initial framing was a shadow-password analogy: hash the
credentials with a salt, "one-way encryption that requires a password to
decrypt." That doesn't survive: a one-way function is one-way *by
definition* — shadow hashing works for login because authentication only
ever needs to **compare** a candidate against a stored hash, and the real
password is never needed again. `fetch` is the opposite case: it must
**reproduce** the original `license_key` in full and hand it to MaxMind as
HTTP basic auth. A hash cannot give that back, salted or not, no matter what
password is supplied at the other end.

The correct primitive family is **password-based encryption (PBE)**:
reversible, symmetric encryption whose key is derived from a
user-supplied passphrase via a slow, memory-hard KDF. Every design goal in
the original proposal survives the correction intact — it was the right
instinct, wrong primitive.

## 3. Chosen primitive (ratified 2026-07-20)

| Stage | Choice | Why |
|---|---|---|
| KDF | **Argon2id** | Memory-hard — expensive to parallelise on GPU/ASIC, current best practice for password-derived keys. New dependency (`argon2`), accepted deliberately over reusing `sha2` for PBKDF2, which would avoid the new dependency but is materially weaker against offline brute-force of a captured ciphertext blob. |
| Cipher | **XChaCha20-Poly1305** (`chacha20poly1305` crate, `0.11.0`) | Authenticated encryption: detects tampering as well as hiding the plaintext. Encrypts the Argon2id-derived key over `account_id` and `license_key`. Ratified 2026-07-20 over AES-256-GCM — see §3a. |
| Fields in scope | **Both `account_id` and `license_key`** | Ratified as uniform treatment over "license_key only." MaxMind's docs treat `account_id` as the basic-auth username rather than a secret, so encrypting it too is stricter than necessary — but it keeps the on-disk format and the mental model uniform (everything under `[maxmind]` is opaque ciphertext, no field-by-field reasoning about what's "sensitive enough"). |

## 3b. TOML shape (ratified 2026-07-20)

**Encoding: hex, not base64.** Every existing case in this codebase of a
binary blob appearing in a text file is already lowercase hex —
`blake3::hash(&data).to_string()` (`backup.rs`), `format!("{:x}",
hasher.finalize())` / `format!("{:x}", Sha256::digest(&data))` (`fetch.rs`)
— and none of it needed a crate: `blake3::Hash`'s `Display` and the
`digest` crate's `LowerHex` impl give it for free. Introducing `base64`
solely for this blob would be a new dependency to buy compactness this
design doesn't need (salt + nonce + ciphertext together is ~150–200 hex
chars). A hand-written encode/decode for an arbitrary `&[u8]` is a few
lines either direction, no crate required.

**Structure: a nested `[maxmind.credentials]` table**, not an inline table
— `config.rs` already gives every struct its own `[section]`
(`[paths]`, `[logging]`, `[processing]`); a dotted sub-table matches that,
an inline table would be the odd one out in this file.

**One combined plaintext, one nonce, one ciphertext — not two separate
encrypt calls.** This closes an ambiguity §3a's own phrasing left open:
"the nonce is used exactly once per key, ever" is only literally true if
`account_id` and `license_key` are encrypted together under one nonce. Two
separate `.encrypt()` calls per field under the same derived key would
still be safe (XChaCha20-Poly1305's 192-bit nonce tolerates it easily), but
it would make §3a's stated invariant imprecise rather than exact.
Concretely: serialize `{account_id, license_key}` with `serde_json`
(already a dependency — no new one needed) and encrypt that single byte
string once.

**Store the Argon2 cost parameters (`m_cost`/`t_cost`/`p_cost`) explicitly;
no separate `kdf` name field.** The cost parameters are a real, foreseeable
need — recommended Argon2id parameters get revised as hardware improves,
the same reason bcrypt/scrypt/Argon2's own PHC string all embed their
parameters rather than hardcoding them in the verifier: hardcoding today's
numbers in code would mean a later bump breaks every previously-encrypted
config. A `kdf = "argon2id"` field, by contrast, is speculative — this
design has already committed to Argon2id specifically (§3), and a wholesale
algorithm change would be a format migration regardless of one string
field.

Resulting shape:

```toml
[maxmind]
url = "https://download.maxmind.com/geoip/databases/GeoLite2-Country-CSV/download"

[maxmind.credentials]
m_cost = 19456
t_cost = 2
p_cost = 1
salt = "<hex>"
nonce = "<hex>"
ciphertext = "<hex>"   # AEAD output over serde_json {"account_id":..,"license_key":..}
```

`#[serde(deny_unknown_fields)]` on the new `Credentials` struct, consistent
with `#76`'s hardening already applied to every other config-adjacent
struct. Exact Argon2id parameter *values* are an implementation-time detail
(confirm current OWASP-recommended figures then, rather than pin a number
in this note that could go stale before implementation).

**Adjacent, worth stating plainly:** today's `MaxMind` struct derives
`Debug` directly over plaintext `account_id`/`license_key` — a second,
independent path to the same class of leak as `conf --show` (any stray
`{:?}`, `log::debug!`, or panic message). Once the struct holds only cost
parameters and hex ciphertext, `Debug` on it is genuinely safe to keep —
nothing printable is secret. The actual protection (`secrecy`'s refusal to
derive `Debug`, §5) only needs to guard the *decrypted*, in-memory value in
`fetch.rs`, which is where it was already planned to live.

## 3a. AEAD choice — XChaCha20-Poly1305 over AES-256-GCM (ratified 2026-07-20)

Checked live rather than reasoned from memory (2026-07-20): both `aes-gcm`
and `chacha20poly1305` are RustCrypto/AEADs, currently `0.11.0`, with
123.7M / 66.8M crates.io downloads respectively. `aes-gcm` carries one
advisory, **RUSTSEC-2023-0096** (MEDIUM) — plaintext exposure on tag-
verification failure — but it is scoped to `decrypt_in_place_detached` in
`0.10.0`–`0.10.2`, patched in `0.10.3`; irrelevant here regardless, since
this design uses the plain `Aead::encrypt`/`decrypt` trait methods, not the
in-place-detached variant. `chacha20poly1305` has no advisory on record.

The usual deciding factor — AES-GCM's 96-bit nonce needs the classic
"don't reuse a nonce under a key" discipline, which XChaCha20-Poly1305's
192-bit nonce makes practically moot — **does not bind on this design's
actual usage pattern**. Every encryption of the credential blob happens
under a *freshly derived* key (new random salt → new Argon2id output, at
every credential set/rotation event), so the nonce is used exactly once per
key, ever, by construction. Neither cipher's nonce-collision risk is
reachable at the volumes this tool will ever see.

With the usual argument neutralised, the remaining tie-breaker is
which primitive stays safe if a *future* change violates today's
"one encryption per key" assumption — e.g. someone later reuses a derived
key to encrypt a second field, or a bug produces a non-random nonce.
XChaCha20-Poly1305's wide nonce space stays safe unconditionally; AES-GCM's
safety would depend on that assumption continuing to hold. Chosen for the
same reason this codebase prefers structurally-guaranteed invariants
elsewhere (e.g. `ResolvedOutcome` not implementing `Display`) over
correctness that depends on a currently-true assumption. Performance is not
a factor at this scale (a ~100-byte blob, encrypted once per `fetch`
invocation) — AES-NI acceleration buys nothing worth trading for.

**AES-256-GCM-SIV** was considered and set aside: it gets misuse-resistance
*and* AES-NI speed, but it's a smaller, less-established crate solving a
problem this design's key-rotation discipline already avoids by
construction.

**Argon2 pin note:** stable is `0.5.3`; `0.6.0` exists only as an `-rc`
prerelease as of this check. Pin to `0.5.3` — same discipline already
applied to `serde-saphyr`.

## 4. What this defends against, and what it does not

Stated explicitly so the design doesn't quietly overclaim, and so a future
reader doesn't mistake this for a general anti-tampering mechanism:

- **Defends: data at rest.** A disk image, snapshot, backup tape, or another
  root context reading the block device directly gets `{salt, nonce,
  ciphertext}` and nothing else. Recovering the credentials means brute-
  forcing Argon2id against whatever passphrase strength the operator chose —
  a fundamentally different cost than reading a file.
- **Does not defend: an active attacker with a root shell on the live,
  running system while `fetch` is executing.** The decrypted key exists in
  that process's memory for the duration of the MaxMind request, and a root
  attacker can read process memory directly — no cracking required. §5's
  memory hygiene narrows the window and prevents *incidental* leakage
  (swap, core dumps, log/Debug output) but cannot defend against a
  deliberate, contemporaneous root-level attacker. That is not this design's
  job; it is not solvable at this layer.

The boundary being drawn is exactly the one the VPS threat model calls for:
disk-level compromise, not live-system compromise.

## 5. Memory hygiene — correcting the `shm` proposal

The user asked about Linux "secure RAM" (`shm`) for handling the decrypted
key. That's the wrong mechanism for this problem, worth correcting before it
lands in an implementation:

- **`/dev/shm` is tmpfs — RAM-backed, not swap-proof.** tmpfs pages are
  ordinary page-cache pages and can still be paged out under memory
  pressure. `shm` solves an IPC problem (sharing a segment between
  processes); it does not solve "keep this out of swap."
- **What actually prevents swapping**: `mlock`/`mlockall` on the specific
  buffer holding the passphrase and the derived key, so the kernel is
  forbidden from paging that page out.
- **What actually prevents the compiler eliding a zero-out**: a plain
  `buf.fill(0)` has no effect observable after the buffer's last real use, so
  an optimizing compiler is entitled to remove it. The `zeroize` crate exists
  specifically to force a volatile write the optimizer cannot skip.
  `secrecy` builds on `zeroize` and additionally suppresses accidental
  `Debug`/`Display`/`Clone` of the wrapped value — closing off the "it got
  logged by accident" class of leak, which is the same failure mode as the
  `conf --show` incident this whole note is a response to.
- **Passphrase entry without terminal echo**: `rpassword` is the standard
  crate; no reason to hand-roll `termios`.

**`secrecy` over hand-applied `zeroize` alone (ratified 2026-07-20).**
Checked live: `secrecy` `0.10.3`'s `SecretBox`/`SecretString` requires
`T: Zeroize` internally, so choosing `secrecy` does not avoid needing
`zeroize` — it adds one crate on top of it, not instead of it. What that one
extra crate buys: `SecretBox` implements neither `Debug`, `Display`,
`Clone` (opt-in via `CloneableSecret`), nor `Serialize` (opt-in via
`SerializableSecret`), and the *only* way to reach the wrapped value is an
explicit `.expose_secret()` call via `ExposeSecret`. `zeroize` alone only
clears memory on drop — it does nothing to prevent the value being printed,
logged, or re-serialized *while still alive*, which is precisely the
failure mode behind the actual `conf --show` incident this design exists to
close (see [[credential-handling]] / `docs/design/103…` intro): that was a
printing bug, not a clearing bug, and `zeroize` would not have caught it.
`secrecy`'s refusal to implement `Debug` turns that class of mistake into a
compile error. Also gives the mandatory guardian re-audit of `fetch.rs` a
precise, grep-able question to check: does every `.expose_secret()` call
feed directly into the MaxMind `basic_auth` call and nowhere else?

**Boundary, so it isn't conflated with `mlock`:** `secrecy` (and `zeroize`)
address code-path exposure and drop-time clearing. Neither touches swap —
`mlock`/`mlockall` remains a separate, still-necessary layer for keeping the
live secret out of swap while it exists. Picking `secrecy` does not retire
the `mlock` item above.

New dependency surface this adds, beyond the KDF/cipher in §3: `zeroize`,
`secrecy`, `rpassword`.

## 6. Operational consequence — this is permanent, not a gap to fix later

An interactive passphrase prompt makes unattended `cron` invocation of
`fetch` impossible **by construction**. This is presumably the point — the
user raised it as the reason plaintext-in-config is unacceptable in the
first place — but it needs to be written down as an *intentional, permanent*
constraint. The failure mode to guard against is someone later "fixing" the
lost automation by stashing the passphrase in a file next to the config,
which would silently defeat the entire scheme and put the project back to
worse than where it started (plaintext credentials, now with an extra layer
of false confidence). If unattended operation is ever wanted, that is a
separate, later decision — e.g. `systemd-creds`, a TPM-sealed key, or an
agent process — and out of scope here.

## 7. Where the decrypt happens (ratified 2026-07-20)

**A new sibling module, `secrets.rs` — not inline in `fetch.rs`.** Two
independent arguments:

1. **`config.rs` already drew this exact boundary, for the same reason.**
   Its own header states: *"Pure: no output, no subprocesses, no prompts —
   see `conf.rs` for the conf subcommand handler."* `conf.rs`'s header:
   *"configuration management actions (show / edit / default),
   preconditions, and interactive creation."* `#93` split those two
   concerns apart already. Decrypt needs `rpassword`'s interactive prompt —
   the same kind of thing `config.rs` refuses to own, and `fetch.rs` has no
   stated exception for either (its role per `CLAUDE.md`'s module table is
   HTTP semantics only). Putting crypto/prompt logic in `fetch.rs` would be
   a second, undeclared exception to a boundary already paid down once.
2. **Guardian re-audit blast radius, mechanically — not just stylistically.**
   A signature covers the *whole file*, not the diff. Inlined, a KDF-
   parameter bump years from now forces a re-audit of the redirect policy
   and mock-HTTP harness too, and vice versa — neither actually changed,
   but both get re-covered because the file boundary doesn't distinguish
   them. Split, a crypto-only change never touches `fetch.rs`'s signature,
   and a future HTTP-only change never forces re-reasoning about the
   crypto path.

Named `secrets.rs`, not `credentials.rs`, to avoid stuttering against
`config::Credentials` (the plain-data ciphertext struct from §3b, which
stays in `config.rs` — it's just TOML fields, no prompts, no crypto
operations). `secrets.rs` owns: Argon2id derivation, XChaCha20-Poly1305
encrypt/decrypt, the `rpassword` prompt, and all `secrecy`/`zeroize`/
`mlock` handling. Two entry points cover both directions:

- `secrets::decrypt(&config::Credentials) -> Result<DecryptedCredentials>`
  — called from `fetch.rs`: one new import, one call, no crypto inline.
- `secrets::encrypt(account_id: &str, license_key: &str) ->
  Result<config::Credentials>` — called from `conf.rs`'s not-yet-designed
  `--set-credentials` handler (§9 item 2), which is exactly where it
  belongs given `conf.rs` is already the "interactive creation" file.

`secrets.rs` becomes the single, focused unit holding all of this project's
secret-handling logic — the natural target for its own dedicated guardian
signature, and arguably deserving *more* scrutiny than `fetch.rs`'s
networking code precisely because everything sensitive is concentrated
there rather than smeared across files. `fetch.rs` itself is still edited
once (adding the `secrets::decrypt` call site) and still needs one re-audit
for that change — this doesn't avoid the first re-audit, it prevents every
*subsequent unrelated* one.

## 8. Decisions closed

1. **KDF: Argon2id**, new dependency, over PBKDF2-on-existing-`sha2`. (§3)
2. **Field scope: both `account_id` and `license_key` encrypted uniformly.**
   (§3)
3. **AEAD: XChaCha20-Poly1305**, over AES-256-GCM. (§3a)
4. **`secrecy` (`SecretBox`/`SecretString`), on top of `zeroize`** — not
   `zeroize` alone. (§5)
5. **TOML shape**: hex encoding, nested `[maxmind.credentials]` table, one
   combined `serde_json`-encoded plaintext under one nonce, explicit
   `m_cost`/`t_cost`/`p_cost` (no separate `kdf` name field). (§3b)
6. **New module `secrets.rs`** owns Argon2id/XChaCha20-Poly1305/`rpassword`/
   `secrecy`/`mlock`; `fetch.rs` calls `secrets::decrypt`, `conf.rs` will
   call `secrets::encrypt`. Not inline in `fetch.rs`. (§7)
7. **First-run/rotation UX**: fourth choice on `conf`'s existing
   `SelectorCommand` (`--set-credentials`); surgical edit via `toml_edit`
   (new dependency) + atomic write via `tempfile` (existing dependency);
   passphrase entered twice, `license_key` not; confirm before overwriting
   an existing credentials table; reject non-interactive stdin; no live
   MaxMind validation at set-time. (§9a)

**No further design decisions open. Ready for implementation.**

## 9a. First-run / rotation UX (ratified 2026-07-20)

**CLI surface: a fourth choice on `conf`'s existing `SelectorCommand`, not a
new command shape.** `docs/spec/cli.yaml`'s `conf` block is
`exactly_one_required: true` over `{s, d, e}`. `--set-credentials` (short
flag TBD at implementation time — not a design-level decision, same
treatment as the Argon2 parameter *values* in §3b) is a fourth choice on
that existing selector, going through the normal `xtgeoip-docgen` regen
like any other spec change — no new spec construct needed. First-run and
rotation are the same code path: there is no meaningful difference between
"the table doesn't exist yet" and "the table exists and is being replaced"
from the operation's point of view.

**Must edit the file surgically via `toml_edit`, not round-trip the whole
config through `serde`/`toml`.** Checked live: `toml_edit` (`0.25.13`,
676M downloads — the `toml-rs` project's own format-preserving parser,
what `cargo add`/`cargo edit` use internally) is the standard tool for
this. `conf -e` already shells to `$EDITOR` on the raw file, which means
this config is explicitly a hand-editable operator file — comments
included. Parsing the whole file into `Config` and re-serializing it would
silently reformat the entire file and drop any comments the admin added,
on an operation that only asked to change one table. That is exactly the
"don't surprise the user with more than was asked" instinct already
established for `build -c -f` and the restore rejection in `#98`.
`toml_edit` parses into a format-preserving document, lets the `Credentials`
struct (already `serde`-serializable, per §3b) be spliced in at
`document["maxmind"]["credentials"]` via `toml_edit`'s own `serde` support,
and serializes everything else back byte-for-byte untouched.

**Write atomically, reusing `tempfile`** (already a dependency) — write the
new document to a temp file in the same directory, then rename over
`/etc/xtgeoip.conf`. Same "don't leave things half-done" instinct as
`fetch.rs`'s `PartialDownload` guard; a process killed mid-write would
otherwise corrupt the one file this entire design depends on.

**Prompt sequence — three fields, two different treatments, because the
underlying risk differs:**

| Field | Echoed? | Confirmed (entered twice)? | Why |
|---|---|---|---|
| `account_id` | Yes (plain read) | No | Not secret; typos are visible as typed. |
| `license_key` | No (`rpassword`) | No | Secret, but MaxMind's own dashboard is an independent source of truth if it's ever wrong — a typo is recoverable. |
| passphrase | No (`rpassword`) | **Yes** | The one value with **no external record** — it exists only in the operator's memory. A masked entry gives no visual feedback, so a typo is invisible until the next `fetch` fails to decrypt, with no way to tell why. Standard double-entry-and-compare (`passwd`, `ssh-keygen`) is the established fix. |

The distinguishing question is the same one `#98` used for backups vs.
restores — does an independent record of the correct value exist
elsewhere? `license_key` has one; the passphrase doesn't. That is why
confirmation applies to one masked field and not the other, rather than
applying uniform ceremony to both regardless of actual recoverability.

**Confirm before overwriting an existing `[maxmind.credentials]` table.**
First-run has nothing to lose; rotation is destructive in the `#98` sense —
the old ciphertext becomes permanently unusable the moment it's
overwritten. A `[y/N]` prompt, reusing the exact idiom already in
`conf.rs`'s `prompt_create_config`, is proportionate — not a full `-f`
force-flag apparatus like `build`/`backup`, since this is a single,
deliberately-invoked action, not a pipeline step with ambiguous force
semantics.

**Non-interactive stdin must fail loudly.** `prompt_create_config` already
checks `io::stdin().is_terminal()` and bails with an actionable message.
`ConfAction::SetCredentials::check_preconditions()` needs the identical
check — three prompts here, none meaningful against a pipe.

**Rejected: validating credentials against MaxMind at set-time.** Tempting
("catch a typo immediately"), but wrong here specifically:
[[testing-boundaries]] already treats the MaxMind rate cap as scarce, and
a live check would make *every* rotation — far more frequent than the
occasional research fetch this project has spent that budget on before —
cost a real request. Validation happens for free the next time `fetch`
runs for real.

**Closes automatically, with zero change to it:** `ConfAction::Show` today
is `fs::read_to_string(SYSTEM_CONFIG)` — confirmed by reading `conf.rs`
directly, it just cats the file, which is exactly why the `conf --show`
incident happened. Once the file only ever holds ciphertext under
`[maxmind.credentials]`, `Show` becomes safe for free — no redaction logic
needs to be bolted onto it.

**Explicitly out of scope, deferred:** a "change the passphrase without
touching the MaxMind credentials" operation (re-encrypt under a new
passphrase, requiring the *old* one to decrypt first). Legitimate future
nicety, but distinct from what was asked here — encrypting data that comes
from MaxMind — and adding it now would be scope creep beyond the ratified
task.

## 10. Rejected

- **Salted one-way hash (shadow-password style).** Not invertible by
  definition; cannot produce a value MaxMind will accept. See §2.
- **`/dev/shm` / tmpfs as the memory-safety mechanism.** Solves IPC sharing,
  not swap exposure. See §5.
- **File-permission-only mitigation (`0600`).** Does not survive the stated
  threat model — a disk mounted from another root context ignores
  permissions on the mounting host's filesystem entirely.
