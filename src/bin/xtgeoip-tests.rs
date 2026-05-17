//! xtgeoip-tests
//! Run xtgeoip commands from docs/generated/testcases.yaml and assert
//! that each exits with the status expected by its spec key:
//!   key: p  → must exit 0
//!   key: f  → must exit non-zero

use std::{
    env, fs,
    process::{self, Command},
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
}

fn run_with_timeout(
    mut child: process::Child,
    timeout: Duration,
) -> anyhow::Result<Option<process::ExitStatus>> {
    let start = Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(Some(status)),
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
            }
            None => thread::sleep(Duration::from_millis(100)),
        }
    }
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

        let child = Command::new("sudo").arg(&bin).args(cmd_args).spawn()?;
        match run_with_timeout(child, timeout)? {
            None => {
                println!("TIMED OUT");
                timed_out += 1;
            }
            Some(status) => {
                let did_succeed = status.success();
                if did_succeed == expected_pass {
                    println!("PASS");
                    passed += 1;
                    if rebuild_after_clean
                        && did_succeed
                        && tc.rebuild.unwrap_or(false)
                    {
                        print!("  [rebuild] xtgeoip build ... ");
                        let rebuild_child = Command::new("sudo")
                            .arg(&bin)
                            .arg("build")
                            .spawn()?;
                        let label = match run_with_timeout(
                            rebuild_child,
                            Duration::from_secs(DEFAULT_TEST_TIMEOUT_SECS),
                        )? {
                            Some(s) if s.success() => "PASS",
                            Some(_) => "FAIL",
                            None => "TIMED OUT",
                        };
                        println!("{label}");
                    }
                } else {
                    let got = if did_succeed { "exit 0" } else { "exit non-0" };
                    let maps = tc
                        .maps_to
                        .as_deref()
                        .map(|m| format!(" (maps_to: {})", m))
                        .unwrap_or_default();
                    println!(
                        "FAIL — expected {}, got {}{}",
                        expect_label, got, maps
                    );
                    failed += 1;
                }
            }
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
