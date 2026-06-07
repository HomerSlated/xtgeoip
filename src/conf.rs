/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip `conf` subcommand: configuration management actions (show /
/// edit / default), preconditions, and interactive creation. Depends on
/// `config` only for the shared system-config path.
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::Path,
    process::Command,
};

use anyhow::{Context, Result, bail};

use crate::config::{SYSTEM_CONFIG, system_config_path};

const DEFAULT_CONFIG: &str = "/usr/share/xt_geoip/xtgeoip.conf.example";

#[derive(Debug, PartialEq, Eq)]
pub enum ConfAction {
    Show,
    Edit,
    Default,
}

impl ConfAction {
    /// Check that the invariants this action requires hold before running.
    pub fn check_preconditions(&self) -> Result<()> {
        match self {
            ConfAction::Default => Ok(()),
            ConfAction::Show => ensure_system_config_exists(),
            ConfAction::Edit => {
                ensure_system_config_exists()?;
                if !system_config_path().exists() {
                    bail!(
                        "Cannot edit: {SYSTEM_CONFIG} does not exist. Run \
                         `xtgeoip conf -d` to view the default config, then \
                         create {SYSTEM_CONFIG} manually."
                    );
                }
                Ok(())
            }
        }
    }
}

fn config_exists() -> bool {
    system_config_path().exists()
}

fn create_default_config() -> Result<()> {
    if !Path::new(DEFAULT_CONFIG).exists() {
        bail!(
            "Default config example not found at {DEFAULT_CONFIG}. You may \
             need to reinstall xtgeoip."
        );
    }
    fs::copy(DEFAULT_CONFIG, SYSTEM_CONFIG).with_context(|| {
        format!("Failed to copy {DEFAULT_CONFIG} to {SYSTEM_CONFIG}")
    })?;
    println!("Created {SYSTEM_CONFIG} from default example.");
    Ok(())
}

/// Returns `true` if the user confirmed creation, `false` if they declined.
fn prompt_create_config() -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!(
            "{SYSTEM_CONFIG} does not exist and stdin is not a terminal. Run \
             `xtgeoip conf -d` to view the default config, then create \
             {SYSTEM_CONFIG} manually."
        );
    }

    println!("Configuration file not found at {SYSTEM_CONFIG}.");
    print!("Do you want to create it from the default example? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_lowercase();

    if answer == "y" || answer == "yes" {
        Ok(true)
    } else {
        println!(
            "Skipping creation of system config. You can edit it manually \
             later."
        );
        Ok(false)
    }
}

fn ensure_system_config_exists() -> Result<()> {
    if config_exists() {
        return Ok(());
    }
    if prompt_create_config()? {
        create_default_config()?;
    }
    Ok(())
}

/// Perform the requested action for `xtgeoip conf`
pub fn run_conf(action: ConfAction) -> Result<()> {
    action.check_preconditions()?;
    match action {
        ConfAction::Default => {
            let contents = fs::read_to_string(DEFAULT_CONFIG)?;
            println!("{contents}");
        }
        ConfAction::Show => {
            if system_config_path().exists() {
                let contents = fs::read_to_string(SYSTEM_CONFIG)?;
                println!("{contents}");
            } else {
                println!("No system config exists to show.");
            }
        }
        ConfAction::Edit => {
            let editor = std::env::var("EDITOR")
                .ok()
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| "vi".to_string());
            let status = Command::new(&editor)
                .arg(SYSTEM_CONFIG)
                .status()
                .with_context(|| {
                    format!("Failed to launch editor '{editor}'")
                })?;
            if !status.success() {
                bail!("Editor '{editor}' exited with {status}");
            }
        }
    }
    Ok(())
}
