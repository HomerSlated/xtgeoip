use anyhow::{Result, anyhow};
/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip CLI parsing and normalization
use clap::{Args, Parser, Subcommand};

use crate::{
    action::Action,
    conf::ConfAction,
    generated::cli_rules::{self, Guard},
};

pub enum CliOutcome {
    Action(Action),
    ShowHelp,
}

#[derive(Args)]
pub struct CommonFlags {
    /// Back up current database before replacing it
    #[arg(short, long)]
    pub backup: bool,

    /// Delete current binary database files
    #[arg(short, long)]
    pub clean: bool,

    /// Force the operation (overrides safety checks)
    #[arg(short, long)]
    pub force: bool,

    /// Enable legacy mode (historical compatibility only)
    #[arg(short = 'l', long)]
    pub legacy: bool,
}

#[derive(Parser)]
#[command(
    name = "xtgeoip",
    version,
    about = "Build and manage xt_geoip data from MaxMind GeoLite2 CSVs",
    propagate_version = false,
    disable_help_subcommand = true,
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(flatten)]
    pub common: CommonFlags,

    /// Prune old bin archives (requires --backup)
    #[arg(short, long)]
    pub prune: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Fetch then build the full pipeline
    #[command(disable_version_flag = true)]
    Run {
        #[command(flatten)]
        common: CommonFlags,

        /// Prune old CSV archives after fetching
        #[arg(short, long)]
        prune: bool,
    },

    /// Build binary database from local CSV archive
    #[command(disable_version_flag = true)]
    Build {
        #[command(flatten)]
        common: CommonFlags,

        /// Prune old bin archives after backup (requires --backup)
        #[arg(short, long)]
        prune: bool,
    },

    /// Download GeoLite2 CSV archive from MaxMind
    #[command(disable_version_flag = true)]
    Fetch {
        /// Prune old CSV archives after fetching
        #[arg(short, long)]
        prune: bool,

        // These flags are not supported for fetch; accepted here to emit
        // proper keyed errors rather than generic clap parse errors.
        #[arg(short = 'l', long, hide = true)]
        legacy: bool,
        #[arg(short = 'b', long, hide = true)]
        backup: bool,
        #[arg(short = 'c', long, hide = true)]
        clean: bool,
        #[arg(short = 'f', long, hide = true)]
        force: bool,
    },

    /// Manage system configuration
    // required(true) is intentionally omitted — the missing-flag case is
    // handled in normalize_cli_to_action to emit a keyed error.
    #[command(group(
        clap::ArgGroup::new("conf_action").multiple(false)
    ))]
    #[command(disable_version_flag = true)]
    Conf {
        /// Show default (example) configuration
        #[arg(short = 'd', long = "default", group = "conf_action")]
        default: bool,

        /// Show system configuration
        #[arg(short = 's', long = "show", group = "conf_action")]
        show: bool,

        /// Open system configuration in $EDITOR
        #[arg(short = 'e', long = "edit", group = "conf_action")]
        edit: bool,
    },
}

fn conf_action(default: bool, show: bool) -> ConfAction {
    if default {
        ConfAction::Default
    } else if show {
        ConfAction::Show
    } else {
        ConfAction::Edit
    }
}

fn keyed_err(key: &str, msg: &str) -> anyhow::Error {
    anyhow!("[{key}]: {msg}")
}

/// Pack the flag universe into the `u8` bitmask the generated guard tables use.
/// Bit positions are defined by `cli_rules` (sorted flag-universe order).
fn flag_mask(b: bool, c: bool, f: bool, l: bool, p: bool) -> u8 {
    use cli_rules::{B, C, F, L, P};
    let mut m = 0;
    if b {
        m |= B;
    }
    if c {
        m |= C;
    }
    if f {
        m |= F;
    }
    if l {
        m |= L;
    }
    if p {
        m |= P;
    }
    m
}

/// First guard whose `require` bits are all set and `forbid` bits all clear.
/// First match wins — table order encodes precedence (see `cli_rules`).
fn first_guard(flags: u8, guards: &'static [Guard]) -> Option<&'static Guard> {
    guards
        .iter()
        .find(|g| flags & g.require == g.require && flags & g.forbid == 0)
}

/// Normalize CLI input into a CliOutcome.
///
/// The combination rules live in `docs/spec/cli.yaml` and are compiled by
/// `xtgeoip-docgen` into the per-context guard tables in
/// `crate::generated::cli_rules`. This function only packs the parsed flags
/// into a bitmask, evaluates the first matching guard, and otherwise constructs
/// the `Action`. `conf` is outside the guard model (its rules are owned by
/// clap's `ArgGroup` + the required positional; only the missing-flag case is
/// keyed here).
pub fn normalize_cli_to_action(cli: &Cli) -> Result<CliOutcome> {
    use Commands::*;

    if let Some(cmd) = &cli.command {
        match cmd {
            Conf {
                default,
                show,
                edit,
            } => {
                if !default && !show && !edit {
                    return Err(keyed_err(
                        "conf_missing_flag",
                        "conf requires one of: --default (-d), --show (-s), \
                         --edit (-e)",
                    ));
                }
                Ok(CliOutcome::Action(Action::Conf(conf_action(
                    *default, *show,
                ))))
            }

            Run { common, prune } => {
                let flags = flag_mask(
                    common.backup,
                    common.clean,
                    common.force,
                    common.legacy,
                    *prune,
                );
                if let Some(g) = first_guard(flags, cli_rules::RUN_GUARDS) {
                    return Err(keyed_err(g.key, g.message));
                }
                Ok(CliOutcome::Action(Action::Run {
                    prune: *prune,
                    legacy: common.legacy,
                    backup: common.backup,
                    clean: common.clean,
                    force: common.force,
                }))
            }

            Build { common, prune } => {
                let flags = flag_mask(
                    common.backup,
                    common.clean,
                    common.force,
                    common.legacy,
                    *prune,
                );
                if let Some(g) = first_guard(flags, cli_rules::BUILD_GUARDS) {
                    return Err(keyed_err(g.key, g.message));
                }
                Ok(CliOutcome::Action(Action::Build {
                    legacy: common.legacy,
                    backup: common.backup,
                    clean: common.clean,
                    force: common.force,
                    prune: *prune,
                }))
            }

            Fetch {
                prune,
                legacy,
                backup,
                clean,
                force,
            } => {
                let flags = flag_mask(*backup, *clean, *force, *legacy, *prune);
                if let Some(g) = first_guard(flags, cli_rules::FETCH_GUARDS) {
                    return Err(keyed_err(g.key, g.message));
                }
                Ok(CliOutcome::Action(Action::Fetch { prune: *prune }))
            }
        }
    } else {
        let b = cli.common.backup;
        let c = cli.common.clean;
        let p = cli.prune;
        let f = cli.common.force;
        let l = cli.common.legacy;

        let flags = flag_mask(b, c, f, l, p);
        if let Some(g) = first_guard(flags, cli_rules::TOP_LEVEL_GUARDS) {
            return Err(keyed_err(g.key, g.message));
        }

        // No guard fired. Bare invocation shows help (main renders this as
        // top_level_no_args); otherwise -b/-c select the top-level action.
        if flags == 0 {
            return Ok(CliOutcome::ShowHelp);
        }
        if b {
            return Ok(CliOutcome::Action(Action::TopLevelBackup {
                clean: c,
                force: f,
                prune: p,
            }));
        }
        if c {
            return Ok(CliOutcome::Action(Action::TopLevelClean { force: f }));
        }
        Err(anyhow!("unsupported flag combination"))
    }
}

/// Exhaustive behavior snapshot of the CLI semantics layer.
///
/// Enumerates every flag combination per context, parses it the way `main` does
/// (`Cli::try_parse_from`), runs `normalize_cli_to_action` (pure — no root, no
/// filesystem, no execution), and locks the outcome against a golden file. This
/// is the regression net for the spec-driven validator rewrite: the new
/// evaluator must reproduce this snapshot byte-for-byte. The spec examples
/// cannot serve this role (one canonical example per error case — see TODO
/// #92).
///
/// Regenerate after an *intended* behavior change:
///   cargo test regenerate_snapshot -- --ignored
#[cfg(test)]
mod snapshot {
    use clap::Parser;

    use super::*;

    /// All invocations as the args following the program name, per context.
    fn all_invocations() -> Vec<Vec<&'static str>> {
        let contexts: &[(&[&str], &[&str])] = &[
            (&[], &["-b", "-c", "-p", "-f", "-l"]),
            (&["fetch"], &["-p", "-b", "-c", "-f", "-l"]),
            (&["build"], &["-b", "-c", "-p", "-f", "-l"]),
            (&["run"], &["-b", "-c", "-p", "-f", "-l"]),
            (&["conf"], &["-d", "-s", "-e"]),
        ];
        let mut out = Vec::new();
        for (prefix, flags) in contexts {
            for mask in 0..(1u32 << flags.len()) {
                let mut argv = vec!["xtgeoip"];
                argv.extend_from_slice(prefix);
                for (i, flag) in flags.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        argv.push(flag);
                    }
                }
                out.push(argv);
            }
        }
        out
    }

    /// Canonical outcome string for one invocation.
    fn outcome(argv: &[&str]) -> String {
        match Cli::try_parse_from(argv) {
            Err(_) => "PARSE_ERR".to_string(),
            Ok(cli) => match normalize_cli_to_action(&cli) {
                Ok(CliOutcome::ShowHelp) => "ShowHelp".to_string(),
                Ok(CliOutcome::Action(action)) => format!("{action:?}"),
                Err(e) => {
                    let s = e.to_string();
                    let key = s
                        .strip_prefix('[')
                        .and_then(|r| r.split_once(']'))
                        .map(|(k, _)| k)
                        .unwrap_or(s.as_str());
                    format!("Err({key})")
                }
            },
        }
    }

    fn snapshot() -> String {
        let mut lines: Vec<String> = all_invocations()
            .iter()
            .map(|argv| format!("{} => {}", argv.join(" "), outcome(argv)))
            .collect();
        lines.sort();
        lines.join("\n") + "\n"
    }

    #[test]
    fn cli_semantics_snapshot() {
        let actual = snapshot();
        let golden = include_str!("cli_snapshot.golden");
        for (i, (a, g)) in actual.lines().zip(golden.lines()).enumerate() {
            assert_eq!(
                a, g,
                "CLI semantics changed at line {i} — if intended, regenerate \
                 the snapshot (see module docs)"
            );
        }
        assert_eq!(
            actual.lines().count(),
            golden.lines().count(),
            "CLI invocation count changed — regenerate the snapshot"
        );
    }

    #[test]
    #[ignore = "writes the golden file; run explicitly after intended changes"]
    fn regenerate_snapshot() {
        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/cli_snapshot.golden");
        std::fs::write(path, snapshot()).unwrap();
    }
}
