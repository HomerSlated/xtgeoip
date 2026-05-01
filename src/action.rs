/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip action runner
use std::path::Path;

use anyhow::Result;
use tempfile::TempDir;

use crate::{
    backup::{BackupMode, PruneMode, backup, delete, prune_archives},
    build::build,
    config::{ConfAction, Config},
    fetch::{FetchMode, fetch},
    messages,
    version::Version,
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

struct ResolvedPaths<'a> {
    output: &'a Path,
    archive: &'a Path,
}

fn resolve_paths(cfg: &Config) -> ResolvedPaths<'_> {
    ResolvedPaths {
        output: Path::new(&cfg.paths.output_dir),
        archive: Path::new(&cfg.paths.archive_dir),
    }
}

#[derive(Clone, Copy)]
enum Step {
    Backup { mode: BackupMode },
    Clean { mode: BackupMode },
    Fetch { mode: FetchMode },
    PruneCsv,
    PruneBin,
    Build { legacy: bool },
}

fn backup_mode(force: bool) -> BackupMode {
    if force {
        BackupMode::Force
    } else {
        BackupMode::Verified
    }
}

fn plan(action: &Action) -> Vec<Step> {
    match action {
        Action::TopLevelBackup {
            clean,
            force,
            prune,
        } => {
            let mode = backup_mode(*force);
            let mut steps = vec![Step::Backup { mode }];
            if *prune {
                steps.push(Step::PruneBin);
            }
            if *clean {
                steps.push(Step::Clean { mode });
            }
            steps
        }

        Action::TopLevelClean { force } => {
            vec![Step::Clean {
                mode: backup_mode(*force),
            }]
        }

        Action::Fetch { prune } => {
            let mut steps = vec![Step::Fetch {
                mode: FetchMode::Remote,
            }];
            if *prune {
                steps.push(Step::PruneCsv);
            }
            steps
        }

        Action::Run {
            backup,
            clean,
            force,
            prune,
            legacy,
        } => {
            let mode = backup_mode(*force);
            let mut steps = vec![];
            if *backup {
                steps.push(Step::Backup { mode });
            }
            if *clean {
                steps.push(Step::Clean { mode });
            }
            steps.push(Step::Fetch {
                mode: FetchMode::Remote,
            });
            if *prune {
                steps.push(Step::PruneCsv);
            }
            steps.push(Step::Build { legacy: *legacy });
            steps
        }

        Action::Build {
            backup,
            clean,
            force,
            prune,
            legacy,
        } => {
            let mode = backup_mode(*force);
            let mut steps = vec![];
            if *backup {
                steps.push(Step::Backup { mode });
            }
            if *prune {
                steps.push(Step::PruneBin);
            }
            if *clean {
                steps.push(Step::Clean { mode });
            }
            steps.push(Step::Fetch {
                mode: FetchMode::Local,
            });
            steps.push(Step::Build { legacy: *legacy });
            steps
        }

        Action::Conf(_) => vec![],
    }
}

#[derive(Default)]
struct RunContext {
    fetch_result: Option<(TempDir, Version)>,
}

fn execute_step(
    cfg: &Config,
    paths: &ResolvedPaths<'_>,
    step: Step,
    ctx: &mut RunContext,
) -> Result<()> {
    match step {
        Step::Backup { mode } => {
            messages::info("Backing up database...");
            backup(paths.output, paths.archive, mode)?;
        }

        Step::Clean { mode } => {
            messages::info("Cleaning output directory...");
            delete(paths.output, mode)?;
        }

        Step::Fetch { mode } => {
            ctx.fetch_result = Some(fetch(cfg, mode)?);
        }

        Step::PruneCsv => {
            messages::info("Pruning CSV archives...");
            prune_archives(cfg, PruneMode::Csv)?;
        }

        Step::PruneBin => {
            messages::info("Pruning bin archives...");
            prune_archives(cfg, PruneMode::Bin)?;
        }

        Step::Build { legacy } => {
            let (temp_dir, version) = ctx
                .fetch_result
                .as_ref()
                .expect("Build step requires prior Fetch");
            messages::info("Building binary database...");
            build(temp_dir.path(), paths.output, version, legacy)?;
        }
    }

    Ok(())
}

pub fn run_action(cfg: &Config, action: Action) -> Result<()> {
    let paths = resolve_paths(cfg);
    let steps = plan(&action);
    let mut ctx = RunContext::default();

    for step in steps {
        execute_step(cfg, &paths, step, &mut ctx)?;
    }

    Ok(())
}
