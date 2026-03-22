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
    backup::{backup, delete},
    config::{load_config, parse_conf_flag, run_conf},
    fetch::fetch,
};

fn print_usage() {
    eprintln!(
        "Usage: xtgeoip <command> [options]\n\nCommands:\n\trun           \
         Fetch CSV archive and build binary data\n\tconf          \
         Configuration operations (with -d/-s/-e/-h)\n\nOptions:\n\t-b, \
         --backup  Backup existing binary files\n\t-d, --delete  Delete \
         existing binary files\n\t-f, --force   Force backup and/or delete \
         without verification\n\nExamples:\n\txtgeoip run\n\txtgeoip \
         -b\n\txtgeoip -d\n\txtgeoip -b -d\n\txtgeoip -b -d -f\n\txtgeoip run \
         -b -d\n\txtgeoip conf -h"
    );
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1); // skip executable name

    let mut force = false;
    let mut do_backup = false;
    let mut do_delete = false;
    let mut run_build = false;
    let mut first_positional: Option<String> = None;

    // Parse flags and first positional argument
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-b" | "--backup" => do_backup = true,
            "-d" | "--delete" => do_delete = true,
            "-f" | "--force" => force = true,
            "run" => run_build = true,
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

    // Validate: must specify at least one valid action
    if !do_backup && !do_delete && !run_build && first_positional.is_none() {
        print_usage();
        std::process::exit(1);
    }

    let cfg = load_config().context("Failed to load config")?;

    // Backup/Delete only (optional)
    if do_backup || do_delete {
        let version_path = Path::new(&cfg.paths.output_dir).join("version");
        let _version_old = std::fs::read_to_string(&version_path)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown".into());
        if do_backup {
            backup(
                Path::new(&cfg.paths.output_dir),
                Path::new(&cfg.paths.archive_dir),
                force,
            )?;
        }
        if do_delete {
            delete(Path::new(&cfg.paths.output_dir), force)?;
        }
    }

    // Fetch + Build (explicit 'run' command)
    if run_build {
        let (temp_dir, version) = fetch(&cfg)?;
        build::build(
            temp_dir.path(),
            Path::new(&cfg.paths.output_dir),
            &version,
        )?;
    }

    Ok(())
}
