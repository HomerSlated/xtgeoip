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
use clap::{Parser, Subcommand, CommandFactory};

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

    /// Force backup and/or clean without verification (only affects backup/clean)
    #[arg(short, long)]
    force: bool,

    /// Prune old archives/backups (requires backup, fetch, or run)
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
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = load_config().context("Failed to load config")?;

    // Extract bare flags
    let do_backup = cli.backup;
    let do_clean = cli.clean;
    let do_force = cli.force;
    let do_prune = cli.prune;

    match &cli.command {
        Some(Commands::Conf { flag }) => {
            // Conf ignores all flags; only the positional flag matters
            let action = parse_conf_flag(Some(flag))
                .map_err(|e| anyhow::anyhow!(e))?;
            run_conf(action)?;
        }

        Some(Commands::Run) => {
            if do_force && !do_backup && !do_clean {
                eprintln!("Error: --force can only be used with --backup or --clean");
                std::process::exit(1);
            }

            if do_prune && !(do_backup || true) { // run implies fetch
                eprintln!("Error: --prune requires backup, fetch, or run");
                std::process::exit(1);
            }

            let (temp_dir, version) = fetch(&cfg, FetchMode::Remote)?;
            build::build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version)?;

            if do_backup {
                backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), do_force)?;
            }
            if do_clean {
                delete(Path::new(&cfg.paths.output_dir), do_force)?;
            }
            if do_prune {
                prune_archives(&cfg, true, do_backup)?;
            }
        }

        Some(Commands::Build) => {
            if do_force && !do_backup && !do_clean {
                eprintln!("Error: --force can only be used with --backup or --clean");
                std::process::exit(1);
            }

            if do_prune && !(do_backup) {
                eprintln!("Error: --prune requires backup, fetch, or run");
                std::process::exit(1);
            }

            let (temp_dir, version) = fetch(&cfg, FetchMode::Local)?;
            build::build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version)?;

            if do_backup {
                backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), do_force)?;
            }
            if do_clean {
                delete(Path::new(&cfg.paths.output_dir), do_force)?;
            }
            if do_prune {
                prune_archives(&cfg, false, do_backup)?;
            }
        }

        Some(Commands::Fetch) => {
            if do_force {
                eprintln!("Error: --force can only be used with --backup or --clean");
                std::process::exit(1);
            }

            if do_prune && !(do_backup || true) { // fetch itself allows prune
                eprintln!("Error: --prune requires backup, fetch, or run");
                std::process::exit(1);
            }

            let _ = fetch(&cfg, FetchMode::Remote)?;

            if do_prune {
                prune_archives(&cfg, true, do_backup)?;
            }
        }

        None => {
            // Bare flags only
            if do_force && !do_backup && !do_clean {
                eprintln!("Error: --force can only be used with --backup or --clean");
                std::process::exit(1);
            }

            if do_prune && !(do_backup) {
                eprintln!("Error: --prune requires backup, fetch, or run");
                std::process::exit(1);
            }

            if !do_backup && !do_clean && !do_prune {
                Cli::command().print_help()?;
                println!();
                std::process::exit(1);
            }

            if do_backup {
                backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), do_force)?;
            }
            if do_clean {
                delete(Path::new(&cfg.paths.output_dir), do_force)?;
            }
            if do_prune {
                prune_archives(&cfg, false, do_backup)?;
            }
        }
    }

    Ok(())
}
