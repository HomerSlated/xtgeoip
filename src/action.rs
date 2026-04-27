/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip Actions
/// xtgeoip action runner
/// Handles all Action enum variants
use std::path::Path;

use anyhow::Result;

use crate::{
    backup::{BackupMode, PruneMode, backup, delete, prune_archives},
    build::build,
    config::{ConfAction, Config},
    fetch::{FetchMode, fetch},
    messages,
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
}

impl Action {
    pub fn requires_root(&self) -> bool {
        !matches!(self, Action::Conf(_))
    }
}

pub fn run_action(cfg: &Config, action: Action) -> Result<()> {
    match action {
        Action::TopLevelBackup {
            clean,
            force,
            prune,
        } => {
            let mode = if force {
                BackupMode::Force
            } else {
                BackupMode::Verified
            };
            messages::info("Backing up database...");
            backup(
                Path::new(&cfg.paths.output_dir),
                Path::new(&cfg.paths.archive_dir),
                mode,
            )?;

            if prune {
                messages::info("Pruning bin archives...");
                prune_archives(cfg, PruneMode::Bin)?;
            }

            if clean {
                messages::info("Cleaning output directory...");
                delete(Path::new(&cfg.paths.output_dir), mode)?;
            }
        }

        Action::TopLevelClean { force } => {
            let mode = if force {
                BackupMode::Force
            } else {
                BackupMode::Verified
            };
            messages::info("Cleaning output directory...");
            delete(Path::new(&cfg.paths.output_dir), mode)?;
        }

        Action::Fetch { prune } => {
            fetch(cfg, FetchMode::Remote)?;
            if prune {
                messages::info("Pruning CSV archives...");
                prune_archives(cfg, PruneMode::Csv)?;
            }
        }

        Action::Run {
            backup: do_backup,
            clean: do_clean,
            force,
            prune,
            legacy,
        } => {
            let mode = if force {
                BackupMode::Force
            } else {
                BackupMode::Verified
            };
            if do_backup {
                messages::info("Backing up database...");
                backup(
                    Path::new(&cfg.paths.output_dir),
                    Path::new(&cfg.paths.archive_dir),
                    mode,
                )?;
            }

            if do_clean {
                messages::info("Cleaning output directory...");
                delete(Path::new(&cfg.paths.output_dir), mode)?;
            }

            let (temp_dir, version) = fetch(cfg, FetchMode::Remote)?;

            if prune {
                messages::info("Pruning CSV archives...");
                prune_archives(cfg, PruneMode::Csv)?;
            }

            messages::info("Building binary database...");
            build(
                temp_dir.path(),
                Path::new(&cfg.paths.output_dir),
                &version,
                legacy,
            )?;
        }

        Action::Build {
            backup: do_backup,
            clean: do_clean,
            force,
            prune,
            legacy,
        } => {
            let mode = if force {
                BackupMode::Force
            } else {
                BackupMode::Verified
            };
            if do_backup {
                messages::info("Backing up database...");
                backup(
                    Path::new(&cfg.paths.output_dir),
                    Path::new(&cfg.paths.archive_dir),
                    mode,
                )?;

                if prune {
                    messages::info("Pruning bin archives...");
                    prune_archives(cfg, PruneMode::Bin)?;
                }
            }

            if do_clean {
                messages::info("Cleaning output directory...");
                delete(Path::new(&cfg.paths.output_dir), mode)?;
            }

            let (temp_dir, version) = fetch(cfg, FetchMode::Local)?;
            messages::info("Building binary database...");
            build(
                temp_dir.path(),
                Path::new(&cfg.paths.output_dir),
                &version,
                legacy,
            )?;
        }

        Action::Conf(_) => unreachable!("Conf is handled before run_action"),
    }

    Ok(())
}
