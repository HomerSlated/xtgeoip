/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip Actions
/// xtgeoip action runner
/// Handles all Action enum variants
use std::path::Path;

use anyhow::Result;

use crate::{
    backup::{backup, delete, prune_archives},
    build::build,
    config::{ConfAction, Config, run_conf},
    fetch::{FetchMode, fetch},
};

#[derive(Debug)]
pub enum Action {
    TopLevelBackup {
        clean: bool,
        force: bool,
        prune: bool,
    },
    TopLevelClean {
        force: bool,
    },
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
    NoArgs,
}

pub fn run_action(cfg: &Config, action: Action) -> Result<()> {
    match action {
        Action::Conf(conf) => run_conf(conf)?,

        Action::TopLevelBackup {
            clean,
            force,
            prune,
        } => {
            backup(
                Path::new(&cfg.paths.output_dir),
                Path::new(&cfg.paths.archive_dir),
                force,
            )?;

            if clean {
                delete(Path::new(&cfg.paths.output_dir), force)?;
            }

            if prune {
                prune_archives(cfg, true, false)?;
            }
        }

        Action::TopLevelClean { force } => {
            delete(Path::new(&cfg.paths.output_dir), force)?;
        }

        Action::Fetch { prune } => {
            fetch(cfg, FetchMode::Remote)?;
            if prune {
                prune_archives(cfg, true, false)?;
            }
        }

        Action::Run {
            backup: do_backup,
            clean: do_clean,
            force,
            prune,
            legacy,
        } => {
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

        Action::Build {
            backup: do_backup,
            clean: do_clean,
            force,
            prune,
            legacy,
        } => {
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

        Action::NoArgs => {
           anyhow::bail!("No command or top-level action specified");
        }
    }

    Ok(())
}
