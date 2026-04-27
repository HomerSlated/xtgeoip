/// xtgeoip © Haze N Sparkle 2026 (MIT)
///
/// Downloads, extracts, and converts GeoIP CSV databases into binary IP
/// range data files, compatible with the Linux x_tables xt_geoip module,
/// which can filter packets based on GeoIP country labels.
///
/// Inspired by xt_geoip_build_maxmind (Jan Engelhardt, Philip
/// Prindeville), now part of Debian's xtables-addons package.
use std::process;

use anyhow::Result;
use clap::{CommandFactory, Parser, error::ErrorKind};

mod action;
mod backup;
mod build;
mod cli;
mod config;
mod fetch;
mod messages;
mod version;

use crate::{
    action::{Action, run_action},
    cli::{Cli, CliOutcome},
    config::load_config,
    messages::{init_logger, log_early_error},
};

const EXIT_CLI_ERROR: i32 = 2;
const EXIT_RUNTIME_ERROR: i32 = 1;

fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|uid| uid.parse::<u32>().ok())
        })
        .map(|uid| uid == 0)
        .unwrap_or(false)
}

fn run(cli: Cli) -> Result<()> {
    let outcome = cli::normalize_cli_to_action(&cli).map_err(|e| {
        eprintln!("Error: {e}");
        e
    })?;

    match outcome {
        CliOutcome::Action(Action::Conf(conf_action)) => {
            config::run_conf(conf_action)?;
        }

        CliOutcome::Action(action) => {
            if action.requires_root() && !is_root() {
                eprintln!("Error: You must be root to run xtgeoip");
                std::process::exit(EXIT_RUNTIME_ERROR);
            }
            let cfg = load_config().map_err(|e| {
                log_early_error(&format!("Failed to load config: {}", e));
                e
            })?;

            if let Some(threads) = cfg
                .processing
                .as_ref()
                .and_then(|p| p.threads)
                .filter(|&t| t > 0)
                && let Err(e) = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build_global()
            {
                messages::warn(&format!("Rayon thread pool init failed: {e}"));
            }

            if let Some(log_file) =
                cfg.logging.as_ref().map(|l| l.log_file.as_str())
            {
                init_logger(log_file)?;
            }

            run_action(&cfg, action)?;
        }

        CliOutcome::ShowHelp => {
            Cli::command().print_help()?;
            println!();
            let e = anyhow::anyhow!("No command or top-level action specified");
            eprintln!("Error: {e}");
            return Err(e);
        }
    }

    Ok(())
}

fn main() -> Result<()> {
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
                process::exit(EXIT_CLI_ERROR);
            }
        },
    };

    if run(cli).is_err() {
        process::exit(EXIT_RUNTIME_ERROR);
    }

    Ok(())
}
