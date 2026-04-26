/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::{
    blocking::Client,
    header::{CONTENT_DISPOSITION, CONTENT_LENGTH},
};
use sha2::{Digest, Sha256};

const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB
const SIZE_TOLERANCE: f64 = 0.5; // ±50% of last known archive size
const DEFAULT_TIMEOUT_SECS: u64 = 300;
const MAX_RETRIES: u32 = 3;
const BASE_DELAY_SECS: u64 = 2;
use tempfile::TempDir;
use zip::ZipArchive;

use crate::{config::Config, messages};

#[derive(Clone, Copy, Debug)]
pub enum FetchMode {
    Remote,
    Local,
}

pub fn fetch(config: &Config, mode: FetchMode) -> Result<(TempDir, String)> {
    let archive_dir = Path::new(&config.paths.archive_dir);

    // Local-only mode: skip remote entirely, use latest valid local archive
    if matches!(mode, FetchMode::Local) {
        fs::create_dir_all(archive_dir)?;
        let (archive_path, version) =
            find_latest_local_csv_archive(archive_dir)?;
        messages::info(&format!(
            "Using latest local archive: {}",
            archive_path.display()
        ));
        let temp_dir = extract_archive_to_temp(&archive_path)?;
        return Ok((temp_dir, version));
    }

    let maxmind_url = &config.maxmind.url;
    let account_id = &config.maxmind.account_id;
    let license_key = &config.maxmind.license_key;

    // Skip if account_id or license_key are not set
    if account_id.is_empty()
        || account_id == "CHANGE ME"
        || license_key.is_empty()
        || license_key == "CHANGE ME"
    {
        messages::error("MaxMind account ID or license key not set in config.");
        bail!("MaxMind credentials not configured");
    }

    fs::create_dir_all(archive_dir)?;

    let client = Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .build()?;

    messages::info("Checking remote archive version...");

    let resp = send_with_retry(|| {
        client
            .get(format!("{maxmind_url}?suffix=zip"))
            .basic_auth(account_id, Some(license_key))
            .send()
    })?;

    if !resp.status().is_success() {
        bail!("Remote request failed: {}", resp.status());
    }

    // Parse Content-Disposition; each failure mode gets a distinct message
    let content_disposition = resp
        .headers()
        .get(CONTENT_DISPOSITION)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Content-Disposition header absent from MaxMind response"
            )
        })?
        .to_str()
        .context("Content-Disposition header contains non-UTF-8 characters")?
        .to_owned();

    let version = extract_version(&content_disposition).ok_or_else(|| {
        anyhow::anyhow!(
            "Could not extract version from Content-Disposition: {:?}",
            content_disposition
        )
    })?;

    messages::info(&format!("Remote archive version: {version}"));

    // Guard against absurd Content-Length before doing anything else
    let content_length = resp
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    if let Some(len) = content_length {
        if len > MAX_DOWNLOAD_BYTES {
            bail!(
                "Content-Length {len} exceeds maximum allowed size \
                 {MAX_DOWNLOAD_BYTES}"
            );
        }
        if let Ok((prev_path, _)) = find_latest_local_csv_archive(archive_dir)
            && let Ok(meta) = fs::metadata(&prev_path)
        {
            let prev = meta.len();
            let lo = (prev as f64 * (1.0 - SIZE_TOLERANCE)) as u64;
            let hi = (prev as f64 * (1.0 + SIZE_TOLERANCE)) as u64;
            if len < lo || len > hi {
                messages::warn(&format!(
                    "Remote Content-Length {len} is outside expected \
                     range [{lo}, {hi}] (±50% of previous {prev} bytes). \
                     Proceeding with caution."
                ));
            }
        }
    }

    let archive_path =
        archive_dir.join(format!("GeoLite2-Country-CSV_{version}.zip"));
    let checksum_path =
        archive_dir.join(format!("GeoLite2-Country-CSV_{version}.zip.sha256"));

    // Re-verify cached archive before trusting it
    if archive_path.exists() && checksum_path.exists() {
        match verify_cached_archive(&archive_path, &checksum_path) {
            Ok(true) => {
                messages::info(&format!(
                    "Reusing verified local copy: {}",
                    archive_path.display()
                ));
                let temp_dir = extract_archive_to_temp(&archive_path)?;
                return Ok((temp_dir, version));
            }
            Ok(false) => {
                messages::warn(
                    "Local archive checksum mismatch — re-downloading.",
                );
            }
            Err(e) => {
                messages::warn(&format!(
                    "Could not verify local archive: {e:#} — re-downloading."
                ));
            }
        }
    }

    messages::info("No verified local copy of this version. Downloading...");

    // Download to a .part file; rename atomically on success
    let tmp_path = archive_path.with_extension("zip.part");

    // Stream archive directly to file + hash while copying
    let mut archive_file =
        File::create(&tmp_path).context("Failed to create archive file")?;

    let mut hasher = Sha256::new();

    let written = {
        let mut hashing_writer = HashingWriter {
            inner: &mut archive_file,
            hasher: &mut hasher,
        };
        // +1 so we can detect a breach vs. exactly-at-limit
        let mut limited = resp.take(MAX_DOWNLOAD_BYTES + 1);
        io::copy(&mut limited, &mut hashing_writer)
            .context("Failed while downloading archive")?
    };

    if written > MAX_DOWNLOAD_BYTES {
        let _ = fs::remove_file(&tmp_path);
        bail!(
            "Download exceeded {MAX_DOWNLOAD_BYTES} bytes — refusing to use \
             truncated archive"
        );
    }

    let actual_hash = format!("{:x}", hasher.finalize());

    // Download checksum
    let checksum_url = format!("{maxmind_url}?suffix=zip.sha256");
    let mut checksum_resp = send_with_retry(|| {
        client
            .get(&checksum_url)
            .basic_auth(account_id, Some(license_key))
            .send()
    })?;

    if !checksum_resp.status().is_success() {
        bail!("Checksum request failed: {}", checksum_resp.status());
    }

    let mut checksum_text = String::new();
    checksum_resp
        .read_to_string(&mut checksum_text)
        .context("Failed to read checksum response")?;

    let expected_hash = checksum_text
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid checksum format"))?;

    // Verify checksum
    if actual_hash != expected_hash {
        if let Err(e) = fs::remove_file(&tmp_path) {
            messages::warn(&format!(
                "Failed to remove partial download {}: {}",
                tmp_path.display(),
                e
            ));
        }
        bail!(
            "Checksum verification failed for {}: expected {}, got {}",
            archive_path.display(),
            expected_hash,
            actual_hash
        );
    }

    messages::info("Checksum verification successful.");

    fs::rename(&tmp_path, &archive_path).with_context(|| {
        format!(
            "Failed to rename {} to {}",
            tmp_path.display(),
            archive_path.display()
        )
    })?;

    // Save checksum
    fs::write(&checksum_path, checksum_text)
        .context("Failed to save checksum")?;

    messages::info(&format!("Saved archive as {}", archive_path.display()));

    let temp_dir = extract_archive_to_temp(&archive_path)?;
    Ok((temp_dir, version))
}

/// Find the latest valid local archive matching:
/// `GeoLite2-Country-CSV_YYYYMMDD.zip`
fn find_latest_local_csv_archive(
    archive_dir: &Path,
) -> Result<(PathBuf, String)> {
    let mut best: Option<(PathBuf, String)> = None;

    for entry in fs::read_dir(archive_dir)
        .with_context(|| format!("Failed to read {}", archive_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let prefix = "GeoLite2-Country-CSV_";
        let suffix = ".zip";

        if !name.starts_with(prefix) || !name.ends_with(suffix) {
            continue;
        }

        let version = name[prefix.len()..name.len() - suffix.len()].to_string();

        // Must be exactly 8 digits
        if version.len() != 8 || !version.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        match &best {
            Some((_, best_version)) if version <= *best_version => {}
            _ => best = Some((path, version)),
        }
    }

    best.ok_or_else(|| {
        anyhow::anyhow!(
            "No valid local GeoLite2 Country CSV archive found in {}\nRun \
             'xtgeoip fetch' first, or use 'xtgeoip run'.",
            archive_dir.display()
        )
    })
}

/// Extract zip archive into a temporary directory and return it
fn extract_archive_to_temp(archive_path: &Path) -> Result<TempDir> {
    let temp_dir = TempDir::new()
        .context("Failed to create temporary extraction directory")?;
    let file = File::open(archive_path)
        .context("Failed to open archive for extraction")?;
    let mut zip =
        ZipArchive::new(file).context("Failed to read zip archive")?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).context("Failed to read zip entry")?;

        let enclosed = entry.enclosed_name().ok_or_else(|| {
            anyhow::anyhow!("Zip entry contains invalid path")
        })?;

        let outpath = flatten_to_temp_root(temp_dir.path(), &enclosed);

        if entry.is_dir() {
            fs::create_dir_all(&outpath).with_context(|| {
                format!("Failed to create directory {}", outpath.display())
            })?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create directory {}", parent.display())
                })?;
            }

            let mut outfile = File::create(&outpath).with_context(|| {
                format!("Failed to create {}", outpath.display())
            })?;
            io::copy(&mut entry, &mut outfile).with_context(|| {
                format!("Failed to extract {}", outpath.display())
            })?;
        }
    }

    Ok(temp_dir)
}

/// Flatten MaxMind's top-level versioned directory so CSVs land directly in
/// temp root
fn flatten_to_temp_root(temp_root: &Path, entry_path: &Path) -> PathBuf {
    let components: Vec<_> = entry_path.components().collect();

    let slice = if components.len() > 1 {
        &components[1..]
    } else {
        &components[..]
    };

    let mut outpath = PathBuf::from(temp_root);
    for comp in slice {
        outpath.push(comp.as_os_str());
    }

    outpath
}

/// Writer wrapper that hashes while writing
struct HashingWriter<'a, W: Write> {
    inner: &'a mut W,
    hasher: &'a mut Sha256,
}

impl<'a, W: Write> Write for HashingWriter<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Retry a send closure on transient network errors or 5xx responses.
fn send_with_retry<F>(f: F) -> Result<reqwest::blocking::Response>
where
    F: Fn() -> reqwest::Result<reqwest::blocking::Response>,
{
    let mut attempt = 0u32;
    loop {
        match f() {
            Err(e) if attempt < MAX_RETRIES && (e.is_timeout() || e.is_connect()) => {
                let delay = BASE_DELAY_SECS * 2u64.pow(attempt);
                messages::warn(&format!(
                    "Transient network error (attempt {}/{MAX_RETRIES}): {e}. \
                     Retrying in {delay}s...",
                    attempt + 1
                ));
                thread::sleep(Duration::from_secs(delay));
                attempt += 1;
            }
            Err(e) => return Err(e.into()),
            Ok(resp) if resp.status().is_server_error() && attempt < MAX_RETRIES => {
                let delay = BASE_DELAY_SECS * 2u64.pow(attempt);
                messages::warn(&format!(
                    "Server error {} (attempt {}/{MAX_RETRIES}). \
                     Retrying in {delay}s...",
                    resp.status(),
                    attempt + 1
                ));
                thread::sleep(Duration::from_secs(delay));
                attempt += 1;
            }
            Ok(resp) => return Ok(resp),
        }
    }
}

/// Re-verify a cached archive against its stored SHA-256 checksum.
fn verify_cached_archive(
    archive_path: &Path,
    checksum_path: &Path,
) -> Result<bool> {
    let checksum_text =
        fs::read_to_string(checksum_path).with_context(|| {
            format!(
                "Failed to read checksum file {}",
                checksum_path.display()
            )
        })?;
    let expected_hash = checksum_text
        .split_whitespace()
        .next()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid checksum format in {}",
                checksum_path.display()
            )
        })?;
    let data = fs::read(archive_path).with_context(|| {
        format!("Failed to read archive {}", archive_path.display())
    })?;
    let actual_hash = format!("{:x}", Sha256::digest(&data));
    Ok(actual_hash == expected_hash)
}

/// Extracts the 8-digit date version from a Content-Disposition header.
///
/// Handles: `attachment; filename=GeoLite2-Country-CSV_20260227.zip`
/// and:     `attachment; filename="GeoLite2-Country-CSV_20260227.zip"`
fn extract_version(content_disposition: &str) -> Option<String> {
    let filename = content_disposition
        .split(';')
        .map(str::trim)
        .find(|part| part.to_ascii_lowercase().starts_with("filename="))?
        .split_once('=')?
        .1
        .trim_matches('"');

    // Filename is now e.g. "GeoLite2-Country-CSV_20260227.zip"
    let version_part = filename.split('_').nth(1)?;
    let digits: String = version_part
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();

    if digits.len() == 8 {
        Some(digits)
    } else {
        None
    }
}
