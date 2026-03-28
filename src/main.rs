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
use clap::{Arg, ArgAction, Command};

mod backup;
mod build;
mod config;
mod fetch;

use crate::{
    backup::{backup, delete, prune_archives},
    config::{load_config, parse_conf_flag, run_conf},
    fetch::{fetch, FetchMode},
};

fn build_cli() -> Command {
    Command::new("xtgeoip")
        .disable_help_subcommand(true)
        .arg(
            Arg::new("backup")
                .short('b')
                .long("backup")
                .help("Backup existing binary files")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("clean")
                .short('c')
                .long("clean")
                .help("Delete existing binary files")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .help("Force backup and/or clean without verification")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("prune")
                .short('p')
                .long("prune")
                .help("Prune old archives/backups")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("command")
                .value_parser(["run", "build", "fetch", "conf"])
                .required(false),
        )
        .arg(
            Arg::new("conf_flag")
                .required(false)
                .allow_hyphen_values(true),
        )
}

fn print_usage(mut cmd: Command) -> Result<()> {
    cmd.print_help()?;
    eprintln!();
    Ok(())
}

fn main() -> Result<()> {
    let cmd = build_cli();
    let matches = cmd.clone().get_matches();

    let force = matches.get_flag("force");
    let do_backup = matches.get_flag("backup");
    let do_clean = matches.get_flag("clean");
    let do_prune = matches.get_flag("prune");

    let command = matches.get_one::<String>("command").map(String::as_str);
    let conf_flag = matches
        .get_one::<String>("conf_flag")
        .map(String::as_str);

    let do_run = command == Some("run");
    let do_build = command == Some("build");
    let do_fetch = command == Some("fetch");

    // conf: preserve existing behavior by forwarding optional -d/-s/-e/-h
    if command == Some("conf") {
        let action = parse_conf_flag(conf_flag).map_err(|e| anyhow::anyhow!(e))?;
        run_conf(action)?;
        return Ok(());
    }

    // Default: no args = usage
    if !do_backup && !do_clean && !do_run && !do_build && !do_fetch {
        print_usage(cmd.clone())?;
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
