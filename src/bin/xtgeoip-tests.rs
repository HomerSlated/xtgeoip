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
    let testcases: Vec<Testcase> = serde_yaml::from_str(&yaml_str)?;

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
        let expect_label = if expected_pass {
            "exit 0"
        } else {
            "exit non-0"
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
        let did_succeed = result.status.unwrap().success();

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
            let got = if did_succeed { "exit 0" } else { "exit non-0" };
            println!("FAIL — expected {expect_label}, got {got}{maps}");
            failed += 1;
        } else {
            println!("FAIL");
            if !exit_ok {
                let got = if did_succeed { "exit 0" } else { "exit non-0" };
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
