/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use csv::ReaderBuilder;
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

use crate::{config::Config, messages, version::Version};

#[derive(Clone, Copy, Debug)]
pub enum FetchMode {
    Remote,
    Local,
}

pub fn fetch(config: &Config, mode: FetchMode) -> Result<(TempDir, Version)> {
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
        validate_csv_contents(temp_dir.path())?;
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

    let cd_filename =
        parse_content_disposition_filename(&content_disposition).ok_or_else(
            || {
                anyhow::anyhow!(
                    "Could not extract filename from Content-Disposition: {:?}",
                    content_disposition
                )
            },
        )?;

    let version = Version::parse(cd_filename).ok_or_else(|| {
        anyhow::anyhow!(
            "Could not extract version from archive filename {:?}",
            cd_filename
        )
    })?;

    if !(version.as_str().len() == 8
        && version.as_str().chars().all(|c| c.is_ascii_digit()))
    {
        messages::warn(&format!(
            "Archive version token {:?} does not look like a date — proceeding anyway",
            version
        ));
    }

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
                validate_csv_contents(temp_dir.path())?;
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
    validate_csv_contents(temp_dir.path())?;
    Ok((temp_dir, version))
}

/// Find the latest valid local archive matching:
/// `GeoLite2-Country-CSV_YYYYMMDD.zip`
fn find_latest_local_csv_archive(
    archive_dir: &Path,
) -> Result<(PathBuf, Version)> {
    let mut best: Option<(PathBuf, Version)> = None;

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

        if !name.starts_with("GeoLite2-Country-CSV_") || !name.ends_with(".zip")
        {
            continue;
        }

        let version = match Version::parse(name) {
            Some(v) => v,
            None => {
                messages::warn(&format!(
                    "Skipping archive with unexpected name: {name}"
                ));
                continue;
            }
        };

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

/// Check that `path` starts with the ZIP local-file signature (`PK\x03\x04`).
fn verify_zip_magic(path: &Path) -> Result<()> {
    let mut f = File::open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).with_context(|| {
        format!("Failed to read magic bytes from {}", path.display())
    })?;
    if magic != [0x50, 0x4B, 0x03, 0x04] {
        bail!(
            "Not a valid ZIP archive (bad magic bytes): {}",
            path.display()
        );
    }
    Ok(())
}

/// Scan all ZIP entries for security issues and detect the common top-level
/// directory prefix.
///
/// Rejects path traversal, absolute paths, and entries with executable bits.
/// Returns `Some(name)` when all entries share one top-level directory (so the
/// caller can strip it), or `None` for flat or multi-root archives.
fn scan_zip_entries(zip: &mut ZipArchive<File>) -> Result<Option<String>> {
    let mut prefix: Option<String> = None;
    let mut has_nested = false;
    let mut prefix_ambiguous = false;

    for i in 0..zip.len() {
        let entry = zip.by_index(i).context("Failed to read ZIP entry")?;
        let raw_name = entry.name().to_owned();

        if raw_name.split(['/', '\\']).any(|c| c == "..") {
            bail!("ZIP entry contains path traversal: {:?}", raw_name);
        }
        if raw_name.starts_with('/')
            || raw_name.starts_with('\\')
            || raw_name.contains(":/")
            || raw_name.contains(":\\")
        {
            bail!("ZIP entry contains absolute path: {:?}", raw_name);
        }
        if !entry.is_dir()
            && let Some(mode) = entry.unix_mode()
            && mode & 0o111 != 0
        {
            bail!("ZIP entry has executable bits set: {:?}", raw_name);
        }

        if prefix_ambiguous {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            bail!("ZIP entry has unsanitizable path: {:?}", raw_name);
        };
        let mut comps = enclosed.components();
        let first = match comps.next() {
            Some(c) => c.as_os_str().to_string_lossy().into_owned(),
            None => continue,
        };
        if comps.next().is_some() {
            has_nested = true;
        }
        match &prefix {
            None => prefix = Some(first),
            Some(prev) if prev == &first => {}
            Some(_) => prefix_ambiguous = true,
        }
    }

    if prefix_ambiguous || !has_nested { Ok(None) } else { Ok(prefix) }
}

/// Extract zip archive into a temporary directory and return it.
///
/// Validates magic bytes and scans all entries for security issues before
/// extracting. Strips the common top-level directory prefix so that CSV files
/// land directly in the temp root.
fn extract_archive_to_temp(archive_path: &Path) -> Result<TempDir> {
    verify_zip_magic(archive_path)?;
    let temp_dir = TempDir::new()
        .context("Failed to create temporary extraction directory")?;
    let file = File::open(archive_path)
        .context("Failed to open archive for extraction")?;
    let mut zip =
        ZipArchive::new(file).context("Failed to read zip archive")?;

    let prefix = scan_zip_entries(&mut zip)?;
    if prefix.is_none() && !zip.is_empty() {
        messages::warn(
            "ZIP archive lacks a common top-level directory; extracting flat.",
        );
    }

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).context("Failed to read zip entry")?;

        let enclosed = entry.enclosed_name().ok_or_else(|| {
            anyhow::anyhow!("Zip entry contains invalid path")
        })?;

        let relative: PathBuf = if prefix.is_some() {
            enclosed.components().skip(1).collect()
        } else {
            enclosed.to_owned()
        };

        if relative.as_os_str().is_empty() {
            continue;
        }

        let outpath = temp_dir.path().join(&relative);

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

/// Extract the `filename=` value from a `Content-Disposition` header.
///
/// Handles both quoted and unquoted forms:
/// - `attachment; filename=GeoLite2-Country-CSV_20260227.zip`
/// - `attachment; filename="GeoLite2-Country-CSV_20260227.zip"`
fn parse_content_disposition_filename(cd: &str) -> Option<&str> {
    let filename = cd
        .split(';')
        .map(str::trim)
        .find(|part| part.to_ascii_lowercase().starts_with("filename="))?
        .split_once('=')?
        .1
        .trim_matches('"');
    if filename.is_empty() { None } else { Some(filename) }
}

/// Validate CSV files extracted into `dir`: locations (en) and both blocks
/// files must exist, have the required columns, and pass first-row sanity
/// checks.
fn validate_csv_contents(dir: &Path) -> Result<()> {
    validate_locations_csv(&dir.join("GeoLite2-Country-Locations-en.csv"))?;
    for suffix in ["IPv4", "IPv6"] {
        validate_blocks_csv(
            &dir.join(format!("GeoLite2-Country-Blocks-{suffix}.csv")),
        )?;
    }
    Ok(())
}

fn validate_locations_csv(path: &Path) -> Result<()> {
    let mut rdr = ReaderBuilder::new().from_path(path).with_context(|| {
        format!("Failed to open {}", path.display())
    })?;
    let headers = rdr
        .headers()
        .with_context(|| {
            format!("Failed to read headers from {}", path.display())
        })?
        .clone();
    for col in ["geoname_id", "country_iso_code", "continent_code"] {
        if !headers.iter().any(|h| h == col) {
            bail!(
                "Missing required column {:?} in {}",
                col,
                path.display()
            );
        }
    }
    let gid_idx = headers.iter().position(|h| h == "geoname_id").unwrap();
    if let Some(result) = rdr.records().next() {
        let rec = result.with_context(|| {
            format!("Failed to read first row of {}", path.display())
        })?;
        if let Some(val) = rec.get(gid_idx)
            && val.parse::<u64>().is_err()
        {
            bail!(
                "geoname_id {:?} is not numeric in {}",
                val,
                path.display()
            );
        }
    }
    Ok(())
}

fn validate_blocks_csv(path: &Path) -> Result<()> {
    let mut rdr = ReaderBuilder::new().from_path(path).with_context(|| {
        format!("Failed to open {}", path.display())
    })?;
    let headers = rdr
        .headers()
        .with_context(|| {
            format!("Failed to read headers from {}", path.display())
        })?
        .clone();
    for col in [
        "network",
        "geoname_id",
        "is_anonymous_proxy",
        "is_satellite_provider",
    ] {
        if !headers.iter().any(|h| h == col) {
            bail!(
                "Missing required column {:?} in {}",
                col,
                path.display()
            );
        }
    }
    if let Some(result) = rdr.records().next() {
        let rec = result.with_context(|| {
            format!("Failed to read first row of {}", path.display())
        })?;
        let net_idx = headers.iter().position(|h| h == "network").unwrap();
        if let Some(net) = rec.get(net_idx)
            && !net.contains('/')
        {
            messages::warn(&format!(
                "First network {:?} in {} does not look like CIDR",
                net,
                path.display()
            ));
        }
        for col in ["is_anonymous_proxy", "is_satellite_provider"] {
            let idx = headers.iter().position(|h| h == col).unwrap();
            if let Some(val) = rec.get(idx)
                && val != "0" && val != "1"
            {
                messages::warn(&format!(
                    "{col:?} value {val:?} in {} is not 0 or 1",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}
