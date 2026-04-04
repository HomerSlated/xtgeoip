//! xtgeoip-tests
//! Run xtgeoip commands from docs/generated/testcases.yaml

use serde::Deserialize;
use std::fs;
use std::process::{Command, exit};
use std::env;

#[derive(Debug, Deserialize)]
struct Testcase {
    key: String,
    cmd: String,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    // Default mode is "--all"
    let filter_failed_only = args.iter().any(|a| a == "--failed");

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
        let program = match parts.next() {
            Some(p) => p,
            None => continue,
        };
        let args: Vec<&str> = parts.collect();

        let status = Command::new("sudo")
            .arg(program)
            .args(args)
            .status()?;

        if status.success() {
            println!("Success");
        } else if let Some(code) = status.code() {
            println!("FAILED (exit {})", code);
        } else {
            println!("FAILED (terminated by signal)");
        }

        println!();
    }

    Ok(())
}
