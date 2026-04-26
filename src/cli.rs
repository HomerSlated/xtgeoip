use anyhow::{Result, anyhow};
/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip CLI parsing and normalization
use clap::{Parser, Subcommand};

use crate::{action::Action, config::ConfAction};

pub enum CliOutcome {
    Action(Action),
    ShowHelp,
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
    /// Back up current database before replacing it
    #[arg(short, long)]
    pub backup: bool,

    /// Delete current binary database files
    #[arg(short, long)]
    pub clean: bool,

    /// Force the operation (overrides safety checks)
    #[arg(short, long)]
    pub force: bool,

    /// Prune old bin archives (requires --backup)
    #[arg(short, long)]
    pub prune: bool,

    /// Enable legacy mode (historical compatibility only)
    #[arg(short = 'l', long)]
    pub legacy: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Fetch then build the full pipeline
    #[command(disable_version_flag = true)]
    Run {
        /// Back up current database before replacing it
        #[arg(short, long)]
        backup: bool,

        /// Delete current binary database files before building
        #[arg(short, long)]
        clean: bool,

        /// Force the operation (overrides safety checks)
        #[arg(short, long)]
        force: bool,

        /// Prune old CSV archives after fetching
        #[arg(short, long)]
        prune: bool,

        /// Enable legacy mode (historical compatibility only)
        #[arg(short, long)]
        legacy: bool,
    },

    /// Build binary database from local CSV archive
    #[command(disable_version_flag = true)]
    Build {
        /// Back up current database before replacing it
        #[arg(short, long)]
        backup: bool,

        /// Delete current binary database files before building
        #[arg(short, long)]
        clean: bool,

        /// Force the operation (overrides safety checks)
        #[arg(short, long)]
        force: bool,

        /// Prune old bin archives after backup (requires --backup)
        #[arg(short, long)]
        prune: bool,

        /// Enable legacy mode (historical compatibility only)
        #[arg(short, long)]
        legacy: bool,
    },

    /// Download GeoLite2 CSV archive from MaxMind
    #[command(disable_version_flag = true)]
    Fetch {
        /// Prune old CSV archives after fetching
        #[arg(short, long)]
        prune: bool,

        #[arg(short, long, hide = true)]
        backup: bool,

        #[arg(short, long, hide = true)]
        clean: bool,

        #[arg(short, long, hide = true)]
        force: bool,

        #[arg(short, long, hide = true)]
        legacy: bool,
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

/// Build a dynamic error message for unsupported flags
fn unsupported_flags_message(flags: &[&str], context: &str) -> String {
    format!("Unsupported: {} {}.", flags.join(" "), context)
}

/// Convert CLI args into ConfAction
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

    // Top-level --legacy is invalid unless used with build/run subcommands
    if cli.legacy && cli.command.is_none() {
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
            } => Ok(CliOutcome::Action(Action::Conf(conf_action(*default, *show)))),

            Run {
                prune,
                backup,
                clean,
                force,
                legacy,
            } => {
                let mut invalid_flags = vec![];

                if *prune && *force && *clean {
                    invalid_flags.extend(&["-c", "-p", "-f"]);
                    return Err(anyhow!(unsupported_flags_message(
                        &invalid_flags,
                        "combination is ambiguous in run"
                    )));
                }

                if *backup && *clean && *prune {
                    invalid_flags.extend(&["-b", "-c", "-p"]);
                    return Err(anyhow!(unsupported_flags_message(
                        &invalid_flags,
                        "combination is ambiguous in run"
                    )));
                }

                Ok(CliOutcome::Action(Action::Run {
                    prune: *prune,
                    legacy: *legacy,
                    backup: *backup,
                    clean: *clean,
                    force: *force,
                }))
            }

            Build {
                prune,
                force,
                backup,
                clean,
                legacy,
            } => {
                if *prune && !*backup {
                    return Err(anyhow!(
                        "Unsupported: --prune cannot be used without --backup \
                         for build"
                    ));
                }

                if *prune && *force && *backup && *clean {
                    let flags = ["-b", "-c", "-p", "-f"];
                    return Err(anyhow!(unsupported_flags_message(
                        &flags,
                        "combination is ambiguous for build"
                    )));
                }

                Ok(CliOutcome::Action(Action::Build {
                    legacy: *legacy,
                    backup: *backup,
                    clean: *clean,
                    force: *force,
                    prune: *prune,
                }))
            }

            Fetch {
                prune,
                backup,
                clean,
                force,
                legacy,
            } => {
                let mut invalid_flags = vec![];

                if *backup {
                    invalid_flags.push("-b");
                }
                if *clean {
                    invalid_flags.push("-c");
                }
                if *force {
                    invalid_flags.push("-f");
                }
                if *legacy {
                    invalid_flags.push("-l");
                }

                if !invalid_flags.is_empty() {
                    return Err(anyhow!(unsupported_flags_message(
                        &invalid_flags,
                        "is invalid for fetch"
                    )));
                }

                Ok(CliOutcome::Action(Action::Fetch { prune: *prune }))
            }
        }
    } else {
        // Top-level flag mode
        let b = cli.backup;
        let c = cli.clean;
        let p = cli.prune;
        let f = cli.force;

        if !b && !c && !p && !f {
            return Ok(CliOutcome::ShowHelp);
        }

        // -p alone invalid
        if p && !b && !c {
            return Err(anyhow!("Unsupported top-level flag combination"));
        }

        // -f must attach to b or c
        if f && !(b || c) {
            return Err(anyhow!("--force only applies to --backup or --clean"));
        }

        // -c -p invalid
        if c && p {
            return Err(anyhow!(unsupported_flags_message(
                &["--clean", "--prune"],
                "cannot be combined"
            )));
        }

        // -b -p -f invalid
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
