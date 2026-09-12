# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development

```bash
cargo build            # debug build
cargo build --release  # release build
cargo clippy --all-targets -- -D warnings   # lint (as CI runs it)
cargo "+$(cat rustfmt-toolchain)" fmt -- --check   # format check (80-col, rustfmt.toml)
# is either toolchain pin stale? newest stable, then newest nightly date:
curl -s https://static.rust-lang.org/dist/channel-rust-stable.toml | grep -m1 -A1 '^\[pkg\.rust\]$' | tail -1
curl -s https://static.rust-lang.org/dist/channel-rust-nightly.toml | grep -m1 '^date'
```

Both toolchains are pinned. The stable one is in `rust-toolchain.toml`, so a
bare `cargo ...` resolves through it. The formatter is the *only* exception:
`rustfmt.toml` uses five nightly-only options, so the format check runs under
the dated nightly named in `rustfmt-toolchain`, which CI and the sync script
both read. Do not add `rustfmt` to the stable toolchain — a stable rustfmt
silently discards those five options, `ignore` among them, and then rewrites
`src/generated/`.

Those two lines read the release manifests, and install nothing; compare them
against `rust-toolchain.toml` and `rustfmt-toolchain`. `scripts/sync.py` makes
the same comparison weekly. Not `rustup check`: it reports only the floating
channels that happen to be installed, so on a machine with no `stable`
toolchain it says nothing about stable at all. Bumping is deliberate — read the
new lints when you do, since `-D warnings` turns a fresh style lint into a hard
CI error.

Before a release build, run the pre-build workflow:

```bash
./scripts/update.fish      # git add/commit/push + cargo fix
cargo build --release
```

Generated source files must be regenerated after changing `docs/spec/cli.yaml`:

```bash
cargo run --bin xtgeoip-docgen
```

This writes to `src/generated/` (error constants, CLI test matrix) and `docs/generated/` (markdown, man page, test cases YAML). Commit generated output alongside spec changes.

## Testing

Two suites, with different jobs. Run the unit tests freely; run the
integration suite deliberately.

**Unit tests** (`cargo test`) — fast, hermetic, no root, no network. These
cover parsing, planning, the spec-vs-implementation contradiction checks, the
136-combination CLI snapshot, and the man-page-vs-program checks.

```bash
cargo test                  # all of it
cargo test --lib            # library only
```

Two are marked `#[ignore]`, for opposite reasons. The first rewrites a golden
file rather than asserting against it; the second is the only test here that
spawns a process and binds a port, and is kept out of the hermetic run. Run
either explicitly after an intended change:

```bash
cargo test --lib -- --ignored regenerate_snapshot   # src/cli_snapshot.golden
cargo test --bin xtgeoip-tests -- --ignored stub_serves   # the HTTPS stub
```

**Integration suite** (`xtgeoip-tests`) — drives the real release binary
end to end:

```bash
sudo target/release/xtgeoip-tests   # requires root and a release build
```

It needs root and `openssl(1)`. Since #98 it touches neither production data
nor the MaxMind API:

- **Writes go to a sandbox.** Each run creates a private `0700` temp tree,
  seeds it by copying the configured `output_dir` and `archive_dir`, and
  appends `--config <sandbox>/xtgeoip.conf` to every case. Production data is
  read once to seed and never written. `--keep-sandbox` leaves the tree behind.
- **Reads go to a local stub.** Each run starts `openssl s_server -HTTP`
  serving the two endpoints `fetch` uses, replaying the newest seeded archive
  under a synthetic version, and appends `--ca-file <sandbox>/stub/ca.pem` to
  every case. That CA *replaces* the trust roots, so a case that addressed the
  real API would fail to verify it rather than quietly succeed. The run fails
  if the remote cases never reach the stub — the corpus asserts exit statuses
  only, so a stub that is never hit would otherwise look green.

It still prompts for the credentials passphrase once per remote case (ten of
them): `secrets::decrypt` reads from the terminal by design (#103), and the
runner deliberately does not route around it.

Its cases come from `docs/generated/testcases.yaml`, generated from
`docs/spec/cli.yaml`, and the runner (`src/bin/xtgeoip-tests.rs`) carries
hand-maintained corpus-size assertions that must be updated when the spec gains
or loses a case.

## Architecture

**Single source of truth**: `docs/spec/cli.yaml` defines all CLI behavior. The `xtgeoip-docgen` binary reads it to generate Rust source, docs, and test cases. Do not edit `src/generated/` files by hand.

**Main binary flow** (`src/`):

| File | Role |
|------|------|
| `main.rs` | Entry point; sets up logger, dispatches to `action.rs` |
| `cli.rs` | `clap`-based arg parsing; normalizes flags into `CliArgs` |
| `action.rs` | Matches `CliArgs` → calls fetch/build/backup/conf |
| `fetch.rs` | Downloads MaxMind GeoLite2 CSV ZIP; version detection avoids redundant downloads |
| `build.rs` | Parses CSVs with Rayon, writes binary IP-range files for `xt_geoip` kernel module |
| `backup.rs` | Archive create / delete / prune |
| `config.rs` | TOML config load (`/etc/xtgeoip.conf`); `conf` subcommand handler |
| `messages.rs` | `fern` + `syslog` logging setup |

**Helper binaries** (`src/bin/`): `xtgeoip-docgen` (codegen), `xtgeoip-tests` (test validator).

**Config** (TOML, default `/etc/xtgeoip.conf`):
- `[maxmind]` — account/license/URL for GeoLite2 CSV download
- `[paths]` — `archive_dir` (`/var/lib/xt_geoip`), `output_dir` (`/usr/share/xt_geoip`)
- `[logging]` — log file path

The binary needs write access to the directories its plan changes
(`output_dir` to build or clean, `archive_dir` to fetch, back up or prune);
`action.rs::check_plan_writable` probes them before the first step. There is
no root gate — on a default install root is simply who owns those paths.
