/// xtgeoip © Haze N Sparkle 2026 (MIT)
///
/// Downloads, extracts, and converts GeoIP CSV databases into binary IP
/// range data files, compatible with the Linux x_tables xt_geoip module,
/// which can filter packets based on GeoIP country labels.
///
/// Inspired by xt_geoip_build_maxmind (Jan Engelhardt, Philip
/// Prindeville), now part of Debian's xtables-addons package.
/// xtgeoip © Haze N Sparkle 2026 (MIT)
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
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch, then build, optionally wrapping with backup/clean/prune.
    Run {
        #[arg(short, long)]
        backup: bool,

        #[arg(short, long)]
        clean: bool,

        #[arg(short, long)]
        prune: bool,

        #[arg(short, long)]
        force: bool,

        #[arg(short = 'l', long)]
        legacy: bool,
    },

    /// Build xt_geoip data from the local CSV copy.
    Build {
        #[arg(short, long)]
        backup: bool,

        #[arg(short, long)]
        clean: bool,

        #[arg(short, long)]
        prune: bool,

        #[arg(short, long)]
        force: bool,

        #[arg(short = 'l', long)]
        legacy: bool,
    },

    /// Download or refresh the local MaxMind CSV archive set.
    Fetch {
        #[arg(short, long)]
        prune: bool,
    },

    /// Configuration operations.
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

// Normalized CLI action type for internal dispatch
enum Action {
    Run {
        backup: bool,
        clean: bool,
        prune: bool,
        force: bool,
        legacy: bool,
    },
    Build {
        backup: bool,
        clean: bool,
        prune: bool,
        force: bool,
        legacy: bool,
    },
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

/// Enforce rules for Run/Build flags
fn enforce_flag_rules_backup_clean(
    backup: bool,
    clean: bool,
    prune: bool,
    force: bool,
) -> Result<()> {
    if force && !(backup || clean) {
        return Err(anyhow!("--force only applies to --backup or --clean"));
    }

    if prune && !backup {
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
    let cfg = load_config().map_err(|e| {
        log_early_error(&format!("Failed to load config: {}", e));
        eprintln!("Fatal: Failed to load config: {}", e);
        e
    })?;

    if let Some(log_file) = cfg.logging.as_ref().map(|l| l.log_file.as_str()) {
        init_logger(log_file)?;
    }

    // Dispatch subcommands
    match cli.command {
        Commands::Run {
            backup,
            clean,
            prune,
            force,
            legacy,
        } => {
            enforce_flag_rules_backup_clean(backup, clean, prune, force)?;
            if backup {
                backup(
                    Path::new(&cfg.paths.output_dir),
                    Path::new(&cfg.paths.archive_dir),
                    force,
                )?;
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

        Commands::Build {
            backup,
            clean,
            prune,
            force,
            legacy,
        } => {
            enforce_flag_rules_backup_clean(backup, clean, prune, force)?;
            if backup {
                backup(
                    Path::new(&cfg.paths.output_dir),
                    Path::new(&cfg.paths.archive_dir),
                    force,
                )?;
            }
            if clean {
                delete(Path::new(&cfg.paths.output_dir), force)?;
            }
            let (temp_dir, version) = fetch(&cfg, FetchMode::Local)?;
            warn_legacy_mode(legacy);
            build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version, legacy)?;
            if prune {
                prune_archives(&cfg, false, backup)?;
            }
        }

        Commands::Fetch { prune } => {
            let _ = fetch(&cfg, FetchMode::Remote)?;
            if prune {
                prune_archives(&cfg, true, false)?;
            }
        }

        Commands::Conf { default, show, edit: _ } => {
            let conf = conf_action(default, show);
            run_conf(conf)?;
        }
    }

    Ok(())
}
