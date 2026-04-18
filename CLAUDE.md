# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development

```bash
cargo build            # debug build
cargo build --release  # release build
cargo clippy           # lint
rustfmt --check src/   # format check (80-col max, see rustfmt.toml)
```

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

There is no `cargo test` suite. Testing is done via the `xtgeoip-tests` binary against a real release build:

```bash
sudo target/release/xtgeoip-tests   # requires root and a release build
```

The test cases come from `docs/generated/testcases.yaml`, which is itself generated from `docs/spec/cli.yaml`. The test binary (`src/bin/xtgeoip-tests.rs`) must be kept in sync with `docs/spec/cli.yaml` changes.

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

**Helper binaries** (`src/bin/`): `xtgeoip-docgen` (codegen), `structure-errors`, `xtgeoip-tests` (test validator).

**Config** (TOML, default `/etc/xtgeoip.conf`):
- `[maxmind]` — account/license/URL for GeoLite2 CSV download
- `[paths]` — `archive_dir` (`/var/lib/xt_geoip`), `output_dir` (`/usr/share/xt_geoip`)
- `[logging]` — log file path

The binary must run as root to write to `output_dir`.
