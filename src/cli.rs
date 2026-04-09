/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip CLI parsing and normalization
use clap::{Parser, Subcommand};
use anyhow::{Result, anyhow};

use crate::{action::Action, config::ConfAction};

use anyhow::{anyhow, Result};
/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip CLI parsing and normalization
use clap::{Parser, Subcommand};

use crate::{action::Action, config::ConfAction};

#[derive(Parser)]
#[command(
    name = "xtgeoip",
    version,
    about = "Downloads and builds GeoIP databases",
    propagate_version = true,
    disable_help_subcommand = true,
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[arg(short, long)]
    pub backup: bool,

    #[arg(short, long)]
    pub clean: bool,

    #[arg(short, long)]
    pub force: bool,

    #[arg(short, long)]
    pub prune: bool,

    #[arg(short = 'l', long)]
    pub legacy: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    Run {
        #[arg(short, long)]
        prune: bool,

        #[arg(short, long)]
        backup: bool,

        #[arg(short, long)]
        clean: bool,

        #[arg(short, long)]
        force: bool,
    },

    Build {
        #[arg(short, long)]
        backup: bool,

        #[arg(short, long)]
        clean: bool,

        #[arg(short, long)]
        force: bool,

        #[arg(short, long)]
        prune: bool,
    },

    Fetch {
        #[arg(short, long)]
        prune: bool,
    },

    #[command(group(
        clap::ArgGroup::new("conf_action")
            .required(true)
            .multiple(false)
    ))]
    Conf {
        #[arg(short = 'd', long = "default", group = "conf_action")]
        default: bool,

        #[arg(short = 's', long = "show", group = "conf_action")]
        show: bool,

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

/// Normalize CLI input into Action
pub fn normalize_cli_to_action(cli: &Cli) -> Result<Option<Action>> {
    use Commands::*;

    // --legacy only valid with build/run
    if cli.legacy {
        match &cli.command {
            Some(Build { .. }) | Some(Run { .. }) => {}
            _ => {
                return Err(anyhow!(
                    "Unsupported: --legacy only valid with build or run"
                ));
            }
        }
    }

    if let Some(cmd) = &cli.command {
        match cmd {
            Conf {
                default,
                show,
                edit: _,
            } => Ok(Some(Action::Conf(conf_action(*default, *show)))),

            Run {
                prune,
                backup,
                clean,
                force,
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

                Ok(Some(Action::Run {
                    prune: *prune,
                    legacy: cli.legacy,
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
            } => {
                if *prune && !*backup {
                    return Err(anyhow!(
                        "Unsupported: --prune cannot be used without --backup for build"
                    ));
                }

                if *prune && *force && *backup && *clean {
                    let flags = ["-b", "-c", "-p", "-f"];
                    return Err(anyhow!(unsupported_flags_message(
                        &flags,
                        "combination is ambiguous for build"
                    )));
                }

                Ok(Some(Action::Build {
                    legacy: cli.legacy,
                    backup: *backup,
                    clean: *clean,
                    force: *force,
                    prune: *prune,
                }))
            }

            Fetch { prune } => {
                let mut invalid_flags = vec![];

                if cli.backup {
                    invalid_flags.push("-b");
                }
                if cli.clean {
                    invalid_flags.push("-c");
                }
                if cli.force {
                    invalid_flags.push("-f");
                }
                if cli.legacy {
                    invalid_flags.push("-l");
                }

                if !invalid_flags.is_empty() {
                    return Err(anyhow!(unsupported_flags_message(
                        &invalid_flags,
                        "is invalid for fetch"
                    )));
                }

                Ok(Some(Action::Fetch { prune: *prune }))
            }
        }
    } else {
        // Top-level flag mode
        let b = cli.backup;
        let c = cli.clean;
        let p = cli.prune;
        let f = cli.force;

        if !b && !c && !p {
            return Ok(None);
        }

        // -p alone invalid
        if p && !b && !c {
            return Err(anyhow!("Unsupported top-level flag combination"));
        }

        // -f must attach to b or c
        if f && !(b || c) {
            return Err(anyhow!(
                "--force only applies to --backup or --clean"
            ));
        }

        // -c -p invalid
        if c && p {
            return Err(anyhow!(
                unsupported_flags_message(&["--clean", "--prune"], "cannot be combined")
            ));
        }

        // -b -p -f invalid
        if b && p && f {
            return Err(anyhow!(
                unsupported_flags_message(&["--backup", "--prune", "--force"], "combination is ambiguous")
            ));
        }

        if b {
            return Ok(Some(Action::TopLevelBackup {
                clean: c,
                force: f,
                prune: p,
            }));
        }

        if c {
            return Ok(Some(Action::TopLevelClean { force: f }));
        }

        Err(anyhow!("Unsupported top-level flag combination"))
    }
}
