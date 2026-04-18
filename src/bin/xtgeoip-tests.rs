//! xtgeoip-tests
//! Run xtgeoip commands from docs/generated/testcases.yaml and assert
//! that each exits with the status expected by its spec key:
//!   key: p  → must exit 0
//!   key: f  → must exit non-zero

use std::{
    env, fs,
    process::{self, Command},
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Testcase {
    case_id: Option<String>,
    key: String,
    cmd: String,
    maps_to: Option<String>,
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
    let mut skipped = 0usize;

    for tc in &testcases {
        let id = tc.case_id.as_deref().unwrap_or("?");

        if let Some(want) = filter_case {
            if tc.case_id.as_deref() != Some(want) {
                skipped += 1;
                continue;
            }
        }

        if filter_failed_only && tc.key != "f" {
            skipped += 1;
            continue;
        }

        // Skip interactive editor — no way to drive it non-interactively
        if tc.cmd.contains("conf -e") {
            println!("[SKIP] [{id}] {}", tc.cmd);
            skipped += 1;
            continue;
        }

        let expected_pass = tc.key == "p";
        let expect_label = if expected_pass {
            "exit 0"
        } else {
            "exit non-0"
        };

        print!("[{id}] {} (expect {}) ... ", tc.cmd, expect_label);

        let mut parts = tc.cmd.split_whitespace();
        let program = parts.next().unwrap();
        let cmd_args: Vec<&str> = parts.collect();
        let bin = format!("target/release/{}", program);

        let status = Command::new("sudo").arg(&bin).args(&cmd_args).status()?;
        let did_succeed = status.success();

        if did_succeed == expected_pass {
            println!("PASS");
            passed += 1;

            // After a successful top-level clean, optionally rebuild so the
            // database is left in a usable state for subsequent tests.
            if rebuild_after_clean
                && did_succeed
                && cmd_args.iter().any(|a| *a == "-c")
                && cmd_args.first().map(|a| a.starts_with('-')).unwrap_or(true)
            {
                print!("  [rebuild] xtgeoip build ... ");
                let ok = Command::new("sudo")
                    .arg(&bin)
                    .arg("build")
                    .status()?
                    .success();
                println!("{}", if ok { "PASS" } else { "FAIL" });
            }
        } else {
            let got = if did_succeed { "exit 0" } else { "exit non-0" };
            let maps = tc
                .maps_to
                .as_deref()
                .map(|m| format!(" (maps_to: {})", m))
                .unwrap_or_default();
            println!("FAIL — expected {}, got {}{}", expect_label, got, maps);
            failed += 1;
        }
    }

    println!();
    println!(
        "Results: {} passed, {} failed, {} skipped",
        passed, failed, skipped
    );

    if failed > 0 {
        process::exit(1);
    }

    Ok(())
}
