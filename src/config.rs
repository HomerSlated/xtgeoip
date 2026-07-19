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
        // The license key is sent as HTTP basic auth on the first request, so
        // a cleartext origin exposes it before any redirect is involved
        // (guardian F-3, #102). `fetch::redirect_policy` refuses an https→http
        // *downgrade*, but it cannot help when the origin is already http:
        // there is no https predecessor to downgrade from. The two checks are
        // complementary halves of one property.
        //
        // Enforced here rather than with reqwest's `.https_only(true)` so that
        // `fetch()` stays scheme-agnostic — its mock-server tests drive
        // `http://127.0.0.1` — and so the change stays out of guardian-signed
        // `fetch.rs`.
        //
        // Scheme comparison is case-insensitive: RFC 3986 defines schemes as
        // case-insensitive, so `HTTPS://` is valid and must pass.
        if !self
            .maxmind
            .url
            .trim()
            .to_ascii_lowercase()
            .starts_with("https://")
        {
            bail!(
                "maxmind.url must use https — the MaxMind license key is sent \
                 as HTTP basic auth and would otherwise cross the network in \
                 cleartext (got {:?})",
                self.maxmind.url.trim()
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_url(url: &str) -> Config {
        Config {
            paths: Paths {
                archive_dir: "/var/lib/xt_geoip".into(),
                archive_prune: 3,
                output_dir: "/usr/share/xt_geoip".into(),
            },
            maxmind: MaxMind {
                url: url.into(),
                account_id: "123456".into(),
                license_key: "key".into(),
            },
            logging: None,
            processing: None,
        }
    }

    #[test]
    fn https_url_is_accepted() {
        assert!(
            config_with_url(
                "https://download.maxmind.com/geoip/databases/\
                 GeoLite2-Country-CSV/download"
            )
            .validate()
            .is_ok()
        );
    }

    /// RFC 3986 schemes are case-insensitive, so this is a valid https URL and
    /// rejecting it would be wrong.
    #[test]
    fn uppercase_scheme_is_accepted() {
        assert!(
            config_with_url("HTTPS://download.maxmind.com/x")
                .validate()
                .is_ok()
        );
    }

    /// The finding this closes (#102): a cleartext origin sends the license
    /// key before any redirect exists to be checked.
    #[test]
    fn http_url_is_rejected() {
        let err = config_with_url("http://download.maxmind.com/x")
            .validate()
            .expect_err("http must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("https"), "unhelpful error: {msg}");
        assert!(
            msg.contains("cleartext"),
            "error should say why, not just what: {msg}"
        );
    }

    /// Loopback gets no exception — the decision was "no exception", so a
    /// local http mirror must be fronted with https rather than special-cased.
    #[test]
    fn http_loopback_is_also_rejected() {
        assert!(
            config_with_url("http://127.0.0.1:8080/download")
                .validate()
                .is_err()
        );
        assert!(
            config_with_url("http://localhost/download")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn other_schemes_are_rejected() {
        for url in ["ftp://example.com/x", "file:///tmp/x", "download"] {
            assert!(
                config_with_url(url).validate().is_err(),
                "accepted non-https url {url:?}"
            );
        }
    }

    /// Leading/trailing whitespace must not smuggle a bad scheme past the
    /// check, and must not fail a good one.
    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert!(
            config_with_url("  https://example.com/x  ")
                .validate()
                .is_ok()
        );
        assert!(
            config_with_url("  http://example.com/x  ")
                .validate()
                .is_err()
        );
    }

    /// The empty check must still fire first, with its own message.
    #[test]
    fn empty_url_still_reports_as_empty() {
        let err = config_with_url("   ").validate().expect_err("must reject");
        assert!(
            err.to_string().contains("must not be empty"),
            "empty url should report emptiness, not scheme: {err}"
        );
    }
}
