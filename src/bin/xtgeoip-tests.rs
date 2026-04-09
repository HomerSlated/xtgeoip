//! xtgeoip-tests
//! Run xtgeoip commands from docs/generated/testcases.yaml

use std::{env, fs, process::Command};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Testcase {
    key: String,
    cmd: String,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    // Flags
    let filter_failed_only = args.iter().any(|a| a == "--failed");
    let rebuild_after_clean = args.iter().any(|a| a == "--rebuild");

    // Read testcases YAML
    let yaml_str = fs::read_to_string("docs/generated/testcases.yaml")?;
    let testcases: Vec<Testcase> = serde_yaml::from_str(&yaml_str)?;

    if testcases.is_empty() {
        println!("No testcases found!");
        return Ok(());
    }

    // Iterate and run
    for tc in testcases.iter() {
        // Skip if --failed and key != "f"
        if filter_failed_only && tc.key != "f" {
            continue;
        }

        // Skip interactive editor
        if tc.cmd.contains("conf -e") {
            println!("[{}] [{}] -> Skipped interactive testcase", tc.key, tc.cmd);
            continue;
        }

        println!("[{}] [{}]", tc.key, tc.cmd);

        // Split command into program + args
        let mut parts = tc.cmd.split_whitespace();
        let program = parts.next().unwrap();
        let args: Vec<&str> = parts.collect();
        let xtgeoip_path = format!("target/release/{}", program);

        let status = Command::new("sudo")
            .arg(&xtgeoip_path)
            .args(&args)
            .status()?;

        if status.success() {
            println!("Success");

            // Check for rebuild condition
            if rebuild_after_clean && args.contains(&"-c") {
                println!(
                    "--rebuild active: running `build` to repopulate target dir"
                );
                let build_status = Command::new("sudo")
                    .arg(&xtgeoip_path)
                    .arg("build")
                    .status()?;

                if build_status.success() {
                    println!("Rebuild succeeded");
                } else if let Some(code) = build_status.code() {
                    println!("Rebuild FAILED (exit {})", code);
                } else {
                    println!("Rebuild FAILED (terminated by signal)");
                }
            }
        } else if let Some(code) = status.code() {
            println!("FAILED (exit {})", code);
        } else {
            println!("FAILED (terminated by signal)");
        }

        println!();
    }

    Ok(())
}
