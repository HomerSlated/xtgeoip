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
    messages, secrets,
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
pub(crate) enum Step {
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
pub(crate) enum Plan {
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

pub(crate) fn backup_mode(force: bool) -> BackupMode {
    if force {
        BackupMode::Force
    } else {
        BackupMode::Verified
    }
}

/// The execution planner, generated from `plan:` in `docs/spec/cli.yaml`.
///
/// Hand-written until 2026-09-02; see
/// `docs/design/26-spec-derived-planning.md`. The rationale for each
/// step's position now lives in the spec's `why:` fields and is carried
/// into `src/generated/plan.rs` as comments, so editing the order means
/// editing the declaration rather than the code.
use crate::generated::plan::plan_generated as plan;

/// Decrypt MaxMind credentials (prompting interactively) and fetch. Only
/// `FetchMode::Remote` needs credentials at all — `Local` never reads
/// `account_id`/`license_key` inside `fetch()`, so this skips the prompt
/// entirely rather than asking for a passphrase a local-only run has no use
/// for.
fn fetch_step(cfg: &Config, mode: FetchMode) -> Result<(TempDir, Version)> {
    match mode {
        FetchMode::Remote => {
            let creds = cfg.maxmind.credentials.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "MaxMind credentials not configured. Run `xtgeoip conf \
                     --set-credentials` first."
                )
            })?;
            let decrypted = secrets::decrypt(creds)?;
            fetch(cfg, mode, decrypted.account_id(), decrypted.license_key())
        }
        FetchMode::Local => fetch(cfg, mode, "", ""),
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
            fetch_step(cfg, mode)?;
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
            let (temp_dir, version): (TempDir, Version) =
                fetch_step(cfg, mode)?;
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
        //
        // Clean sits AFTER Fetch (#24 stage 1). Changed deliberately
        // 2026-07-18: cleaning first meant a network failure emptied
        // output_dir with no replacement to install. Backup stays in `pre` —
        // it is the one step that must happen before anything is disturbed.
        assert_eq!(
            steps(&Action::Run {
                prune: true,
                legacy: true,
                backup: true,
                clean: true,
                force: true,
            }),
            "[Backup { mode: Force }, Fetch { mode: Remote }, Clean { mode: \
             Force }, PruneCsv, Build { legacy: true }]"
        );
    }

    // ── spec-derived planning (#26/#27), stage 3 ────────────────────────

    /// Every `Action` the program can hold, including combinations the CLI
    /// guards reject — `plan()` is total over the type, so the comparison
    /// should be too.
    fn all_actions() -> Vec<Action> {
        let mut all = Vec::new();
        for i in 0..8u8 {
            all.push(Action::TopLevelBackup {
                clean: i & 1 != 0,
                force: i & 2 != 0,
                prune: i & 4 != 0,
            });
        }
        for i in 0..2u8 {
            all.push(Action::TopLevelClean { force: i & 1 != 0 });
            all.push(Action::Fetch { prune: i & 1 != 0 });
        }
        for i in 0..32u8 {
            let (p, l, b, c, f) =
                (i & 1 != 0, i & 2 != 0, i & 4 != 0, i & 8 != 0, i & 16 != 0);
            all.push(Action::Run {
                prune: p,
                legacy: l,
                backup: b,
                clean: c,
                force: f,
            });
            all.push(Action::Build {
                prune: p,
                legacy: l,
                backup: b,
                clean: c,
                force: f,
            });
        }
        all
    }

    /// Every plan is a subsequence of one fixed order.
    ///
    /// This is the assumption `plan:` in `cli.yaml` is built on: ordering is a
    /// **rank per step**, not a dependency graph, which is only sound while a
    /// single total order covers every context. The differential test that
    /// proved the generated planner reproduced the hand-written one was
    /// migration scaffolding and went with it; this is the property that has
    /// to keep holding afterwards.
    ///
    /// `docs/design/26-spec-derived-planning.md` §1 records that this is an
    /// observation about today's six steps, not an invariant — so it is worth
    /// a test rather than a comment. A step that ran at different points in
    /// different contexts would break the model, and this is what would say so.
    #[test]
    fn every_plan_is_a_subsequence_of_one_canonical_order() {
        const CANON: &[&str] = &[
            "backup",
            "prune_bin",
            "fetch",
            "clean",
            "prune_csv",
            "build",
        ];

        let actions = all_actions();
        for action in &actions {
            let got = step_names(action);
            let mut canon = CANON.iter();
            for step in &got {
                assert!(
                    canon.any(|c| c == step),
                    "{action:?} plans {got:?}, which is not a subsequence of \
                     {CANON:?} — the rank model in cli.yaml no longer holds"
                );
            }
        }
        assert_eq!(actions.len(), 76, "the Action space changed shape");
    }

    // ── spec ↔ plan agreement (#92) ──────────────────────────────────────

    /// Every step in a plan, in execution order, as the names `cli.yaml` uses.
    fn step_names(action: &Action) -> Vec<&'static str> {
        fn name(s: &Step) -> &'static str {
            match s {
                Step::Backup { .. } => "backup",
                Step::Clean { .. } => "clean",
                Step::Fetch { .. } => "fetch",
                Step::PruneCsv => "prune_csv",
                Step::PruneBin => "prune_bin",
            }
        }
        match plan(action) {
            Plan::Simple(steps) => steps.iter().map(name).collect(),
            Plan::Pipeline { pre, mid, .. } => {
                let mut v: Vec<&'static str> = pre.iter().map(name).collect();
                v.push("fetch");
                v.extend(mid.iter().map(name));
                v.push("build");
                v
            }
        }
    }

    /// The spec's `steps:` must match what `plan()` actually does.
    ///
    /// This is the check whose absence let three `outcome:` strings claim
    /// clean-before-fetch for six weeks after `0712783` (#24 stage 1) reversed
    /// that order — R-004, R-005 and R-010 shipped into the man page saying so,
    /// and were found by reading, not by tooling. `outcome:` stays authored
    /// prose; `steps:` is the machine-checkable half, and this compares it
    /// against the real parser and the real planner.
    ///
    /// Covers every documented invocation rather than the eleven Actions the
    /// goldens above pin by hand. Step *parameters* (backup mode, fetch mode,
    /// legacy) are the goldens' job; this one owns membership and order.
    #[test]
    fn spec_steps_agree_with_plan() {
        use clap::Parser;

        use crate::{
            cli::{Cli, CliOutcome, normalize_cli_to_action},
            generated::cli_matrix::CLI_MATRIX,
        };

        let mut problems = Vec::new();

        for ex in CLI_MATRIX {
            let argv: Vec<&str> = ex.cmd.split_whitespace().collect();
            let action = match Cli::try_parse_from(&argv) {
                Ok(cli) => match normalize_cli_to_action(&cli) {
                    Ok(CliOutcome::Action(a)) => Some(a),
                    _ => None,
                },
                Err(_) => None,
            };

            match (action, ex.steps) {
                (Some(action), Some(declared)) => {
                    let actual = step_names(&action);
                    if actual != declared {
                        problems.push(format!(
                            "  {:?}: spec says {declared:?}, plan() gives \
                             {actual:?}",
                            ex.cmd
                        ));
                    }
                }
                // An invocation that reaches `Action` has a plan, so leaving
                // `steps:` off would opt it out of this check silently. That
                // is the failure mode the check exists to prevent, so it is
                // itself a failure.
                (Some(_), None) => problems.push(format!(
                    "  {:?}: reaches Action but declares no `steps:` in \
                     cli.yaml",
                    ex.cmd
                )),
                (None, Some(declared)) => problems.push(format!(
                    "  {:?}: declares steps {declared:?} but never reaches \
                     Action",
                    ex.cmd
                )),
                (None, None) => {}
            }
        }

        assert!(
            problems.is_empty(),
            "{} of {} spec examples disagree with plan():\n{}",
            problems.len(),
            CLI_MATRIX.len(),
            problems.join("\n")
        );
    }

    /// The point of #24 stage 1, stated as an invariant rather than a
    /// sequence: nothing destructive may precede the fetch except the backup.
    #[test]
    fn clean_never_precedes_fetch() {
        for &b in &[false, true] {
            for &f in &[false, true] {
                for &p in &[false, true] {
                    for &l in &[false, true] {
                        for rendered in [
                            steps(&Action::Run {
                                prune: p,
                                legacy: l,
                                backup: b,
                                clean: true,
                                force: f,
                            }),
                            steps(&Action::Build {
                                legacy: l,
                                backup: b,
                                clean: true,
                                force: f,
                                prune: p,
                            }),
                        ] {
                            let clean = rendered.find("Clean ").expect("clean");
                            let fetch = rendered.find("Fetch ").expect("fetch");
                            assert!(
                                fetch < clean,
                                "Clean precedes Fetch — a failed fetch would \
                                 leave output_dir empty: {rendered}"
                            );
                        }
                    }
                }
            }
        }
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
            "[Backup { mode: Force }, PruneBin, Fetch { mode: Local }, Clean \
             { mode: Force }, Build { legacy: true }]"
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
