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

/// Enforce rules for global flags
fn enforce_flag_rules(cli: &Cli) -> Result<()> {
    if cli.force && !(cli.backup || cli.clean) {
        error("Error: --force only applies to --backup or --clean");
        return Err(anyhow!("--force only applies to --backup or --clean"));
    }

    if cli.prune && !cli.backup {
        error("Error: --prune requires --backup");
        return Err(anyhow!("--prune requires --backup"));
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
    // Load system configuration (needed for most actions)
    let cfg = load_config().map_err(|e| {
        log_early_error(&format!("Failed to load config: {}", e));
        eprintln!("Fatal: Failed to load config: {}", e);
        e
    })?;

    // Initialize logging
    if let Some(log_file) = cfg.logging.as_ref().map(|l| l.log_file.as_str()) {
        init_logger(log_file)?;
    }

    // Enforce top-level flag rules
    enforce_flag_rules(&cli)?;

    // First, handle global flags in order: backup → clean → prune
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
