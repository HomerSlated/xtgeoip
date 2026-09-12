/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip action runner
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use tempfile::{NamedTempFile, TempDir};

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

/// Whether this process's effective uid is 0.
///
/// Advisory only. It chooses the wording of an error and gates nothing: the
/// question every command actually has to answer is whether it can write to
/// the directories its plan touches, and [`check_plan_writable`] asks the
/// kernel that directly. A blanket root test answered a proxy for it, and
/// refused work on any configuration where the two disagree.
///
/// Reads the *effective* uid (the second `Uid:` field), not the real one:
/// permission checks are made against the effective uid, so that is the one
/// whose answer this advice is standing in for.
pub fn is_root() -> bool {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(2))
                .and_then(|uid| uid.parse::<u32>().ok())
        })
        .is_some_and(|uid| uid == 0)
}

/// A configured directory that some step writes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Archive,
    Output,
}

impl Dir {
    fn key(self) -> &'static str {
        match self {
            Dir::Archive => "paths.archive_dir",
            Dir::Output => "paths.output_dir",
        }
    }

    fn path<'a>(self, paths: &ResolvedPaths<'a>) -> &'a Path {
        match self {
            Dir::Archive => paths.archive,
            Dir::Output => paths.output,
        }
    }
}

/// The directory `step` creates, replaces or deletes files in, if any.
///
/// Writes only — `Backup` *reads* `output_dir`, which is not a requirement
/// this check can usefully enforce. A local fetch writes nothing: it calls
/// `create_dir_all` on `archive_dir`, which needs no write access to a
/// directory that already exists, and has no archive to extract from one that
/// does not.
///
/// Deliberately no wildcard arm, so a new `Step` cannot be added without
/// deciding what it writes.
fn step_writes(step: Step) -> Option<Dir> {
    match step {
        Step::Backup { .. }
        | Step::Fetch {
            mode: FetchMode::Remote,
        }
        | Step::PruneCsv
        | Step::PruneBin => Some(Dir::Archive),
        Step::Fetch {
            mode: FetchMode::Local,
        } => None,
        Step::Clean { .. } => Some(Dir::Output),
    }
}

/// Every directory `plan` writes into, archive before output.
fn dirs_written(plan: &Plan) -> Vec<Dir> {
    let written: Vec<Dir> = match plan {
        Plan::Simple(steps) => {
            steps.iter().filter_map(|&s| step_writes(s)).collect()
        }
        Plan::Pipeline {
            pre, fetch, mid, ..
        } => pre
            .iter()
            .copied()
            .chain([Step::Fetch { mode: *fetch }])
            .chain(mid.iter().copied())
            .filter_map(step_writes)
            // The build itself, which is not a `Step`.
            .chain([Dir::Output])
            .collect(),
    };
    [Dir::Archive, Dir::Output]
        .into_iter()
        .filter(|d| written.contains(d))
        .collect()
}

/// Refuse the whole plan, before its first step, if any directory it writes
/// into is not writable.
fn check_plan_writable(plan: &Plan, paths: &ResolvedPaths<'_>) -> Result<()> {
    for dir in dirs_written(plan) {
        check_writable(dir.path(paths)).with_context(|| {
            let advice = if is_root() {
                ""
            } else {
                " (re-run as root, e.g. with sudo, or point it at a directory \
                 you can write)"
            };
            format!(
                "{} is not writable, so nothing was done{advice}",
                dir.key()
            )
        })?;
    }
    Ok(())
}

/// Fail unless a file can be created in `dir` — or, if `dir` does not exist
/// yet, in its nearest existing ancestor, since the steps `create_dir_all` it.
///
/// Probes by doing, as `conf.rs::check_system_config_writable` does, so the
/// answer is the kernel's verdict on the operation the steps will attempt
/// rather than a model of it. The probe file is unlinked when it drops, and
/// its `.tmp` name is not one `build`'s orphan detection claims.
fn check_writable(dir: &Path) -> Result<()> {
    let probed = nearest_existing(dir);
    NamedTempFile::new_in(&probed).map(drop).with_context(|| {
        if probed == dir {
            format!("Cannot write to {}", dir.display())
        } else {
            format!(
                "Cannot create {}: {} is not writable",
                dir.display(),
                probed.display()
            )
        }
    })
}

/// `dir` itself if anything is there, else its nearest ancestor that is.
///
/// Walks up on `NotFound` alone. Any other failure to stat stops the walk and
/// the probe fails on that path. Usually the ancestor it would otherwise reach
/// fails the probe as well — a file component, an unsearchable parent — but
/// not always: a name over `NAME_MAX` has a perfectly writable parent, and a
/// walk that went past it would pass a path `create_dir_all` cannot make.
///
/// `symlink_metadata`, not `metadata`, for the same reason: a dangling
/// symlink does not follow to anything, so a following stat would walk past
/// it to its writable parent, while `create_dir_all` fails on it.
fn nearest_existing(dir: &Path) -> PathBuf {
    let mut candidate = dir;
    loop {
        match fs::symlink_metadata(candidate) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                match candidate.parent() {
                    Some(p) if p.as_os_str().is_empty() => {
                        return PathBuf::from(".");
                    }
                    Some(p) => candidate = p,
                    None => return candidate.to_path_buf(),
                }
            }
            _ => return candidate.to_path_buf(),
        }
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
fn fetch_step(
    cfg: &Config,
    mode: FetchMode,
    ca_file: Option<&Path>,
) -> Result<(TempDir, Version)> {
    match mode {
        FetchMode::Remote => {
            let creds = cfg.maxmind.credentials.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "MaxMind credentials not configured. Run `xtgeoip conf \
                     --set-credentials` first."
                )
            })?;
            let decrypted = secrets::decrypt(creds)?;
            fetch(
                cfg,
                mode,
                decrypted.account_id(),
                decrypted.license_key(),
                ca_file,
            )
        }
        FetchMode::Local => fetch(cfg, mode, "", "", ca_file),
    }
}

fn execute_step(
    cfg: &Config,
    paths: &ResolvedPaths<'_>,
    step: Step,
    ca_file: Option<&Path>,
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
            fetch_step(cfg, mode, ca_file)?;
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
    ca_file: Option<&Path>,
) -> Result<()> {
    for step in steps {
        execute_step(cfg, paths, step, ca_file)?;
    }
    Ok(())
}

pub fn run_action(
    cfg: &Config,
    action: Action,
    ca_file: Option<&Path>,
) -> Result<()> {
    let paths = resolve_paths(cfg);
    let plan = plan(&action);

    // Before any step, because the steps are not all cheap to repeat: a
    // remote fetch prompts for the passphrase and spends a download against
    // MaxMind's daily cap before `build` would discover it cannot write.
    check_plan_writable(&plan, &paths)?;

    match plan {
        Plan::Simple(steps) => execute_steps(cfg, &paths, steps, ca_file)?,

        Plan::Pipeline {
            pre,
            fetch: mode,
            mid,
            legacy,
        } => {
            execute_steps(cfg, &paths, pre, ca_file)?;
            // Owned, not an Option: the plan could not have described a build
            // without this fetch, so there is nothing to unwrap.
            let (temp_dir, version): (TempDir, Version) =
                fetch_step(cfg, mode, ca_file)?;
            execute_steps(cfg, &paths, mid, ca_file)?;
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
    /// clean-before-fetch for six weeks after `850bfd8` (#24 stage 1) reversed
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

    // ── man page ↔ planner agreement ─────────────────────────────────────

    /// EXECUTION ORDER in the man page must agree with the real planner.
    ///
    /// The section lists four representative invocations and, for each, the
    /// exact sequence of steps it produces. That is the same claim
    /// `spec_steps_agree_with_plan` checks for `cli.yaml`'s `steps:` — but
    /// written a second time, by hand, in prose, where nothing looked at it.
    /// It went stale exactly as you would expect: #24 stage 1 (`850bfd8`)
    /// moved `Clean` after `Fetch` and this section still described the old
    /// order six weeks later, found by reading rather than by tooling
    /// (2026-09-02). Three defects were found in this template that day; the
    /// only thing preventing a fourth was that someone happened to look.
    ///
    /// The failure message names `docs/spec/manpage-template.toml`, not the
    /// generated `.1`. Editing the generated file would appear to fix this
    /// and be silently reverted by the next docgen run.
    #[test]
    fn manpage_execution_order_agrees_with_the_planner() {
        use clap::Parser;

        use crate::cli::{Cli, CliOutcome, normalize_cli_to_action};

        /// Prose phrase → the step name `cli.yaml` and `step_names` use.
        ///
        /// Two phrases map to `fetch`: the man page distinguishes a remote
        /// download from reading a cached archive, which is a real and
        /// useful distinction for a reader, while the planner calls both
        /// `Step::Fetch` and separates them by `FetchMode`. Keeping the map
        /// explicit is what lets the prose stay readable without the check
        /// losing its grip.
        const PHRASES: &[(&str, &str)] = &[
            ("backup", "backup"),
            ("prune binary archives", "prune_bin"),
            ("fetch", "fetch"),
            ("read local archive", "fetch"),
            ("clean", "clean"),
            ("prune CSV archives", "prune_csv"),
            ("build", "build"),
        ];

        let man = std::fs::read_to_string("docs/generated/xtgeoip.1")
            .expect("docs/generated/xtgeoip.1 missing — run docgen");
        let section = man
            .split(".SH EXECUTION ORDER")
            .nth(1)
            .and_then(|s| s.split("\n.SH ").next())
            .expect("no EXECUTION ORDER section in the man page");

        // `.TP` / `.B "xtgeoip …"` / prose — the roff shape of a tagged
        // paragraph. Anything else in the section is narrative and ignored.
        let mut checked = 0;
        let lines: Vec<&str> = section.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // The `.TP` is load-bearing, not decoration. The same
            // `.B "xtgeoip \-b \-c"` appears again in the narrative at the
            // foot of the section ("this means … are identical"), where the
            // following line is prose, not a step list. Only a tagged
            // paragraph carries an ordering claim.
            if i == 0 || lines[i - 1].trim() != ".TP" {
                continue;
            }
            let Some(cmd) = line.strip_prefix(".B \"xtgeoip ") else {
                continue;
            };
            let Some(cmd) = cmd.strip_suffix('"') else {
                continue;
            };
            let argv_str = format!("xtgeoip {}", cmd.replace("\\-", "-"));
            let Some(prose) = lines.get(i + 1) else {
                continue;
            };

            let mut expected = Vec::new();
            for phrase in prose.split("\\(->") {
                let phrase = phrase.trim();
                let name = PHRASES
                    .iter()
                    .find(|(p, _)| *p == phrase)
                    .map(|(_, n)| *n)
                    .unwrap_or_else(|| {
                        panic!(
                            "EXECUTION ORDER for {argv_str:?} names the step \
                             {phrase:?}, which maps to nothing the planner \
                             produces. Either the prose is wrong or PHRASES \
                             needs the new name — do not delete the entry to \
                             make this pass. \
                             (docs/spec/manpage-template.toml, \
                             execution_order)"
                        )
                    });
                expected.push(name);
            }

            let argv: Vec<&str> = argv_str.split_whitespace().collect();
            let action = Cli::try_parse_from(&argv)
                .ok()
                .and_then(|cli| match normalize_cli_to_action(&cli) {
                    Ok(CliOutcome::Action(a)) => Some(a),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    panic!(
                        "EXECUTION ORDER documents {argv_str:?}, which does \
                         not parse into an Action at all \
                         (docs/spec/manpage-template.toml, execution_order)"
                    )
                });

            let actual = step_names(&action);
            assert_eq!(
                actual, expected,
                "EXECUTION ORDER says {argv_str:?} runs {expected:?}, but the \
                 planner runs {actual:?}. Fix the prose in \
                 docs/spec/manpage-template.toml (execution_order) and re-run \
                 docgen — editing docs/generated/xtgeoip.1 is reverted by the \
                 next generation."
            );
            checked += 1;
        }

        // Without this, deleting every example — or breaking the roff shape
        // the scan depends on — would leave a test that passes by examining
        // nothing. That failure mode is the reason this whole area exists.
        assert_eq!(
            checked, 4,
            "expected 4 documented orderings in EXECUTION ORDER, parsed \
             {checked}. If an ordering was added or removed deliberately, \
             update this count; if not, the roff shape the scan relies on has \
             changed."
        );
    }

    // ── writability pre-flight ───────────────────────────────────────────

    fn dirs(action: &Action) -> Vec<Dir> {
        dirs_written(&plan(action))
    }

    #[test]
    fn fetch_writes_only_the_archive() {
        for prune in [false, true] {
            assert_eq!(dirs(&Action::Fetch { prune }), [Dir::Archive]);
        }
    }

    #[test]
    fn plain_build_writes_only_the_output() {
        // The local fetch feeding it reads `archive_dir` and writes nothing,
        // so an operator who can read root's archives can build into a
        // directory of their own.
        let build = Action::Build {
            legacy: false,
            backup: false,
            clean: false,
            force: false,
            prune: false,
        };
        assert_eq!(dirs(&build), [Dir::Output]);
    }

    #[test]
    fn top_level_clean_writes_only_the_output() {
        for force in [false, true] {
            assert_eq!(dirs(&Action::TopLevelClean { force }), [Dir::Output]);
        }
    }

    #[test]
    fn backup_writes_the_archive_and_reads_the_output() {
        let backup = Action::TopLevelBackup {
            clean: false,
            force: false,
            prune: false,
        };
        assert_eq!(dirs(&backup), [Dir::Archive]);
        let backup_and_clean = Action::TopLevelBackup {
            clean: true,
            force: false,
            prune: false,
        };
        assert_eq!(dirs(&backup_and_clean), [Dir::Archive, Dir::Output]);
    }

    /// Over every `Action`: the directories required are exactly those the
    /// flattened plan's steps write, plus `output_dir` for any build. Derived
    /// from `steps()`'s rendering rather than from `step_writes`, so the two
    /// halves of this comparison do not share the code under test.
    #[test]
    fn required_dirs_match_every_plans_steps() {
        for action in all_actions() {
            let rendered = steps(&action);
            let archive =
                ["Backup", "Fetch { mode: Remote }", "PruneCsv", "PruneBin"]
                    .iter()
                    .any(|s| rendered.contains(s));
            let output =
                ["Clean", "Build"].iter().any(|s| rendered.contains(s));
            let expected: Vec<Dir> =
                [(archive, Dir::Archive), (output, Dir::Output)]
                    .into_iter()
                    .filter_map(|(needed, d)| needed.then_some(d))
                    .collect();
            assert_eq!(dirs(&action), expected, "{action:?} plans {rendered}");
        }
    }

    /// Root bypasses the mode bits these tests rely on, so under root they
    /// would fail for the wrong reason. The unprivileged case is the one that
    /// matters and the one CI runs.
    fn unprivileged() -> bool {
        if is_root() {
            eprintln!("skipped: running as root, which ignores 0o555");
            return false;
        }
        true
    }

    fn read_only_dir() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555))
            .unwrap();
        dir
    }

    fn entries(dir: &Path) -> usize {
        fs::read_dir(dir).unwrap().count()
    }

    #[test]
    fn a_writable_dir_passes_and_is_left_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        check_writable(dir.path()).unwrap();
        assert_eq!(entries(dir.path()), 0, "the probe file was left behind");
    }

    #[test]
    fn a_missing_dir_under_a_writable_parent_passes_and_is_not_created() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("not/yet/created");
        check_writable(&target).unwrap();
        assert_eq!(entries(dir.path()), 0);
    }

    #[test]
    fn a_read_only_dir_is_refused_by_name() {
        if !unprivileged() {
            return;
        }
        let dir = read_only_dir();
        let err = check_writable(dir.path()).unwrap_err();
        assert!(
            err.to_string()
                .contains(&format!("Cannot write to {}", dir.path().display())),
            "{err:#}"
        );
    }

    #[test]
    fn a_missing_dir_under_a_read_only_parent_is_refused_naming_both() {
        if !unprivileged() {
            return;
        }
        let dir = read_only_dir();
        let target = dir.path().join("sub/dir");
        let err = check_writable(&target).unwrap_err().to_string();
        assert!(
            err.contains(&target.display().to_string())
                && err.contains(&format!(
                    "{} is not writable",
                    dir.path().display()
                )),
            "{err}"
        );
    }

    /// A pin, not a discriminator: a walk that went up on *every* stat error
    /// would still stop at `file`, which exists, and fail probing it. The
    /// test that separates the two walks is the over-long name below.
    #[test]
    fn a_path_through_a_file_is_refused() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("file");
        fs::write(&file, b"").unwrap();
        assert!(check_writable(&file.join("sub")).is_err());
    }

    /// `ENAMETOOLONG`, not `ENOENT`, and its parent is writable — so this is
    /// the case where walking up on any error would pass a path that
    /// `create_dir_all` cannot make. Confirmed failing against that mutant.
    #[test]
    fn an_over_long_name_is_refused_not_walked_past() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("x".repeat(300));
        assert!(check_writable(&target).is_err());
    }

    #[test]
    fn a_dangling_symlink_is_refused_not_walked_past() {
        let dir = tempfile::TempDir::new().unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path().join("absent"), &link).unwrap();
        assert!(check_writable(&link).is_err());
    }

    #[test]
    fn the_plan_is_refused_before_any_step_with_advice_for_non_root() {
        if !unprivileged() {
            return;
        }
        let archive = tempfile::TempDir::new().unwrap();
        let output = read_only_dir();
        let paths = ResolvedPaths {
            output: output.path(),
            archive: archive.path(),
        };
        let clean = plan(&Action::TopLevelClean { force: false });
        let err = check_plan_writable(&clean, &paths).unwrap_err().to_string();
        assert!(
            err.starts_with("paths.output_dir is not writable")
                && err.contains("re-run as root"),
            "{err}"
        );
        // Fetch writes only the archive, which is writable here.
        let fetch = plan(&Action::Fetch { prune: false });
        check_plan_writable(&fetch, &paths).unwrap();
    }
}
