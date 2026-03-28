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
#[command(name = "xtgeoip")]
#[command(about = "Downloads and builds GeoIP databases", long_about = None)]
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

    /// Prune old archives/backups
    #[arg(short, long)]
    prune: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch CSV archive and build binary data
    Run,
    /// Build binary data from latest local CSV archive
    Build,
    /// Fetch CSV archive only
    Fetch,
    /// Configuration operations (-d/-s/-e/-h)
    Conf {
        /// Configuration flag
        flag: String,
        /// Backup existing binary files
        #[arg(short, long)]
        backup: bool,
        /// Delete existing binary files
        #[arg(short, long)]
        clean: bool,
        /// Force backup and/or clean without verification
        #[arg(short, long)]
        force: bool,
        /// Prune old archives/backups
        #[arg(short, long)]
        prune: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let cfg = load_config().context("Failed to load config")?;

    // Apply backup/clean for global flags (outside subcommands) when no subcommand
    if cli.command.is_none() && (cli.backup || cli.clean) {
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
    }

    // Handle subcommands
    match cli.command {
        Some(Commands::Run) => {
            if cli.backup || cli.clean {
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
            }
            let (temp_dir, version) = fetch(&cfg, FetchMode::Remote)?;
            build::build(
                temp_dir.path(),
                Path::new(&cfg.paths.output_dir),
                &version,
            )?;
            if cli.prune {
                prune_archives(&cfg, true, cli.backup)?;
            }
        }
        Some(Commands::Build) => {
            if cli.backup || cli.clean {
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
            }
            let (temp_dir, version) = fetch(&cfg, FetchMode::Local)?;
            build::build(
                temp_dir.path(),
                Path::new(&cfg.paths.output_dir),
                &version,
            )?;
        }
        Some(Commands::Fetch) => {
            let _ = fetch(&cfg, FetchMode::Remote)?;
            if cli.prune {
                prune_archives(&cfg, true, false)?;
            }
        }
        Some(Commands::Conf {
            flag,
            backup,
            clean,
            force,
            prune,
        }) => {
            let action = parse_conf_flag(Some(&flag)).map_err(|e| anyhow::anyhow!(e))?;
            run_conf(action)?;

            // Apply optional flags for conf
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
            if prune {
                prune_archives(&cfg, false, backup)?;
            }
        }
        None => {
            if !cli.backup && !cli.clean && !cli.prune {
                // No subcommand and no flags => print help
                Cli::command().print_help()?;
                println!();
            }
        }
    }

    Ok(())
}
