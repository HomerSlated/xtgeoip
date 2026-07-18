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
    let testcases: Vec<Testcase> = serde_saphyr::from_str(&yaml_str)?;

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
        serde_saphyr::from_str(&yaml).expect("testcases.yaml failed to parse")
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
