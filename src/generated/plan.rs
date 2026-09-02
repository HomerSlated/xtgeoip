// auto-generated — see docs/design/26-spec-derived-planning.md
//
// Ordering comes from `plan.steps[*].rank` in docs/spec/cli.yaml; membership
// from `plan.contexts`. Each step's `why` is carried through from the spec, so
// the reasoning survives the migration instead of becoming a bare integer.
//
// `dead_code` because stages 1-3 run this *alongside* the hand-written
// `action::plan()`: nothing calls it but the differential test. Stage 4
// (deleting `plan()`) is the sign-off point and removes the need for it.
#![allow(dead_code)]

use crate::{
    action::{Action, Plan, Step, backup_mode},
    fetch::FetchMode,
};

pub(crate) fn plan_generated(action: &Action) -> Plan {
    match action {
        Action::TopLevelBackup { clean, force, prune } => {
            let mode = backup_mode(*force);
            let mut steps = Vec::new();
            // backup: The one step that must precede everything else: it runs before anything is disturbed, so a failure later leaves a copy behind.
            if true { steps.push(Step::Backup { mode }); }
            // prune_bin: Prunes old binary tarballs, which only makes sense once the new backup exists — otherwise the retention count is off by one.
            if *prune { steps.push(Step::PruneBin); }
            // clean: Runs after the fetch (#24 stage 1, 2026-07-18). Cleaning first meant a network outage left output_dir empty with no replacement to install; fetching first means a failed run leaves the existing install intact.
            if *clean { steps.push(Step::Clean { mode }); }
            Plan::Simple(steps)
        }

        Action::TopLevelClean { force } => {
            let mode = backup_mode(*force);
            let mut steps = Vec::new();
            // backup: The one step that must precede everything else: it runs before anything is disturbed, so a failure later leaves a copy behind.
            if false { steps.push(Step::Backup { mode }); }
            // prune_bin: Prunes old binary tarballs, which only makes sense once the new backup exists — otherwise the retention count is off by one.
            if false { steps.push(Step::PruneBin); }
            // clean: Runs after the fetch (#24 stage 1, 2026-07-18). Cleaning first meant a network outage left output_dir empty with no replacement to install; fetching first means a failed run leaves the existing install intact.
            if true { steps.push(Step::Clean { mode }); }
            Plan::Simple(steps)
        }

        Action::Fetch { prune } => {
            let mut steps = Vec::new();
            // fetch: Acquire before destroying. Everything downstream depends on the archive being in hand.
            steps.push(Step::Fetch { mode: FetchMode::Remote });
            // prune_csv: Prunes old CSV archives once the current one has been fetched, for the same off-by-one reason as prune_bin.
            if *prune { steps.push(Step::PruneCsv); }
            Plan::Simple(steps)
        }

        Action::Run { backup, clean, force, prune, legacy } => {
            let mode = backup_mode(*force);
            let mut pre = Vec::new();
            // backup: The one step that must precede everything else: it runs before anything is disturbed, so a failure later leaves a copy behind.
            if *backup { pre.push(Step::Backup { mode }); }
            let mut mid = Vec::new();
            // clean: Runs after the fetch (#24 stage 1, 2026-07-18). Cleaning first meant a network outage left output_dir empty with no replacement to install; fetching first means a failed run leaves the existing install intact.
            if *clean { mid.push(Step::Clean { mode }); }
            // prune_csv: Prunes old CSV archives once the current one has been fetched, for the same off-by-one reason as prune_bin.
            if *prune { mid.push(Step::PruneCsv); }
            Plan::Pipeline { pre, fetch: FetchMode::Remote, mid, legacy: *legacy }
        }

        Action::Build { backup, clean, force, prune, legacy } => {
            let mode = backup_mode(*force);
            let mut pre = Vec::new();
            // backup: The one step that must precede everything else: it runs before anything is disturbed, so a failure later leaves a copy behind.
            if *backup { pre.push(Step::Backup { mode }); }
            // prune_bin: Prunes old binary tarballs, which only makes sense once the new backup exists — otherwise the retention count is off by one.
            if *prune { pre.push(Step::PruneBin); }
            let mut mid = Vec::new();
            // clean: Runs after the fetch (#24 stage 1, 2026-07-18). Cleaning first meant a network outage left output_dir empty with no replacement to install; fetching first means a failed run leaves the existing install intact.
            if *clean { mid.push(Step::Clean { mode }); }
            Plan::Pipeline { pre, fetch: FetchMode::Local, mid, legacy: *legacy }
        }

        Action::Conf(_) => {
            let steps = Vec::new();
            Plan::Simple(steps)
        }

    }
}
