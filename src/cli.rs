/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip CLI parsing and normalization
use clap::{Parser, Subcommand};
use anyhow::{Result, anyhow};

use crate::action::{Action, ConfAction};

#[derive(Parser)]
#[command(
    name = "xtgeoip",
    version = "2026",
    about = "Downloads and builds GeoIP databases",
    propagate_version = true
)]
pub struct Cli {
    #[arg(short, long, global = true)]
    pub backup: bool,
    #[arg(short, long, global = true)]
    pub clean: bool,
    #[arg(short, long, global = true)]
    pub force: bool,
    #[arg(short, long, global = true)]
    pub prune: bool,
    #[arg(short = 'l', long, global = true)]
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
    let joined = flags.join(" ");
    format!("Unsupported: {} {}.", joined, context)
}

/// Convert CLI args into ConfAction for the conf command
fn conf_action(default: bool, show: bool) -> ConfAction {
    if default {
        ConfAction::Default
    } else if show {
        ConfAction::Show
    } else {
        ConfAction::Edit
    }
}

/// Enforce top-level flag rules
pub fn enforce_flag_rules(cli: &Cli) -> Result<()> {
    if cli.command.is_none() {
        let b = cli.backup;
        let c = cli.clean;
        let p = cli.prune;
        let f = cli.force;

        if p && !b && !c {
            return Err(anyhow!("Unsupported: -p alone is ambiguous"));
        }
        if f && !(b || c) {
            return Err(anyhow!("--force only applies to --backup or --clean"));
        }
        if c && p {
            return Err(anyhow!(unsupported_flags_message(&["--clean", "--prune"], "cannot be combined")));
        }
        if b && p && f {
            return Err(anyhow!(unsupported_flags_message(&["--backup", "--prune", "--force"], "combination is ambiguous")));
        }
    }
    Ok(())
}

/// Normalize CLI input into Action
pub fn normalize_cli_to_action(cli: &Cli) -> Result<Option<Action>> {
    use Commands::*;

    if cli.legacy {
        match &cli.command {
            Some(Build { .. }) | Some(Run { .. }) => {}
            _ => return Err(anyhow!("Unsupported: --legacy only valid with build or run")),
        }
    }

    if let Some(cmd) = &cli.command {
        match cmd {
            Conf { default, show, edit: _ } => Ok(Some(Action::Conf(conf_action(*default, *show)))),

            Run { prune, backup, clean, force } => {
                let mut invalid_flags = vec![];
                if *prune && *force && *clean {
                    invalid_flags.extend(&["-c", "-p", "-f"]);
                    return Err(anyhow!(unsupported_flags_message(&invalid_flags, "combination is ambiguous in run")));
                }
                if *backup && *clean && *prune {
                    invalid_flags.extend(&["-b", "-c", "-p"]);
                    return Err(anyhow!(unsupported_flags_message(&invalid_flags, "combination is ambiguous in run")));
                }

                Ok(Some(Action::Run {
                    prune: *prune,
                    legacy: cli.legacy,
                    backup: *backup,
                    clean: *clean,
                    force: *force,
                }))
            }

            Build { prune, force, backup, clean } => {
                if *prune && !*backup {
                    return Err(anyhow!("Unsupported: --prune cannot be used without --backup for build"));
                }
                if *prune && *force && *backup && *clean {
                    let flags = ["-b", "-c", "-p", "-f"];
                    return Err(anyhow!(unsupported_flags_message(&flags, "combination is ambiguous for build")));
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
                if cli.backup { invalid_flags.push("-b"); }
                if cli.clean { invalid_flags.push("-c"); }
                if cli.force { invalid_flags.push("-f"); }

                if !invalid_flags.is_empty() {
                    return Err(anyhow!(unsupported_flags_message(&invalid_flags, "is invalid for fetch")));
                }

                Ok(Some(Action::Fetch { prune: *prune }))
            }
        }
    } else {
        let b = cli.backup;
        let c = cli.clean;
        let p = cli.prune;
        let f = cli.force;

        if !b && !c && !p {
            return Ok(None);
        }
        if b {
            return Ok(Some(Action::TopLevelBackup { clean: c, force: f, prune: p }));
        }
        if c {
            return Ok(Some(Action::TopLevelClean { force: f }));
        }

        Err(anyhow!("Unsupported top-level flag combination"))
    }
}
