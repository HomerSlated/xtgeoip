/// xtgeoip © Haze N Sparkle 2026 (MIT)
///
/// Downloads, extracts, and converts GeoIP CSV databases into binary IP
/// range data files, compatible with the Linux x_tables xt_geoip module,
/// which can filter packets based on GeoIP country labels.
///
/// Inspired by xt_geoip_build_maxmind (Jan Engelhardt, Philip
/// Prindeville), now part of Debian's xtables-addons package.
use std::{path::Path, process};

use anyhow::{Result, anyhow};
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};

mod backup;
mod build;
mod config;
mod fetch;
mod messages;

use crate::{
    backup::{backup, delete, prune_archives},
    build::build,
    config::{ConfAction, load_config, run_conf},
    fetch::{FetchMode, fetch},
    messages::{init_logger, log_early_error},
};

#[derive(Parser)]
#[command(
    name = "xtgeoip",
    version,
    about = "Downloads and builds GeoIP databases",
    propagate_version = true
)]
struct Cli {
    #[arg(short, long, global = true)]
    backup: bool,
    #[arg(short, long, global = true)]
    clean: bool,
    #[arg(short, long, global = true)]
    force: bool,
    #[arg(short, long, global = true)]
    prune: bool,
    #[arg(short = 'l', long, global = true)]
    legacy: bool, // <-- now global
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long)]
        prune: bool,
        #[arg(short, long)]
        backup: bool,
        #[arg(short, long)]
        clean: bool,
        #[arg(short, long)]
        force: bool,
    },
    Build {
        #[arg(short, long)]
        backup: bool,
        #[arg(short, long)]
        clean: bool,
        #[arg(short, long)]
        force: bool,
        #[arg(short, long)]
        prune: bool,
    },
    Fetch {
        #[arg(short, long)]
        prune: bool,
    },
    #[command(group(
        clap::ArgGroup::new("conf_action")
            .required(true)
            .multiple(false)
    ))]
    Conf {
        #[arg(short = 'd', long = "default", group = "conf_action")]
        default: bool,
        #[arg(short = 's', long = "show", group = "conf_action")]
        show: bool,
        #[arg(short = 'e', long = "edit", group = "conf_action")]
        edit: bool,
    },
}

enum Action {
    Run {
        prune: bool,
        legacy: bool,
        backup: bool,
        clean: bool,
        force: bool,
    },
    Build {
        legacy: bool,
        backup: bool,
        clean: bool,
        force: bool,
        prune: bool,
    },
    Fetch {
        prune: bool,
    },
    Conf(ConfAction),
}

fn conf_action(default: bool, show: bool) -> ConfAction {
    if default {
        ConfAction::Default
    } else if show {
        ConfAction::Show
    } else {
        ConfAction::Edit
    }
}

/// Enforce global flag rules exactly per spec
fn enforce_flag_rules(cli: &Cli) -> Result<()> {
    if cli.command.is_none() {
        let b = cli.backup;
        let c = cli.clean;
        let p = cli.prune;
        let f = cli.force;

        // -p alone is invalid
        if p && !b && !c {
            return Err(anyhow!("Unsupported: -p alone is ambiguous"));
        }

        // force only applies to backup or clean
        if f && !(b || c) {
            return Err(anyhow!("--force only applies to --backup or --clean"));
        }
    }
    Ok(())
}

/// Convert CLI input into normalized Action with validation
fn normalize_cli_to_action(cli: &Cli) -> Result<Option<Action>> {
    // Reject invalid uses of --legacy
    if cli.legacy {
        match &cli.command {
            Some(Commands::Build { .. }) | Some(Commands::Run { .. }) => {} // OK
            _ => {
                return Err(anyhow!(
                    "Unsupported: --legacy only valid with build or run"
                ));
            }
        }
    }

    if let Some(cmd) = &cli.command {
        match cmd {
            Commands::Conf { default, show, edit: _ } => {
                Ok(Some(Action::Conf(conf_action(*default, *show))))
            }

            Commands::Run { prune, backup, clean, force } => {
                // Ambiguous combinations
                if *prune && *force && *clean {
                    return Err(anyhow!(
                        "Unsupported: -c -p -f combination is ambiguous in run"
                    ));
                }
                if *backup && *clean && *prune {
                    return Err(anyhow!(
                        "Unsupported: -b -c -p combination is ambiguous in run"
                    ));
                }
                Ok(Some(Action::Run {
                    prune: *prune,
                    legacy: cli.legacy,
                    backup: *backup,
                    clean: *clean,
                    force: *force,
                }))
            }

            Commands::Build { prune, force, backup, clean } => {
                // prune alone invalid
                if *prune && !*backup {
                    return Err(anyhow!(
                        "Unsupported: --prune cannot be used without --backup for build"
                    ));
                }
                // ambiguous combination
                if *prune && *force && *backup && *clean {
                    return Err(anyhow!(
                        "Unsupported: -b -c -p -f combination is ambiguous for build"
                    ));
                }
                Ok(Some(Action::Build {
                    legacy: cli.legacy,
                    backup: *backup,
                    clean: *clean,
                    force: *force,
                    prune: *prune,
                }))
            }

            Commands::Fetch { prune } => {
                if cli.backup || cli.clean {
                    return Err(anyhow!(
                        "Unsupported: -b or -c is invalid for fetch"
                    ));
                }
                Ok(Some(Action::Fetch { prune: *prune }))
            }
        }
    } else {
        // Top-level flags → synthetic Action::Run
        let b = cli.backup;
        let c = cli.clean;
        let p = cli.prune;
        let f = cli.force;

        if !b && !c && !p {
            return Ok(None);
        }

        Ok(Some(Action::Run {
            backup: b,
            clean: c,
            force: f,
            prune: p,
            legacy: cli.legacy,
        }))
    }
}

fn run_action(cfg: &crate::config::Config, action: Action) -> Result<()> {
    match action {
        Action::Conf(conf) => run_conf(conf)?,

        Action::Fetch { prune } => {
            fetch(cfg, FetchMode::Remote)?;
            if prune {
                prune_archives(cfg, true, false)?;
            }
        }

        Action::Run { backup: do_backup, clean: do_clean, force, prune, legacy } => {
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

            let (temp_dir, version) = fetch(cfg, FetchMode::Remote)?;
            build(
                temp_dir.path(),
                Path::new(&cfg.paths.output_dir),
                &version,
                legacy,
            )?;

            if prune {
                prune_archives(cfg, true, false)?;
            }
        }

        Action::Build { backup: do_backup, clean: do_clean, force, prune, legacy } => {
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

            let (temp_dir, version) = fetch(cfg, FetchMode::Local)?;
            build(
                temp_dir.path(),
                Path::new(&cfg.paths.output_dir),
                &version,
                legacy,
            )?;

            if prune {
                prune_archives(cfg, true, false)?;
            }
        }
    }

    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    let cfg = load_config().map_err(|e| {
        log_early_error(&format!("Failed to load config: {}", e));
        eprintln!("Fatal: Failed to load config: {}", e);
        e
    })?;

    if let Some(log_file) = cfg.logging.as_ref().map(|l| l.log_file.as_str()) {
        init_logger(log_file)?;
    }

    enforce_flag_rules(&cli)?;

    let action = normalize_cli_to_action(&cli)?;

    if let Some(action) = action {
        run_action(&cfg, action)?;
    } else if !(cli.backup || cli.clean || cli.prune) {
        Cli::command().print_help()?;
        println!();
        return Err(anyhow!("No command or top-level action specified"));
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
                process::exit(2);
            }
        },
    };

    if let Err(e) = run(cli) {
        if let Some(os_err) = e.downcast_ref::<std::io::Error>() {
            if os_err.kind() == std::io::ErrorKind::PermissionDenied {
                eprintln!("Error: You must be root to run xtgeoip");
                process::exit(1);
            }
        }

        // fallback for other errors
        eprintln!("Error: {}", e);
        process::exit(1);
    }

    run(cli)?;
    Ok(())
}
