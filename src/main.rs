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
use clap::{Parser, Subcommand};

use xtgeoip::{
    backup::{backup, delete, prune_archives},
    config::Config,
    messages::{init_logger, info, warn, error},
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "/etc/xtgeoip/config.toml")]
    config: String,

    /// Force operation (backup/delete) ignoring version/manifest checks
    #[arg(short, long)]
    force: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Backup binary GeoLite2 data
    Backup,

    /// Delete binary GeoLite2 data
    Delete,

    /// Prune old archives
    Prune {
        /// Prune CSV archives
        #[arg(long)]
        csv: bool,

        /// Prune bin archives
        #[arg(long)]
        bin: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logger first
    init_logger("/var/log/xtgeoip.log")?;

    info("Starting xtgeoip...");

    // Load config
    let cfg = match Config::load(&cli.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            error(&format!("Failed to load config {}: {}", cli.config, e));
            return Err(e);
        }
    };

    match cli.command {
        Commands::Backup => {
            backup(Path::new(&cfg.paths.data_dir), Path::new(&cfg.paths.archive_dir), cli.force)?;
            info("Backup completed successfully.");
        }
        Commands::Delete => {
            delete(Path::new(&cfg.paths.data_dir), cli.force)?;
            info("Deletion completed successfully.");
        }
        Commands::Prune { csv, bin } => {
            if !csv && !bin {
                warn("No prune option selected. Use --csv and/or --bin.");
            } else {
                prune_archives(&cfg, csv, bin)?;
                info("Prune operation completed successfully.");
            }
        }
    }

    Ok(())
}
