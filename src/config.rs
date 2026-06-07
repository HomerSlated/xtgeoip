/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip configuration data and loading. Pure: no output, no
/// subprocesses, no prompts — see `conf.rs` for the `conf` subcommand
/// handler.
use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub(crate) const SYSTEM_CONFIG: &str = "/etc/xtgeoip.conf";

pub(crate) fn system_config_path() -> &'static Path {
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
