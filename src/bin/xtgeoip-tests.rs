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

        println!("[{}] [{}]", tc.key, tc.cmd);

        // Split command into program + args
        let mut parts = tc.cmd.split_whitespace();
        let program = parts.next().unwrap();
        let args: Vec<&str> = parts.collect();
        let xtgeoip_path = format!("target/release/{}", program);

        // Run the command and capture output
        let output = Command::new("sudo")
            .arg(&xtgeoip_path)
            .args(&args)
            .output()?;

        // Print stdout/stderr
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);

        println!("{}", stdout_str);
        println!("{}", stderr_str);

        if output.status.success() {
            println!("Success");

            // Check for rebuild condition
            if rebuild_after_clean && args.contains(&"-c") {
                if stdout_str.contains("Deleted old binary data files")
                    || stdout_str.contains("Force deleted binary data files")
                {
                    println!(
                        "--rebuild active: previous clean deleted binaries, running `build`"
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
                } else {
                    println!(
                        "--rebuild active: no binary files deleted, skipping rebuild"
                    );
                }
            }
        } else if let Some(code) = output.status.code() {
            println!("FAILED (exit {})", code);
        } else {
            println!("FAILED (terminated by signal)");
        }

        println!();
    }

    Ok(())
}
