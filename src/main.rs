/// xtgeoip © Haze N Sparkle 2026 (MIT)
///
/// Downloads, extracts, and converts GeoIP CSV databases into binary IP
/// range data files, compatible with the Linux x_tables xt_geoip module,
/// which can filter packets based on GeoIP country labels.
///
/// Inspired by xt_geoip_build_maxmind (Jan Engelhardt, Philip
/// Prindeville), now part of Debian's xtables-addons package.
use std::{env, path::Path};

use anyhow::{Context, Result};

mod backup;
mod build;
mod config;
mod fetch;

use crate::{
    backup::{backup, delete, prune_archives},
    config::{load_config, parse_conf_flag, run_conf},
    fetch::{FetchMode, fetch},
};

fn print_usage() {
    eprintln!(
        "Usage: xtgeoip [command] [options]\n\nCommands:\n\trun           \
         Fetch CSV archive and build binary data\n\tbuild         Build \
         binary data from latest local CSV archive\n\tfetch         Fetch CSV \
         archive only\n\tconf          Configuration operations (with \
         -d/-s/-e/-h)\n\nOptions:\n\t-b, --backup  Backup existing binary \
         files\n\t-c, --clean   Delete existing binary files\n\t-f, --force   \
         Force backup and/or clean without \
         verification\n\nExamples:\n\txtgeoip\n\txtgeoip -b\n\txtgeoip \
         -c\n\txtgeoip -b -c\n\txtgeoip -b -c -f\n\txtgeoip run\n\txtgeoip \
         run -b -c\n\txtgeoip build\n\txtgeoip build -b -c\n\txtgeoip \
         fetch\n\txtgeoip conf -h"
    );
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1); // skip executable name

    let mut force = false;
    let mut do_backup = false;
    let mut do_clean = false;
    let mut do_run = false;
    let mut do_build = false;
    let mut do_fetch = false;
    let mut do_prune = false;
    let mut first_positional: Option<String> = None;

    // Parse flags and first positional argument
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-b" | "--backup" => do_backup = true,
            "-c" | "--clean" => do_clean = true,
            "-f" | "--force" => force = true,
            "-p" | "--prune" => do_prune = true,
            "run" => do_run = true,
            "build" => do_build = true,
            "fetch" => do_fetch = true,
            "conf" => {
                // Forward remaining args to config handler
                let flag = args.next();
                let action = parse_conf_flag(flag.as_deref())
                    .map_err(|e| anyhow::anyhow!(e))?;
                run_conf(action)?;
                return Ok(());
            }
            _ => {
                first_positional = Some(arg);
                break;
            }
        }
    }

    // Unknown positional argument
    if first_positional.is_some() {
        print_usage();
        std::process::exit(1);
    }

    // Default: no args = usage
    if !do_backup && !do_clean && !do_run && !do_build && !do_fetch {
        print_usage();
        std::process::exit(1);
    }

    // Only one command allowed among run/build/fetch
    let command_count = do_run as u8 + do_build as u8 + do_fetch as u8;

    if command_count > 1 {
        print_usage();
        std::process::exit(1);
    }

    // Prune must be tied to a clear intent: fetch, run, or --backup
    if do_prune && !(do_fetch || do_run || do_backup) {
        eprintln!(
            "Error: must specify one of fetch, run, or --backup to prune old \
             archives"
        );
        std::process::exit(1);
    }

    let cfg = load_config().context("Failed to load config")?;

    // Backup/Clean only for:
    // - bare flag mode
    // - run
    // - build
    //
    // Ignored for:
    // - fetch
    // - conf (already returned above)
    let should_apply_backup_clean = !do_fetch;

    if should_apply_backup_clean && (do_backup || do_clean) {
        if do_backup {
            backup(
                Path::new(&cfg.paths.output_dir),
                Path::new(&cfg.paths.archive_dir),
                force,
            )?;
        }
        if do_clean {
            delete(Path::new(&cfg.paths.output_dir), force)?;
        }
    }

    // Fetch only
    if do_fetch {
        let _ = fetch(&cfg, FetchMode::Remote)?;
    }

    // Fetch + Build
    if do_run {
        let (temp_dir, version) = fetch(&cfg, FetchMode::Remote)?;
        build::build(
            temp_dir.path(),
            Path::new(&cfg.paths.output_dir),
            &version,
        )?;
    }

    // Build from latest local archive
    if do_build {
        let (temp_dir, version) = fetch(&cfg, FetchMode::Local)?;
        build::build(
            temp_dir.path(),
            Path::new(&cfg.paths.output_dir),
            &version,
        )?;
    }

    // Pruning:
    // - Fetch or run  => prune CSV archives
    // - Backup        => prune bin archives
    // - Both          => prune both CSV and bin archives
    if do_prune {
        let prune_csv = do_fetch || do_run;
        let prune_bin = do_backup;
        prune_archives(&cfg, prune_csv, prune_bin)?;
    }

    Ok(())
}
