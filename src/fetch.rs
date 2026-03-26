/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use reqwest::{blocking::Client, header::CONTENT_DISPOSITION};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zip::ZipArchive;

use crate::config::Config;

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
        let (archive_path, version) = find_latest_local_csv_archive(archive_dir)?;
        println!("Using latest local archive: {}", archive_path.display());
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
        eprintln!(
            "Warning: MaxMind account ID or license key not set in config. \
             Skipping fetch."
        );
        bail!("MaxMind credentials not configured");
    }

    fs::create_dir_all(archive_dir)?;

    let client = Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()?;

    println!("Checking remote archive version...");

    let resp = client
        .get(format!("{maxmind_url}?suffix=zip"))
        .basic_auth(account_id, Some(license_key))
        .send()
        .context("Failed to query MaxMind archive")?;

    if !resp.status().is_success() {
        bail!("Remote request failed: {}", resp.status());
    }

    // Extract version from Content-Disposition filename
    let remote_filename = resp
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let version = extract_version(remote_filename).ok_or_else(|| {
        anyhow::anyhow!("Failed to parse version from filename")
    })?;

    println!("Remote archive version: {version}");

    let archive_path =
        archive_dir.join(format!("GeoLite2-Country-CSV_{version}.zip"));
    let checksum_path =
        archive_dir.join(format!("GeoLite2-Country-CSV_{version}.zip.sha256"));

    if archive_path.exists() && checksum_path.exists() {
        println!("Reusing local copy: {}", archive_path.display());
        let temp_dir = extract_archive_to_temp(&archive_path)?;
        return Ok((temp_dir, version));
    }

    println!("No local copy of this version. Downloading...");

    // Stream archive directly to file + hash while copying
    let mut archive_file =
        File::create(&archive_path).context("Failed to create archive file")?;

    let mut hasher = Sha256::new();

    {
        let mut hashing_writer = HashingWriter {
            inner: &mut archive_file,
            hasher: &mut hasher,
        };

        let mut limited = resp.take(10 * 1024 * 1024 * 1024); // 10GB safety cap
        io::copy(&mut limited, &mut hashing_writer)
            .context("Failed while downloading archive")?;
    }

    let actual_hash = format!("{:x}", hasher.finalize());

    // Download checksum
    let checksum_url = format!("{maxmind_url}?suffix=zip.sha256");
    let mut checksum_resp = client
        .get(&checksum_url)
        .basic_auth(account_id, Some(license_key))
        .send()
        .context("Failed to download checksum")?;

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
        fs::remove_file(&archive_path).ok(); // remove partial file
        bail!(
            "Checksum verification failed: expected {expected_hash}, got \
             {actual_hash}"
        );
    }

    println!("Checksum verification successful.");

    // Save checksum
    fs::write(&checksum_path, checksum_text)
        .context("Failed to save checksum")?;

    println!("Saved archive as {}", archive_path.display());

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

        let version = &name[prefix.len()..name.len() - suffix.len()];

        // Must be exactly 8 digits
        if version.len() != 8 || !version.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        match &best {
            Some((_, best_version)) if version <= best_version.as_str() => {}
            _ => best = Some((path, version.to_string())),
        }
    }

    best.ok_or_else(|| {
        anyhow::anyhow!(
            "No valid local GeoLite2 Country CSV archive found in {}\n\
             Run 'xtgeoip fetch' first, or use 'xtgeoip run'.",
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

/// Extracts version/date from filename like:
/// `GeoLite2-Country-CSV_20260227.zip`
fn extract_version(filename: &str) -> Option<String> {
    filename
        .split('_')
        .nth(1)
        .and_then(|s| s.strip_suffix(".zip"))
        .map(|s| s.to_string())
}
