/// xtgeoip © Haze N Sparkle 2026 (MIT)
///
/// Downloads, extracts, and converts GeoIP CSV databases into binary IP
/// range data files, compatible with the Linux x_tables xt_geoip module,
/// which can filter packets based on GeoIP country labels.
///
/// Inspired by xt_geoip_build_maxmind (Jan Engelhardt, Philip
/// Prindeville), now part of Debian's xtables-addons package.
use std::path::Path;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

mod backup;
mod build;
mod config;
mod fetch;

use crate::{
    backup::{backup, delete, prune_archives},
    config::{load_config, parse_conf_flag, run_conf},
    fetch::{FetchMode, fetch},
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
    /// Fetch CSV archive and build binary data
    Run {
        /// Prune old CSV archives
        #[arg(short, long)]
        prune: bool,
    },
    /// Build binary data from latest local CSV archive
    Build {
        /// Backup existing binary files
        #[arg(short, long)]
        backup: bool,

        /// Delete existing binary files
        #[arg(short, long)]
        clean: bool,

        /// Force backup and/or clean without verification
        #[arg(short, long)]
        force: bool,
    },
    /// Fetch CSV archive only
    Fetch {
        /// Prune old CSV archives
        #[arg(short, long)]
        prune: bool,
    },
    /// Configuration operations (-d/-s/-e/-h)
    Conf {
        /// Configuration flag
        #[arg(value_name = "FLAG", allow_hyphen_values = true)]
        flag: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = load_config().context("Failed to load config")?;

    // Enforce flag rules
    if cli.force && !(cli.backup || cli.clean) {
        eprintln!("Error: --force only applies to --backup or --clean");
        std::process::exit(1);
    }

    if cli.prune && !cli.backup {
        eprintln!("Error: --prune requires --backup at top-level");
        std::process::exit(1);
    }

    // Handle top-level flags (backup/clean/prune)
    if cli.backup {
        backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), cli.force)?;
    }
    if cli.clean {
        delete(Path::new(&cfg.paths.output_dir), cli.force)?;
    }
    if cli.prune {
        prune_archives(&cfg, false, cli.backup)?;
    }

    // Handle subcommands
    match &cli.command {
        Some(Commands::Run { prune }) => {
            let (temp_dir, version) = fetch(&cfg, FetchMode::Remote)?;
            build::build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version)?;
            if *prune {
                prune_archives(&cfg, true, false)?;
            }
        }
        Some(Commands::Build { backup, clean, force }) => {
            let (temp_dir, version) = fetch(&cfg, FetchMode::Local)?;
            build::build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version)?;
            if *backup {
                backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), *force)?;
            }
            if *clean {
                delete(Path::new(&cfg.paths.output_dir), *force)?;
            }
        }
        Some(Commands::Fetch { prune }) => {
            let _ = fetch(&cfg, FetchMode::Remote)?;
            if *prune {
                prune_archives(&cfg, true, false)?;
            }
        }
        Some(Commands::Conf { flag }) => {
            let action = parse_conf_flag(Some(flag))
                .map_err(|e| anyhow::anyhow!(e))?;
            run_conf(action)?;
        }
        None => {
            if !(cli.backup || cli.clean || cli.prune) {
                Cli::command().print_help()?;
                println!();
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
