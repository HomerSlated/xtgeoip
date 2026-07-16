/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip action runner
use std::path::Path;

use anyhow::Result;
use tempfile::TempDir;

use crate::{
    backup::{BackupMode, PruneMode, backup, delete, prune_archives},
    build::build,
    conf::ConfAction,
    config::Config,
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

#[derive(Clone, Copy, Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden helper: the plan's `Debug` form pins both the step *sequence* and
    /// each step's fields in one assertion. Mirrors how `cli::snapshot` pins
    /// `Action`. `plan()` is otherwise only exercised end-to-end by
    /// `xtgeoip-tests` (root + live MaxMind), so these are its unit-level pin.
    fn steps(action: &Action) -> String {
        format!("{:?}", plan(action))
    }

    // ── top-level backup ─────────────────────────────────────────────────────

    #[test]
    fn top_level_backup_plain() {
        assert_eq!(
            steps(&Action::TopLevelBackup {
                clean: false,
                force: false,
                prune: false,
            }),
            "[Backup { mode: Verified }]"
        );
    }

    #[test]
    fn top_level_backup_force_selects_force_mode() {
        assert_eq!(
            steps(&Action::TopLevelBackup {
                clean: false,
                force: true,
                prune: false,
            }),
            "[Backup { mode: Force }]"
        );
    }

    #[test]
    fn top_level_backup_prune_precedes_clean() {
        // Order is load-bearing: back up, prune bins, then clean.
        assert_eq!(
            steps(&Action::TopLevelBackup {
                clean: true,
                force: false,
                prune: true,
            }),
            "[Backup { mode: Verified }, PruneBin, Clean { mode: Verified }]"
        );
    }

    // ── top-level clean ──────────────────────────────────────────────────────

    #[test]
    fn top_level_clean_modes() {
        assert_eq!(
            steps(&Action::TopLevelClean { force: false }),
            "[Clean { mode: Verified }]"
        );
        assert_eq!(
            steps(&Action::TopLevelClean { force: true }),
            "[Clean { mode: Force }]"
        );
    }

    // ── fetch ────────────────────────────────────────────────────────────────

    #[test]
    fn fetch_is_remote_and_prunes_csv() {
        assert_eq!(
            steps(&Action::Fetch { prune: false }),
            "[Fetch { mode: Remote }]"
        );
        assert_eq!(
            steps(&Action::Fetch { prune: true }),
            "[Fetch { mode: Remote }, PruneCsv]"
        );
    }

    // ── run ──────────────────────────────────────────────────────────────────

    #[test]
    fn run_plain_fetches_remote_then_builds() {
        assert_eq!(
            steps(&Action::Run {
                prune: false,
                legacy: false,
                backup: false,
                clean: false,
                force: false,
            }),
            "[Fetch { mode: Remote }, Build { legacy: false }]"
        );
    }

    #[test]
    fn run_full_sequence() {
        // run fetches Remote and prunes CSVs (contrast build_full_sequence).
        assert_eq!(
            steps(&Action::Run {
                prune: true,
                legacy: true,
                backup: true,
                clean: true,
                force: true,
            }),
            "[Backup { mode: Force }, Clean { mode: Force }, Fetch { mode: \
             Remote }, PruneCsv, Build { legacy: true }]"
        );
    }

    // ── build ────────────────────────────────────────────────────────────────

    #[test]
    fn build_plain_fetches_local_then_builds() {
        // build reuses the cached CSV: Local, never Remote.
        assert_eq!(
            steps(&Action::Build {
                legacy: false,
                backup: false,
                clean: false,
                force: false,
                prune: false,
            }),
            "[Fetch { mode: Local }, Build { legacy: false }]"
        );
    }

    #[test]
    fn build_full_sequence() {
        // build fetches Local and prunes BINs — the mirror of
        // run_full_sequence.
        assert_eq!(
            steps(&Action::Build {
                legacy: true,
                backup: true,
                clean: true,
                force: true,
                prune: true,
            }),
            "[Backup { mode: Force }, PruneBin, Clean { mode: Force }, Fetch \
             { mode: Local }, Build { legacy: true }]"
        );
    }

    // ── conf ─────────────────────────────────────────────────────────────────

    #[test]
    fn conf_plans_no_steps() {
        assert_eq!(steps(&Action::Conf(ConfAction::Show)), "[]");
    }

    // ── invariant ────────────────────────────────────────────────────────────

    /// `execute_step` ends `Step::Build` with
    /// `.expect("Build step requires prior Fetch")`. That expect is unreachable
    /// only because every `plan()` arm emitting Build emits a Fetch first — an
    /// invariant held by construction, not by the type system. Pin it across
    /// every flag combination so a future edit cannot silently turn it into a
    /// runtime panic.
    #[test]
    fn build_is_always_preceded_by_fetch() {
        let mut actions = vec![
            Action::Fetch { prune: false },
            Action::Fetch { prune: true },
            Action::TopLevelClean { force: false },
            Action::Conf(ConfAction::Show),
        ];
        for &b in &[false, true] {
            for &c in &[false, true] {
                for &f in &[false, true] {
                    for &p in &[false, true] {
                        actions.push(Action::TopLevelBackup {
                            clean: c,
                            force: f,
                            prune: p,
                        });
                        for &l in &[false, true] {
                            actions.push(Action::Run {
                                prune: p,
                                legacy: l,
                                backup: b,
                                clean: c,
                                force: f,
                            });
                            actions.push(Action::Build {
                                legacy: l,
                                backup: b,
                                clean: c,
                                force: f,
                                prune: p,
                            });
                        }
                    }
                }
            }
        }

        for action in &actions {
            let plan = plan(action);
            let Some(build_at) =
                plan.iter().position(|s| matches!(s, Step::Build { .. }))
            else {
                continue;
            };
            assert!(
                plan[..build_at]
                    .iter()
                    .any(|s| matches!(s, Step::Fetch { .. })),
                "Build with no preceding Fetch for {action:?}: {plan:?}"
            );
        }
    }
}
