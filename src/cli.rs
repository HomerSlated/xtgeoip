use anyhow::{Result, anyhow};
/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip CLI parsing and normalization
use clap::{Args, Parser, Subcommand};

use crate::{action::Action, config::ConfAction};

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
    },

    /// Manage system configuration
    #[command(group(
        clap::ArgGroup::new("conf_action")
            .required(true)
            .multiple(false)
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

fn unsupported_flags_message(flags: &[&str], context: &str) -> String {
    format!("Unsupported: {} {}.", flags.join(" "), context)
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

/// Normalize CLI input into a CliOutcome
pub fn normalize_cli_to_action(cli: &Cli) -> Result<CliOutcome> {
    use Commands::*;

    if cli.common.legacy && cli.command.is_none() {
        return Err(anyhow!(
            "Unsupported: --legacy only valid with build or run"
        ));
    }

    if let Some(cmd) = &cli.command {
        match cmd {
            Conf {
                default,
                show,
                edit: _,
            } => Ok(CliOutcome::Action(Action::Conf(conf_action(
                *default, *show,
            )))),

            Run { common, prune } => {
                if *prune && common.force && common.clean {
                    return Err(anyhow!(unsupported_flags_message(
                        &["-c", "-p", "-f"],
                        "combination is ambiguous in run"
                    )));
                }

                if common.backup && common.clean && *prune {
                    return Err(anyhow!(unsupported_flags_message(
                        &["-b", "-c", "-p"],
                        "combination is ambiguous in run"
                    )));
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
                if *prune && !common.backup {
                    return Err(anyhow!(
                        "Unsupported: --prune cannot be used without --backup \
                         for build"
                    ));
                }

                if *prune && common.force && common.backup && common.clean {
                    return Err(anyhow!(unsupported_flags_message(
                        &["-b", "-c", "-p", "-f"],
                        "combination is ambiguous for build"
                    )));
                }

                Ok(CliOutcome::Action(Action::Build {
                    legacy: common.legacy,
                    backup: common.backup,
                    clean: common.clean,
                    force: common.force,
                    prune: *prune,
                }))
            }

            Fetch { prune } => {
                Ok(CliOutcome::Action(Action::Fetch { prune: *prune }))
            }
        }
    } else {
        let b = cli.common.backup;
        let c = cli.common.clean;
        let p = cli.prune;
        let f = cli.common.force;

        if !b && !c && !p && !f {
            return Ok(CliOutcome::ShowHelp);
        }

        if p && !b && !c {
            return Err(anyhow!("Unsupported top-level flag combination"));
        }

        if f && !(b || c) {
            return Err(anyhow!("--force only applies to --backup or --clean"));
        }

        if c && p {
            return Err(anyhow!(unsupported_flags_message(
                &["--clean", "--prune"],
                "cannot be combined"
            )));
        }

        if b && p && f {
            return Err(anyhow!(unsupported_flags_message(
                &["--backup", "--prune", "--force"],
                "combination is ambiguous"
            )));
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

        Err(anyhow!("Unsupported top-level flag combination"))
    }
}
