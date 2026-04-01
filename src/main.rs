/// xtgeoip © Haze N Sparkle 2026 (MIT)
///
/// Downloads, extracts, and converts GeoIP CSV databases into binary IP
/// range data files, compatible with the Linux x_tables xt_geoip module,
/// which can filter packets based on GeoIP country labels.
///
/// Inspired by xt_geoip_build_maxmind (Jan Engelhardt, Philip
/// Prindeville), now part of Debian's xtables-addons package.
use std::path::Path;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};

mod backup;
mod build;
mod config;
mod fetch;
mod messages;

use crate::{
    backup::{backup, delete, prune_archives},
    build::build,
    config::{load_config, run_conf, ConfAction},
    fetch::{fetch, FetchMode},
    messages::{init_logger, log_config_failure, log_print, warn},
};

#[derive(Parser)]
#[command(
    name = "xtgeoip",
    version,
    about = "Downloads and builds GeoIP databases"
)]
struct Cli {
    /// Backup existing binary files
    #[arg(short, long)]
    backup: bool,

    /// Delete existing binary files
    #[arg(short, long)]
    clean: bool,

    /// Force backup and/or clean without verification
    #[arg(short, long)]
    force: bool,

    /// Prune old binary archives (requires backup)
    #[arg(short, long)]
    prune: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long)]
        prune: bool,

        /// Use legacy mode
        #[arg(short = 'l', long)]
        legacy: bool,
    },
    Build {
        #[arg(short, long)]
        backup: bool,
        #[arg(short, long)]
        clean: bool,
        #[arg(short, long)]
        force: bool,

        /// Use legacy MaxMind-compatible continent mappings (EU/AS) instead of
        /// O1
        #[arg(
            short = 'l',
            long,
            help = "Enable legacy MaxMind-compatible continent mappings (e.g. \
                    EU/AS) instead of O1"
        )]
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

    // Handle `xtgeoip conf` subcommand before loading config
    if let Some(Commands::Conf {
        default,
        show,
        edit: _,
    }) = &cli.command
    {
        let action = if *default {
            ConfAction::Default
        } else if *show {
            ConfAction::Show
        } else {
            ConfAction::Edit
        };
        run_conf(action)?;
        return Ok(());
    }

    // Load system configuration
    let cfg = match load_config() {
        Ok(c) => c,
        Err(e) => {
            log_config_failure(&format!("Failed to load config: {}", e));
            eprintln!("Fatal: Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

    // Initialize normal logging using configured log file
    if let Some(log_file) = cfg.logging.as_ref().map(|l| l.log_file.as_str()) {
        init_logger(log_file)?;
    }

    // Enforce flag rules
    if cli.force && !(cli.backup || cli.clean) {
        log_print(
            "Error: --force only applies to --backup or --clean",
            log::Level::Error,
        );
        std::process::exit(1);
    }

    if cli.prune && !cli.backup {
        log_print(
            "Error: --prune requires --backup at top-level",
            log::Level::Error,
        );
        std::process::exit(1);
    }

    // Handle top-level flags (backup/clean/prune)
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

    // Handle subcommands
    match &cli.command {
        Some(Commands::Run { prune, legacy }) => {
            let (temp_dir, version) = fetch(&cfg, FetchMode::Remote)?;
            warn_legacy_mode(*legacy);
            build(
                temp_dir.path(),
                Path::new(&cfg.paths.output_dir),
                &version,
                *legacy,
            )?;
            if *prune {
                prune_archives(&cfg, true, false)?;
            }
        }
        Some(Commands::Build {
            backup: do_backup,
            clean: do_clean,
            force: do_force,
            legacy,
        }) => {
            let (temp_dir, version) = fetch(&cfg, FetchMode::Local)?;
            warn_legacy_mode(*legacy);
            build(
                temp_dir.path(),
                Path::new(&cfg.paths.output_dir),
                &version,
                *legacy,
            )?;
            if *do_backup {
                backup(
                    Path::new(&cfg.paths.output_dir),
                    Path::new(&cfg.paths.archive_dir),
                    *do_force,
                )?;
            }
            if *do_clean {
                delete(Path::new(&cfg.paths.output_dir), *do_force)?;
            }
        }
        Some(Commands::Fetch { prune }) => {
            let _ = fetch(&cfg, FetchMode::Remote)?;
            if *prune {
                prune_archives(&cfg, true, false)?;
            }
        }
        None => {
            if !(cli.backup || cli.clean || cli.prune) {
                Cli::command().print_help()?;
                println!();
                std::process::exit(1);
            }
        }
        Some(Commands::Conf { .. }) => unreachable!(), // already handled above
    }

    Ok(())
}
