/// xtgeoip © Haze N Sparkle 2026 (MIT)
///
/// Downloads, extracts, and converts GeoIP CSV databases into binary IP
/// range data files, compatible with the Linux x_tables xt_geoip module,
/// which can filter packets based on GeoIP country labels.
///
/// Inspired by xt_geoip_build_maxmind (Jan Engelhardt, Philip
/// Prindeville), now part of Debian's xtables-addons package.
/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::process;

use anyhow::{Result, anyhow};
use clap::{CommandFactory, Parser, error::ErrorKind};

mod action;
mod backup;
mod build;
mod cli;
mod config;
mod fetch;
mod messages;

use crate::{
    action::{Action, run_action},
    cli::Cli,
    config::load_config,
    messages::{error, init_logger, log_early_error},
};

fn normalize_cli_to_action(cli: &Cli) -> Result<Option<Action>> {
    crate::cli::normalize_cli_to_action(cli)
}

fn run(cli: Cli) -> Result<()> {
    let cfg = load_config().map_err(|e| {
        log_early_error(&format!("Failed to load config: {}", e));
        e
    })?;

    if let Some(log_file) = cfg.logging.as_ref().map(|l| l.log_file.as_str()) {
        init_logger(log_file)?;
    }

    let action = normalize_cli_to_action(&cli)?;

    if let Some(action) = action {
        run_action(&cfg, action)?;
    } else {
        Cli::command().print_help()?;
        println!();
        let err = anyhow!("No command or top-level action specified");
        error(&err.to_string());
        return Err(err);
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
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                e.print()?;
                return Ok(());
            }
            _ => {
                log_early_error(&format!(
                    "CLI argument parsing failed: {}",
                    e.kind()
                ));
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
