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
mod conf;
mod config;
mod fetch;
mod generated;
mod messages;
mod secrets;
mod version;

use crate::{
    action::{Action, run_action},
    cli::{Cli, CliOutcome},
    config::load_config,
    messages::{init_logger, log_early_error, resolve_log_file},
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

fn init_runtime(cfg: &config::Config) -> Result<()> {
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

    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    let outcome = cli::normalize_cli_to_action(&cli).map_err(|e| {
        eprintln!("Error: {e}");
        e
    })?;

    match outcome {
        CliOutcome::Action(Action::Conf(conf_action)) => {
            // conf runs before config load, so it has no configured
            // log-file path — but an explicit `--log-file` is known already
            // and takes precedence, so honour it here too rather than
            // silently ignoring the flag on one subcommand.
            init_logger(
                resolve_log_file(cli.no_log, cli.log_file.as_deref(), None)
                    .as_deref(),
            )?;
            conf::run_conf(conf_action)?;
        }

        CliOutcome::Action(action) => {
            if action.requires_root() && !is_root() {
                eprintln!("Error: You must be root to run xtgeoip");
                std::process::exit(EXIT_RUNTIME_ERROR);
            }
            // The logger must be installed *before* `load_config` is
            // attempted, not after — otherwise a config-load failure (bad
            // TOML, missing file, unknown field, ...) propagates through
            // `messages::error` in `main`'s catch-all while the global
            // `log` logger is still unset, and the `log` crate silently
            // drops every message rather than erroring. `log_file` is only
            // known once config has loaded, so it's `None` (terminal-only)
            // exactly when a load failure is what we're trying to report.
            let cfg_result = load_config();
            let configured = cfg_result
                .as_ref()
                .ok()
                .and_then(|c| c.logging.as_ref())
                .map(|l| l.log_file.clone());
            // `--log-file`/`--no-log` override `[logging]` (#1 residual). The
            // override is also the only path that can log a *failed* load:
            // `configured` is `None` in exactly that case, because the
            // configured path is not known until the load succeeds.
            let log_file = resolve_log_file(
                cli.no_log,
                cli.log_file.as_deref(),
                configured.as_deref(),
            );
            init_logger(log_file.as_deref())?;
            let cfg = cfg_result.map_err(|e| {
                log_early_error(&format!("Failed to load config: {}", e));
                e
            })?;

            init_runtime(&cfg)?;
            run_action(&cfg, action)?;
        }

        CliOutcome::ShowHelp => {
            Cli::command().print_help()?;
            println!();
            let e = anyhow::anyhow!("No command or top-level action specified");
            eprintln!("Error [top_level_no_args]: {e}");
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

    if let Err(e) = run(cli) {
        // `{e:#}` prints anyhow's whole cause chain, and that is deliberate:
        // fetch/build/backup rely on `.context()` to turn a bare syscall
        // failure into something actionable, and `{e}` would drop all of it.
        //
        // The cost is that this funnel cannot know how sensitive any link in
        // a chain is, so the rule is that errors are sanitized where they are
        // *made*, not where they are printed — see `config::parse_config`,
        // which #104 fixed after `toml::de::Error` was found carrying (and
        // quoting) the raw config file, plaintext credentials included.
        messages::error(&format!("{e:#}"));
        process::exit(EXIT_RUNTIME_ERROR);
    }

    Ok(())
}
