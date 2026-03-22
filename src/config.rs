/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    fs,
    io::{self, Write},
    path::Path,
    process::Command,
};

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_CONFIG: &str = "/usr/share/xt_geoip/xtgeoip.conf.example";
const SYSTEM_CONFIG: &str = "/etc/xtgeoip.conf";

#[derive(Debug, Deserialize)]
pub struct Paths {
    pub archive_dir: String,
    pub output_dir: String,
}

#[derive(Debug, Deserialize)]
pub struct MaxMind {
    pub url: String,
    pub account_id: String,
    pub license_key: String,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub paths: Paths,
    pub maxmind: MaxMind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfAction {
    Show,
    Edit,
    Default,
    Help,
}

/// Prompt user to create system config if missing (used for `-s` or `-e`)
fn ensure_system_config_exists() -> io::Result<()> {
    let path = Path::new(SYSTEM_CONFIG);
    if path.exists() {
        return Ok(());
    }

    println!("Configuration file not found at {SYSTEM_CONFIG}.");
    print!("Do you want to create it from the default example? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    if input == "y" || input == "yes" {
        fs::copy(DEFAULT_CONFIG, SYSTEM_CONFIG)?;
        println!("Created {SYSTEM_CONFIG} from default example.");
    } else {
        println!(
            "Skipping creation of system config. You can edit it manually later."
        );
    }

    Ok(())
}

/// Perform the requested action for `xtgeoip conf`
pub fn run_conf(action: ConfAction) -> Result<()> {
    match action {
        ConfAction::Default => {
            // Show default config, ignore system config existence
            let contents = fs::read_to_string(DEFAULT_CONFIG)?;
            println!("{contents}");
        }
        ConfAction::Show => {
            ensure_system_config_exists()?;
            if Path::new(SYSTEM_CONFIG).exists() {
                let contents = fs::read_to_string(SYSTEM_CONFIG)?;
                println!("{contents}");
            } else {
                println!("No system config exists to show.");
            }
        }
        ConfAction::Edit => {
            ensure_system_config_exists()?;
            let editor =
                std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            if !Path::new(SYSTEM_CONFIG).exists() {
                fs::File::create(SYSTEM_CONFIG)?;
            }
            Command::new(editor)
                .arg(SYSTEM_CONFIG)
                .status()
                .context("Failed to launch editor")?;
        }
        ConfAction::Help => {
            print_usage();
        }
    }

    Ok(())
}

/// Parses the flag provided to `xtgeoip conf`
/// Default behavior (no flag) is Show
pub fn parse_conf_flag(flag: Option<&str>) -> Result<ConfAction, String> {
    match flag {
        Some("-d") | Some("--default") => Ok(ConfAction::Default),
        Some("-s") | Some("--show") | None => Ok(ConfAction::Show),
        Some("-e") | Some("--edit") => Ok(ConfAction::Edit),
        Some("-h") | Some("--help") => Ok(ConfAction::Help),
        Some(other) => Err(format!(
            "Unsupported flag '{other}'\nUsage: xtgeoip conf [-d|-s|-e|-h]"
        )),
    }
}

/// Load the TOML configuration into a Config struct
pub fn load_config() -> Result<Config> {
    let path = Path::new(SYSTEM_CONFIG);

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

    Ok(cfg)
}

/// Simple usage message
fn print_usage() {
    println!("Usage: xtgeoip conf [-d|-s|-e|-h]");
    println!("  -d, --default   Show default configuration");
    println!("  -s, --show      Show current configuration (default)");
    println!("  -e, --edit      Edit current configuration in $EDITOR");
    println!("  -h, --help      Show this help message");
}
