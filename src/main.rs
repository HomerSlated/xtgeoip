/// xtgeoip © Haze N Sparkle 2026 (MIT)
///
/// Downloads, extracts, and converts GeoIP CSV databases into binary IP
/// range data files, compatible with the Linux x_tables xt_geoip module,
/// which can filter packets based on GeoIP country labels.
///
/// Inspired by xt_geoip_build_maxmind (Jan Engelhardt, Philip
/// Prindeville), now part of Debian's xtables-addons package.
/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{path::Path, process};
use anyhow::{Result};
use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};
mod backup;
mod build;
mod config;
mod fetch;
mod messages;
mod cli;
mod action;

use crate::{
    cli::{Cli, enforce_flag_rules, normalize_cli_to_action},
    action::Action,
    backup::{backup, delete, prune_archives},
    build::build,
    config::{load_config, run_conf},
    fetch::{FetchMode, fetch},
    messages::{init_logger, log_early_error},
};

fn run_action(cfg: &crate::config::Config, action: Action) -> Result<()> {
    match action {
        Action::Conf(conf) => run_conf(conf)?,
        Action::TopLevelBackup { clean, force, prune } => {
            backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), force)?;
            if clean { delete(Path::new(&cfg.paths.output_dir), force)?; }
            if prune { prune_archives(cfg, true, false)?; }
        }
        Action::TopLevelClean { force } => { delete(Path::new(&cfg.paths.output_dir), force)?; }
        Action::Fetch { prune } => {
            fetch(cfg, FetchMode::Remote)?;
            if prune { prune_archives(cfg, true, false)?; }
        }
        Action::Run { backup: do_backup, clean: do_clean, force, prune, legacy } => {
            if do_backup { backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), force)?; }
            if do_clean { delete(Path::new(&cfg.paths.output_dir), force)?; }
            let (temp_dir, version) = fetch(cfg, FetchMode::Remote)?;
            build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version, legacy)?;
            if prune { prune_archives(cfg, true, false)?; }
        }
        Action::Build { backup: do_backup, clean: do_clean, force, prune, legacy } => {
            if do_backup { backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), force)?; }
            if do_clean { delete(Path::new(&cfg.paths.output_dir), force)?; }
            let (temp_dir, version) = fetch(cfg, FetchMode::Local)?;
            build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version, legacy)?;
            if prune { prune_archives(cfg, true, false)?; }
        }
    }
    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    let cfg = load_config().map_err(|e| {
        log_early_error(&format!("Failed to load config: {}", e));
        e
    })?;

    if let Some(log_file) = cfg.logging.as_ref().map(|l| l.log_file.as_str()) {
        init_logger(log_file)?;
    }

    enforce_flag_rules(&cli)?;
    let action = normalize_cli_to_action(&cli)?;

    if let Some(action) = action {
        run_action(&cfg, action)?;
    } else {
        Cli::command().print_help()?;
        println!();
        return Err(anyhow::anyhow!("No command or top-level action specified"));
    }

    Ok(())
}

fn main() -> Result<()> {
    if std::env::args_os().len() == 1 {
        let mut cmd = Cli::command();
        cmd.print_help()?;
        println!();
        process::exit(1);
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => match e.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => { e.print()?; return Ok(()); }
            _ => {
                log_early_error(&format!("CLI argument parsing failed: {}", e.kind()));
                e.print()?;
                process::exit(2);
            }
        },
    };

    if let Err(e) = run(cli) {
        if let Some(os_err) = e.downcast_ref::<std::io::Error>()
            && os_err.kind() == std::io::ErrorKind::PermissionDenied
        {
            eprintln!("Error: You must be root to run xtgeoip");
            process::exit(1);
        }
        eprintln!("Error: {}", e);
        process::exit(1);
    }

    Ok(())
}
