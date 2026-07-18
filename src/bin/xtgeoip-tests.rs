//! xtgeoip-tests
//! Run xtgeoip commands from docs/generated/testcases.yaml and assert
//! that each exits with the status expected by its spec key:
//!   key: p  → must exit 0
//!   key: f  → must exit non-zero
//!
//! Optional output assertions:
//!   expected_stdout: ""        → stdout must be empty
//!   expected_stdout: "text"    → stdout must contain "text"
//!   expected_stderr: (same)

use std::{
    env, fs,
    io::Read,
    process::{self, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;

const DEFAULT_TEST_TIMEOUT_SECS: u64 = 60;

/// Schema version of `docs/generated/testcases.yaml` this binary understands.
/// Must match `TESTCASES_SCHEMA_VERSION` in `xtgeoip-docgen.rs`. Validated on
/// load rather than merely recorded — an unrecognised version aborts instead
/// of running cases whose meaning may have changed.
const TESTCASES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
struct TestcaseFile {
    schema_version: u32,
    testcases: Vec<Testcase>,
}

/// Load and version-check the generated corpus.
fn load_testcases(yaml: &str) -> anyhow::Result<Vec<Testcase>> {
    let file: TestcaseFile = serde_saphyr::from_str(yaml)?;
    if file.schema_version != TESTCASES_SCHEMA_VERSION {
        anyhow::bail!(
            "testcases.yaml schema version {} is not supported (expected {}) \
             — regenerate with `cargo run --bin xtgeoip-docgen`",
            file.schema_version,
            TESTCASES_SCHEMA_VERSION
        );
    }
    Ok(file.testcases)
}

#[derive(Debug, Deserialize)]
struct Testcase {
    case_id: Option<String>,
    key: String,
    cmd: Vec<String>,
    maps_to: Option<String>,
    exit_status: Option<i32>,
    rebuild: Option<bool>,
    timeout_secs: Option<u64>,
    expected_stdout: Option<String>,
    expected_stderr: Option<String>,
}

struct RunResult {
    status: Option<process::ExitStatus>,
    stdout: String,
    stderr: String,
}

fn run_with_timeout(
    mut child: process::Child,
    timeout: Duration,
) -> anyhow::Result<RunResult> {
    let start = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(status) => break Some(status),
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => thread::sleep(Duration::from_millis(100)),
        }
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(ref mut out) = child.stdout {
        let _ = out.read_to_string(&mut stdout);
    }
    if let Some(ref mut err) = child.stderr {
        let _ = err.read_to_string(&mut stderr);
    }
    Ok(RunResult {
        status,
        stdout,
        stderr,
    })
}

// Returns Some(failure message) if the assertion fails, None if it passes.
// Empty expected string means "must be empty".
fn check_output(label: &str, expected: &str, actual: &str) -> Option<String> {
    if expected.is_empty() {
        if !actual.is_empty() {
            return Some(format!("[{label}] expected empty, got: {actual:?}"));
        }
    } else if !actual.contains(expected) {
        return Some(format!("[{label}] expected {expected:?} in: {actual:?}"));
    }
    None
}

fn main() -> anyhow::Result<()> {
    let argv: Vec<String> = env::args().collect();

    let filter_failed_only = argv.iter().any(|a| a == "--failed");
    let rebuild_after_clean = argv.iter().any(|a| a == "--rebuild");
    let filter_case = argv
        .iter()
        .position(|a| a == "--case")
        .and_then(|i| argv.get(i + 1))
        .map(String::as_str);

    let yaml_str = fs::read_to_string("docs/generated/testcases.yaml")?;
    let testcases = load_testcases(&yaml_str)?;

    if testcases.is_empty() {
        println!("No testcases found.");
        return Ok(());
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut timed_out = 0usize;
    let mut skipped = 0usize;

    for tc in &testcases {
        let id = tc.case_id.as_deref().unwrap_or("?");

        if let Some(want) = filter_case
            && tc.case_id.as_deref() != Some(want)
        {
            skipped += 1;
            continue;
        }

        if filter_failed_only && tc.key != "f" {
            skipped += 1;
            continue;
        }

        // Skip interactive editor — no way to drive it non-interactively
        if tc.cmd.get(1).is_some_and(|s| s == "conf")
            && tc.cmd.iter().any(|s| s == "-e")
        {
            println!("[SKIP] [{id}] {}", tc.cmd.join(" "));
            skipped += 1;
            continue;
        }

        let expected_pass = tc.key == "p";
        let expect_label = if let Some(n) = tc.exit_status {
            format!("exit {n}")
        } else if expected_pass {
            "exit 0".to_string()
        } else {
            "exit non-0".to_string()
        };

        print!("[{id}] {} (expect {}) ... ", tc.cmd.join(" "), expect_label);

        let program = tc.cmd.first().expect("cmd must not be empty");
        let cmd_args = &tc.cmd[1..];
        let bin = format!("target/release/{}", program);
        let timeout = Duration::from_secs(
            tc.timeout_secs.unwrap_or(DEFAULT_TEST_TIMEOUT_SECS),
        );

        let child = Command::new("sudo")
            .arg(&bin)
            .args(cmd_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let result = run_with_timeout(child, timeout)?;

        if result.status.is_none() {
            println!("TIMED OUT");
            timed_out += 1;
            continue;
        }
        let status = result.status.unwrap();
        let did_succeed = status.success();

        let mut output_failures: Vec<String> = Vec::new();
        if let Some(ref expected) = tc.expected_stdout
            && let Some(detail) =
                check_output("stdout", expected, &result.stdout)
        {
            output_failures.push(detail);
        }
        if let Some(ref expected) = tc.expected_stderr
            && let Some(detail) =
                check_output("stderr", expected, &result.stderr)
        {
            output_failures.push(detail);
        }
        if let Some(detail) = tc.maps_to.as_deref().and_then(|key| {
            check_output("maps_to", &format!("[{key}]"), &result.stderr)
        }) {
            output_failures.push(detail);
        }
        if let Some(expected_code) = tc.exit_status
            && status.code() != Some(expected_code)
        {
            output_failures.push(format!(
                "[exit_status] expected exit {expected_code}, got {}",
                status
                    .code()
                    .map_or_else(|| "(signal)".to_string(), |c| c.to_string(),),
            ));
        }

        let got = status
            .code()
            .map_or_else(|| "(signal)".to_string(), |c| format!("exit {c}"));
        let maps = tc
            .maps_to
            .as_deref()
            .map(|m| format!(" (maps_to: {})", m))
            .unwrap_or_default();
        let exit_ok = did_succeed == expected_pass;

        if exit_ok && output_failures.is_empty() {
            println!("PASS");
            passed += 1;
            if rebuild_after_clean && did_succeed && tc.rebuild.unwrap_or(false)
            {
                print!("  [rebuild] xtgeoip build ... ");
                let rebuild_child = Command::new("sudo")
                    .arg(&bin)
                    .arg("build")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()?;
                let rebuild_result = run_with_timeout(
                    rebuild_child,
                    Duration::from_secs(DEFAULT_TEST_TIMEOUT_SECS),
                )?;
                let label = match rebuild_result.status {
                    Some(s) if s.success() => "PASS",
                    Some(_) => "FAIL",
                    None => "TIMED OUT",
                };
                println!("{label}");
            }
        } else if !exit_ok && output_failures.is_empty() {
            println!("FAIL — expected {expect_label}, got {got}{maps}");
            failed += 1;
        } else {
            println!("FAIL");
            if !exit_ok {
                println!("  exit: expected {expect_label}, got {got}{maps}");
            }
            for detail in &output_failures {
                println!("  {detail}");
            }
            failed += 1;
        }
    }

    println!();
    println!(
        "Results: {} passed, {} failed, {} timed out, {} skipped",
        passed, failed, timed_out, skipped
    );

    if failed > 0 || timed_out > 0 {
        process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed corpus this suite runs. Parsing it is otherwise only
    /// exercised by a full root + live-MaxMind run, so a deserialiser change
    /// (e.g. the serde_yaml → serde-saphyr migration, #2) could silently
    /// drop or mangle cases and only surface during a rate-capped run.
    /// `cargo test` sets CWD to the package root, so the path resolves.
    fn load() -> Vec<Testcase> {
        let yaml = fs::read_to_string("docs/generated/testcases.yaml")
            .expect("docs/generated/testcases.yaml missing — run docgen");
        load_testcases(&yaml).expect("testcases.yaml failed to load")
    }

    /// The version gate must actually reject, not just record. A file whose
    /// cases have changed meaning is worse than no file.
    #[test]
    fn wrong_schema_version_is_rejected() {
        let bad = format!(
            "schema_version: {}\ntestcases: []\n",
            TESTCASES_SCHEMA_VERSION + 1
        );
        let err = load_testcases(&bad).expect_err("must reject");
        assert!(
            err.to_string().contains("not supported"),
            "unhelpful error: {err}"
        );
    }

    #[test]
    fn current_schema_version_is_accepted() {
        let ok = format!(
            "schema_version: {TESTCASES_SCHEMA_VERSION}\ntestcases: []\n"
        );
        assert!(load_testcases(&ok).expect("must accept").is_empty());
    }

    #[test]
    fn corpus_parses_with_expected_case_count() {
        assert_eq!(load().len(), 51);
    }

    #[test]
    fn every_case_is_well_formed() {
        for tc in &load() {
            let id = tc.case_id.as_deref().unwrap_or("<no case_id>");
            assert!(
                tc.key == "p" || tc.key == "f",
                "{id}: key must be \"p\" or \"f\", got {:?}",
                tc.key
            );
            assert!(!tc.cmd.is_empty(), "{id}: empty cmd");
            assert_eq!(tc.cmd[0], "xtgeoip", "{id}: cmd must invoke xtgeoip");
            // A failing case must say which exit status it expects; a
            // passing one must not claim a non-zero status.
            match (tc.key.as_str(), tc.exit_status) {
                ("f", None) => panic!("{id}: key 'f' with no exit_status"),
                ("p", Some(s)) if s != 0 => {
                    panic!("{id}: key 'p' with exit_status {s}")
                }
                _ => {}
            }
        }
    }

    /// Pins the emission order (#77/#79).
    ///
    /// The order is deterministic by construction: docgen emits the top-level
    /// command first, then `spec.commands` in `BTreeMap` (alphabetical) order
    /// — build, conf, fetch, run. This test asserts that, so a change to the
    /// spec's map type or iteration cannot silently reshuffle the corpus.
    ///
    /// **Do not "fix" this by sorting on `case_id`.** #77 proposed that; it
    /// was rejected deliberately. Sorting yields B, C, F, R, TL — moving all
    /// 15 top-level cases from first to last — and this suite is
    /// order-dependent (see #87): TL-007 (`-c`) empties `output_dir`, so the
    /// state sequence every later case runs against would change. Validating
    /// that costs a rate-capped live MaxMind run for no benefit, since the
    /// existing order is already deterministic.
    #[test]
    fn emission_order_is_stable() {
        let cases = load();
        let prefixes: Vec<&str> = cases
            .iter()
            .filter_map(|tc| tc.case_id.as_deref())
            .map(|id| id.rsplit_once('-').map_or(id, |(p, _)| p))
            .collect();

        // Run-length encode: (prefix, count) in file order.
        let mut groups: Vec<(&str, usize)> = Vec::new();
        for p in prefixes {
            match groups.last_mut() {
                Some((last, n)) if *last == p => *n += 1,
                _ => groups.push((p, 1)),
            }
        }

        assert_eq!(
            groups,
            vec![("TL", 15), ("B", 13), ("C", 4), ("F", 6), ("R", 13)],
            "emission order changed — top-level first, then commands \
             alphabetically (build, conf, fetch, run). See #77: do not sort \
             by case_id."
        );
    }

    /// Case IDs are how failures are reported and how `--case` selects a
    /// single test; duplicates would make both ambiguous.
    #[test]
    fn case_ids_are_present_and_unique() {
        let cases = load();
        let ids: Vec<&str> = cases
            .iter()
            .filter_map(|tc| tc.case_id.as_deref())
            .collect();
        assert_eq!(ids.len(), 51, "every case needs a case_id");
        let unique: std::collections::BTreeSet<&str> =
            ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "duplicate case_id");
    }
}
