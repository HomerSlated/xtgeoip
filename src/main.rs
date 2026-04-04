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
use clap::{Parser, Subcommand};

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
    messages::{init_logger, log_early_error, warn},
};

/// CLI top-level parser
#[derive(Parser)]
#[command(
    name = "xtgeoip",
    version,
    about = "Downloads and builds GeoIP databases",
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Subcommands
#[derive(Subcommand)]
enum Commands {
    /// Fetch and build (optionally prune/backup/clean)
    Run {
        #[arg(short, long)]
        prune: bool,
        #[arg(short = 'l', long)]
        legacy: bool,
        #[arg(short, long)]
        backup: bool,
        #[arg(short, long)]
        clean: bool,
        #[arg(short, long)]
        force: bool,
    },

    /// Build using local CSV
    Build {
        #[arg(short = 'l', long)]
        legacy: bool,
        #[arg(short, long)]
        backup: bool,
        #[arg(short, long)]
        clean: bool,
        #[arg(short, long)]
        force: bool,
        #[arg(short, long)]
        prune: bool,
    },

    /// Fetch remote CSV archive
    Fetch {
        #[arg(short, long)]
        prune: bool,
    },

    /// Configuration operations
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

/// Enforce rules for backup/prune/force per command
fn enforce_flag_rules(backup: bool, clean: bool, prune: bool, force: bool) -> Result<()> {
    if force && !(backup || clean) {
        return Err(anyhow!("--force only applies to --backup or --clean"));
    }
    if prune && !backup {
        return Err(anyhow!("--prune requires --backup"));
    }
    Ok(())
}

/// Warn user if legacy mode is enabled
fn warn_legacy_mode(legacy: bool) {
    if legacy {
        warn(
            "Warning: Legacy Mode activated. See documentation for collisions.",
        );
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // If no command, show top-level help and exit
    if cli.command.is_none() {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    }

    run(cli)?;

    Ok(())
}

/// Dispatch CLI commands
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

    match cli.command {
        Some(Commands::Run { prune, legacy, backup, clean, force }) => {
            enforce_flag_rules(backup, clean, prune, force)?;
            if backup {
                backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), force)?;
            }
            if clean {
                delete(Path::new(&cfg.paths.output_dir), force)?;
            }

            let (temp_dir, version) = fetch(&cfg, FetchMode::Remote)?;
            warn_legacy_mode(legacy);
            build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version, legacy)?;

            if prune {
                prune_archives(&cfg, true, false)?;
            }
        }

        Some(Commands::Build { legacy, backup, clean, force, prune }) => {
            enforce_flag_rules(backup, clean, prune, force)?;
            if backup {
                backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), force)?;
            }
            if clean {
                delete(Path::new(&cfg.paths.output_dir), force)?;
            }
            if prune {
                prune_archives(&cfg, false, backup)?;
            }

            let (temp_dir, version) = fetch(&cfg, FetchMode::Local)?;
            warn_legacy_mode(legacy);
            build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version, legacy)?;
        }

        Some(Commands::Fetch { prune }) => {
            let _ = fetch(&cfg, FetchMode::Remote)?;
            if prune {
                prune_archives(&cfg, true, false)?;
            }
        }

        Some(Commands::Conf { default, show, edit: _ }) => {
            run_conf(conf_action(default, show))?;
        }

        None => unreachable!("Handled top-level None above"),
    }

    Ok(())
}
