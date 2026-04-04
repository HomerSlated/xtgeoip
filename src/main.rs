/// xtgeoip © Haze N Sparkle 2026 (MIT)
///
/// Downloads, extracts, and converts GeoIP CSV databases into binary IP
/// range data files, compatible with the Linux x_tables xt_geoip module,
/// which can filter packets based on GeoIP country labels.
///
/// Inspired by xt_geoip_build_maxmind (Jan Engelhardt, Philip
/// Prindeville), now part of Debian's xtables-addons package.
use std::path::Path;

use anyhow::{anyhow, Result};
use clap::{CommandFactory, Parser, Subcommand};

mod backup;
mod build;
mod config;
mod fetch;
mod messages;

use crate::{
    backup::{backup, delete, prune_archives},
    build::build,
    config::{ConfAction, load_config, run_conf},
    fetch::{FetchMode, fetch},
    messages::{error, init_logger, log_early_error, warn},
};

#[derive(Parser)]
#[command(
    name = "xtgeoip",
    version,
    about = "Downloads and builds GeoIP databases",
    propagate_version = true
)]
struct Cli {
    #[arg(short, long, global = true)]
    backup: bool,

    #[arg(short, long, global = true)]
    clean: bool,

    #[arg(short, long, global = true)]
    force: bool,

    #[arg(short, long, global = true)]
    prune: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long)]
        prune: bool,

        #[arg(short = 'l', long)]
        legacy: bool,
    },
    Build {
        #[arg(short = 'l', long)]
        legacy: bool,
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

/// Normalized CLI action type for internal dispatch
enum Action {
    Run { prune: bool, legacy: bool },
    Build { legacy: bool },
    Fetch { prune: bool },
    Conf(ConfAction),
}

/// Warn user if legacy mode is enabled
fn warn_legacy_mode(legacy: bool) {
    if legacy {
        warn(
            "Warning: Legacy Mode activated. See documentation for collisions.",
        );
    }
}

/// Convert Conf CLI args to ConfAction enum
fn conf_action(default: bool, show: bool) -> ConfAction {
    if default {
        ConfAction::Default
    } else if show {
        ConfAction::Show
    } else {
        ConfAction::Edit
    }
}

/// Enforce rules for global flags before executing any subcommand
fn validate_flags(cli: &Cli) -> Result<()> {
    match &cli.command {
        Some(Commands::Fetch { .. }) => {
            if cli.backup {
                return Err(anyhow!("unsupported option, -b is not valid for fetch"));
            }
            if cli.clean {
                return Err(anyhow!("unsupported option, -c is not valid for fetch"));
            }
            if cli.force {
                return Err(anyhow!("unsupported option, -f is not valid for fetch"));
            }
        }
        Some(Commands::Build { .. }) => {
            if cli.prune && !cli.backup {
                return Err(anyhow!("unsupported option, -p requires --backup"));
            }
            if cli.force && !(cli.backup || cli.clean) {
                return Err(anyhow!("unsupported option, -f only applies to -b or -c"));
            }
            if cli.backup && cli.prune && cli.force {
                return Err(anyhow!(
                    "unsupported option, ambiguous and prune does not support force"
                ));
            }
        }
        Some(Commands::Run { .. }) => {
            if cli.backup && cli.prune && cli.clean {
                return Err(anyhow!(
                    "unsupported option, ambiguous (does prune apply to fetch or backup?)"
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::try_parse().map_err(|e| {
        log_early_error(&format!("CLI argument parsing failed: {}", e.kind()));
        eprintln!("{e}");
        e
    })?;

    run(cli)?;

    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    // Load system configuration
    let cfg = load_config().map_err(|e| {
        log_early_error(&format!("Failed to load config: {}", e));
        eprintln!("Fatal: Failed to load config: {}", e);
        e
    })?;

    // Initialize logging
    if let Some(log_file) = cfg.logging.as_ref().map(|l| l.log_file.as_str()) {
        init_logger(log_file)?;
    }

    // Validate flag rules according to spec
    validate_flags(&cli)?;

    // Execute top-level flags only if valid for the command
    if cli.backup {
        backup(
            Path::new(&cfg.paths.output_dir),
            Path::new(&cfg.paths.archive_dir),
            cli.force,
        )?;
    }
    if cli.clean {
        delete(Path::new(&cfg.paths.output_dir), cli.force)?;
    }
    if cli.prune {
        prune_archives(&cfg, false, cli.backup)?;
    }

    // Convert CLI subcommand into normalized internal Action
    let action: Option<Action> = match &cli.command {
        Some(Commands::Conf { default, show, edit: _ }) => {
            Some(Action::Conf(conf_action(*default, *show)))
        }
        Some(Commands::Run { prune, legacy }) => Some(Action::Run {
            prune: *prune,
            legacy: *legacy,
        }),
        Some(Commands::Build { legacy }) => Some(Action::Build { legacy: *legacy }),
        Some(Commands::Fetch { prune }) => Some(Action::Fetch { prune: *prune }),
        None => None,
    };

    // Dispatch subcommands
    if let Some(action) = action {
        match action {
            Action::Conf(conf) => run_conf(conf)?,

            Action::Run { prune, legacy } => {
                let (temp_dir, version) = fetch(&cfg, FetchMode::Remote)?;
                warn_legacy_mode(legacy);
                build(
                    temp_dir.path(),
                    Path::new(&cfg.paths.output_dir),
                    &version,
                    legacy,
                )?;
                if prune {
                    prune_archives(&cfg, true, false)?;
                }
            }

            Action::Build { legacy } => {
                let (temp_dir, version) = fetch(&cfg, FetchMode::Local)?;
                warn_legacy_mode(legacy);
                build(
                    temp_dir.path(),
                    Path::new(&cfg.paths.output_dir),
                    &version,
                    legacy,
                )?;
            }

            Action::Fetch { prune } => {
                let _ = fetch(&cfg, FetchMode::Remote)?;
                if prune {
                    prune_archives(&cfg, true, false)?;
                }
            }
        }
    } else if !(cli.backup || cli.clean || cli.prune) {
        // If no flags or subcommands, show help
        Cli::command().print_help()?;
        println!();
        return Err(anyhow!("No command or top-level action specified"));
    }

    Ok(())
}
