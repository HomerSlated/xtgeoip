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

/// A step that needs no value from any other step.
///
/// `Build` is deliberately *not* here: it consumes the result of a `Fetch`,
/// and expressing that as a peer step is what forced the old runtime
/// `.expect("Build step requires prior Fetch")`. See [`Plan`].
#[derive(Clone, Copy, Debug)]
enum Step {
    Backup { mode: BackupMode },
    Clean { mode: BackupMode },
    Fetch { mode: FetchMode },
    PruneCsv,
    PruneBin,
}

/// The shape of an execution.
///
/// `Pipeline` encodes Fetch-before-Build *structurally*: a build cannot be
/// described without naming the fetch that feeds it, so the invariant holds by
/// construction rather than by a runtime assertion. `mid` exists because the
/// two are not adjacent — `run --prune` prunes CSVs between fetching and
/// building — so fusing them into one step would silently reorder that prune.
#[derive(Debug)]
enum Plan {
    /// Steps only; nothing consumes a fetch result. Note this still covers
    /// plans that *contain* a `Fetch` (`xtgeoip fetch`), whose result is
    /// simply discarded.
    Simple(Vec<Step>),
    Pipeline {
        pre: Vec<Step>,
        fetch: FetchMode,
        mid: Vec<Step>,
        legacy: bool,
    },
}

fn backup_mode(force: bool) -> BackupMode {
    if force {
        BackupMode::Force
    } else {
        BackupMode::Verified
    }
}

fn plan(action: &Action) -> Plan {
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
            Plan::Simple(steps)
        }

        Action::TopLevelClean { force } => Plan::Simple(vec![Step::Clean {
            mode: backup_mode(*force),
        }]),

        Action::Fetch { prune } => {
            let mut steps = vec![Step::Fetch {
                mode: FetchMode::Remote,
            }];
            if *prune {
                steps.push(Step::PruneCsv);
            }
            Plan::Simple(steps)
        }

        Action::Run {
            backup,
            clean,
            force,
            prune,
            legacy,
        } => {
            let mode = backup_mode(*force);
            let mut pre = vec![];
            if *backup {
                pre.push(Step::Backup { mode });
            }
            if *clean {
                pre.push(Step::Clean { mode });
            }
            let mut mid = vec![];
            if *prune {
                mid.push(Step::PruneCsv);
            }
            Plan::Pipeline {
                pre,
                fetch: FetchMode::Remote,
                mid,
                legacy: *legacy,
            }
        }

        Action::Build {
            backup,
            clean,
            force,
            prune,
            legacy,
        } => {
            let mode = backup_mode(*force);
            let mut pre = vec![];
            if *backup {
                pre.push(Step::Backup { mode });
            }
            if *prune {
                pre.push(Step::PruneBin);
            }
            if *clean {
                pre.push(Step::Clean { mode });
            }
            Plan::Pipeline {
                pre,
                fetch: FetchMode::Local,
                mid: vec![],
                legacy: *legacy,
            }
        }

        Action::Conf(_) => Plan::Simple(vec![]),
    }
}

fn execute_step(
    cfg: &Config,
    paths: &ResolvedPaths<'_>,
    step: Step,
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

        // Standalone fetch: nothing downstream consumes the result, so the
        // extracted temp dir is dropped here.
        Step::Fetch { mode } => {
            fetch(cfg, mode)?;
        }

        Step::PruneCsv => {
            messages::info("Pruning CSV archives...");
            prune_archives(cfg, PruneMode::Csv)?;
        }

        Step::PruneBin => {
            messages::info("Pruning bin archives...");
            prune_archives(cfg, PruneMode::Bin)?;
        }
    }

    Ok(())
}

fn execute_steps(
    cfg: &Config,
    paths: &ResolvedPaths<'_>,
    steps: Vec<Step>,
) -> Result<()> {
    for step in steps {
        execute_step(cfg, paths, step)?;
    }
    Ok(())
}

pub fn run_action(cfg: &Config, action: Action) -> Result<()> {
    let paths = resolve_paths(cfg);

    match plan(&action) {
        Plan::Simple(steps) => execute_steps(cfg, &paths, steps)?,

        Plan::Pipeline {
            pre,
            fetch: mode,
            mid,
            legacy,
        } => {
            execute_steps(cfg, &paths, pre)?;
            // Owned, not an Option: the plan could not have described a build
            // without this fetch, so there is nothing to unwrap.
            let (temp_dir, version): (TempDir, Version) = fetch(cfg, mode)?;
            execute_steps(cfg, &paths, mid)?;
            messages::info("Building binary database...");
            build(temp_dir.path(), paths.output, &version, legacy)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden helper: flattens a [`Plan`] back into the linear step sequence it
    /// describes, pinning both *order* and each step's fields in one
    /// assertion. Mirrors how `cli::snapshot` pins `Action`.
    ///
    /// The flattening is deliberate. These goldens predate the `Plan` split
    /// and their expected strings are unchanged by it — so they assert that
    /// encoding Fetch-before-Build in the type system altered no observable
    /// order or argument. That matters because `run_action` itself is only
    /// exercised by `xtgeoip-tests` (root + live MaxMind, rate-capped), making
    /// these the only affordable regression net over the execution path.
    fn steps(action: &Action) -> String {
        let parts: Vec<String> = match plan(action) {
            Plan::Simple(steps) => {
                steps.iter().map(|s| format!("{s:?}")).collect()
            }
            Plan::Pipeline {
                pre,
                fetch,
                mid,
                legacy,
            } => {
                let mut v: Vec<String> =
                    pre.iter().map(|s| format!("{s:?}")).collect();
                v.push(format!("Fetch {{ mode: {fetch:?} }}"));
                v.extend(mid.iter().map(|s| format!("{s:?}")));
                v.push(format!("Build {{ legacy: {legacy} }}"));
                v
            }
        };
        format!("[{}]", parts.join(", "))
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

    /// Fetch-before-Build is now a *type* guarantee: a build is only
    /// expressible as `Plan::Pipeline`, which cannot be constructed without
    /// naming the fetch that feeds it. This sweep is kept as the behavioural
    /// half of that claim — it checks the guarantee survives flattening for
    /// every flag combination, i.e. that no arm emits a build whose fetch
    /// lands after it in execution order.
    ///
    /// It previously guarded `execute_step`'s
    /// `.expect("Build step requires prior Fetch")`, which no longer exists.
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
            let rendered = steps(action);
            let Some(build_at) = rendered.find("Build ") else {
                // No build in this plan; nothing to guarantee.
                assert!(
                    matches!(plan(action), Plan::Simple(_)),
                    "{action:?} has no Build but is not Simple"
                );
                continue;
            };
            let fetch_at = rendered.find("Fetch ").unwrap_or_else(|| {
                panic!("Build with no Fetch at all for {action:?}: {rendered}")
            });
            assert!(
                fetch_at < build_at,
                "Fetch must precede Build for {action:?}: {rendered}"
            );
            // The structural half: a build is only expressible as a Pipeline.
            assert!(
                matches!(plan(action), Plan::Pipeline { .. }),
                "{action:?} builds but is not a Pipeline"
            );
        }
    }
}
