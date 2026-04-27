/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::Path,
    process::Command,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const DEFAULT_CONFIG: &str = "/usr/share/xt_geoip/xtgeoip.conf.example";
const SYSTEM_CONFIG: &str = "/etc/xtgeoip.conf";

fn system_config_path() -> &'static Path {
    Path::new(SYSTEM_CONFIG)
}

#[derive(Debug, Deserialize)]
pub struct Paths {
    pub archive_dir: String,
    pub archive_prune: usize,
    pub output_dir: String,
}

#[derive(Debug, Deserialize)]
pub struct MaxMind {
    pub url: String,
    pub account_id: String,
    pub license_key: String,
}

#[derive(Debug, Deserialize)]
pub struct Logging {
    pub log_file: String,
}

#[derive(Debug, Deserialize)]
pub struct Processing {
    /// Number of Rayon worker threads. 0 or absent = use all available cores.
    pub threads: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub paths: Paths,
    pub maxmind: MaxMind,
    pub logging: Option<Logging>,
    pub processing: Option<Processing>,
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.paths.archive_dir.trim().is_empty() {
            bail!("paths.archive_dir must not be empty");
        }
        if self.paths.output_dir.trim().is_empty() {
            bail!("paths.output_dir must not be empty");
        }
        if self.maxmind.url.trim().is_empty() {
            bail!("maxmind.url must not be empty");
        }
        if self.maxmind.account_id.trim().is_empty() {
            bail!("maxmind.account_id must not be empty");
        }
        if self.maxmind.license_key.trim().is_empty() {
            bail!("maxmind.license_key must not be empty");
        }
        Ok(())
    }
}

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
                        "Cannot edit: {SYSTEM_CONFIG} does not exist. \
                         Run `xtgeoip conf -d` to view the default config, \
                         then create {SYSTEM_CONFIG} manually."
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
            "Default config example not found at {DEFAULT_CONFIG}. \
             You may need to reinstall xtgeoip."
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
        eprintln!(
            "Error: {SYSTEM_CONFIG} does not exist. \
             Run `xtgeoip conf -d` to view the default config, \
             then create {SYSTEM_CONFIG} manually."
        );
        bail!("{SYSTEM_CONFIG} missing and stdin is not a terminal");
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
                .with_context(|| format!("Failed to launch editor '{editor}'"))?;
            if !status.success() {
                bail!("Editor '{editor}' exited with {status}");
            }
        }
    }
    Ok(())
}

/// Load the TOML configuration into a Config struct
pub fn load_config() -> Result<Config> {
    let path = system_config_path();

    if !path.exists() {
        anyhow::bail!("{} missing", SYSTEM_CONFIG);
    }

    let contents = fs::read_to_string(path)
        .context("Failed to read system configuration file")?;

    if contents.trim().is_empty() {
        anyhow::bail!("{} is empty", SYSTEM_CONFIG);
    }

    let cfg: Config = toml::from_str(&contents)
        .context("Failed to parse TOML configuration")?;

    cfg.validate()?;

    Ok(cfg)
}
