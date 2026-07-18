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
/// Cumulative cap on bytes written during archive extraction.
/// `MAX_DOWNLOAD_BYTES` only bounds the *compressed* download; without this a
/// small archive that decompresses to many GiB (a "zip bomb") would exhaust the
/// extraction filesystem. The binary runs as root, and `FetchMode::Local`
/// extracts archives already sitting in `archive_dir` with no network trust
/// boundary — so the cap must guard extraction, not just download. Real
/// GeoLite2 Country CSV data is tens of MiB; 2 GiB is generous headroom while
/// still bounding disk use. (Guardian audit finding M-1.)
const MAX_EXTRACT_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
const SIZE_TOLERANCE: f64 = 0.5; // ±50% of last known archive size
const DEFAULT_TIMEOUT_SECS: u64 = 300;
const MAX_RETRIES: u32 = 3;
const BASE_DELAY_SECS: u64 = 2;

/// Maximum redirect hops the MaxMind client will follow (#101). One hop is
/// observed in practice; this leaves headroom without being unbounded.
const MAX_REDIRECTS: usize = 3;
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
        let temp_dir = extract_and_validate(&archive_path)?;
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
        bail!("MaxMind account ID or license key not set in config.");
    }

    fs::create_dir_all(archive_dir)?;

    let client = Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .redirect(redirect_policy())
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

    // Header-derived facts (version, size guard) must be read before the
    // response body is consumed by acquire_remote_archive below.
    let version = resolve_version(&resp)?;
    messages::info(&format!("Remote archive version: {version}"));
    check_download_size(&resp, archive_dir)?;

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
                let temp_dir = extract_and_validate(&archive_path)?;
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
    acquire_remote_archive(
        &client,
        resp,
        account_id,
        license_key,
        maxmind_url,
        &archive_path,
        &checksum_path,
    )?;

    let temp_dir = extract_and_validate(&archive_path)?;
    Ok((temp_dir, version))
}

/// Resolve the archive version from the response's `Content-Disposition`
/// filename. Reads only headers, so it must be called before the body is
/// consumed by [`acquire_remote_archive`].
fn resolve_version(resp: &reqwest::blocking::Response) -> Result<Version> {
    let content_disposition = resp
        .headers()
        .get(CONTENT_DISPOSITION)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Content-Disposition header absent from MaxMind response"
            )
        })?
        .to_str()
        .context("Content-Disposition header contains non-UTF-8 characters")?;

    let cd_filename = parse_content_disposition_filename(content_disposition)
        .ok_or_else(|| {
        anyhow::anyhow!(
            "Could not extract filename from Content-Disposition: {:?}",
            content_disposition
        )
    })?;

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
            "Archive version token {:?} does not look like a date — \
             proceeding anyway",
            version
        ));
    }

    Ok(version)
}

/// Reject an absurd `Content-Length`, and warn if it deviates far from the last
/// known archive size. Reads only headers, so it must be called before the body
/// is consumed by [`acquire_remote_archive`].
fn check_download_size(
    resp: &reqwest::blocking::Response,
    archive_dir: &Path,
) -> Result<()> {
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
                    "Remote Content-Length {len} is outside expected range \
                     [{lo}, {hi}] (±50% of previous {prev} bytes). Proceeding \
                     with caution."
                ));
            }
        }
    }
    Ok(())
}

/// Download the archive body plus its checksum, verify the SHA-256, and move
/// the archive + checksum atomically into place. Consumes `resp` (the archive
/// body).
/// Deletes a partial download on drop unless [`disarm`](Self::disarm)ed.
///
/// A failed fetch must leave no ephemeral data behind. The two explicit
/// cleanups that used to exist covered the size-breach and checksum-mismatch
/// paths only; six others — a dropped connection mid-copy, a failed or
/// non-success checksum request, an unreadable or malformed checksum body, and
/// a failed rename — returned via `?` or `bail!` and leaked the `.part` file.
///
/// Leaked files were inert but immortal: `find_latest_local_csv_archive`
/// requires `.zip`, so they were never mistaken for an archive, but
/// `prune_csv_archives` matches only `.zip`/`.zip.sha256`, so they were never
/// pruned either — accumulating unboundedly in `archive_dir`.
///
/// Doing this with `Drop` rather than more explicit cleanups means new error
/// paths are covered by construction rather than by remembering.
struct PartialDownload<'a> {
    path: &'a Path,
    armed: bool,
}

impl<'a> PartialDownload<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path, armed: true }
    }

    /// Call once the file has been renamed into place and must be kept.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PartialDownload<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Err(e) = fs::remove_file(self.path)
            && e.kind() != io::ErrorKind::NotFound
        {
            messages::warn(&format!(
                "Failed to remove partial download {}: {e}",
                self.path.display()
            ));
        }
    }
}

fn acquire_remote_archive(
    client: &Client,
    resp: reqwest::blocking::Response,
    account_id: &str,
    license_key: &str,
    maxmind_url: &str,
    archive_path: &Path,
    checksum_path: &Path,
) -> Result<()> {
    // Download to a .part file; rename atomically on success
    let tmp_path = archive_path.with_extension("zip.part");
    // Armed from here on: every early return below removes the .part file.
    let mut partial = PartialDownload::new(&tmp_path);

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
        // `partial` removes the .part file on the way out.
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
        bail!(
            "Checksum verification failed for {}: expected {}, got {}",
            archive_path.display(),
            expected_hash,
            actual_hash
        );
    }

    messages::info("Checksum verification successful.");

    fs::rename(&tmp_path, archive_path).with_context(|| {
        format!(
            "Failed to rename {} to {}",
            tmp_path.display(),
            archive_path.display()
        )
    })?;
    // Renamed into place: the .part path no longer exists and must not be
    // pursued on drop.
    partial.disarm();

    // Save checksum
    fs::write(checksum_path, checksum_text)
        .context("Failed to save checksum")?;

    messages::info(&format!("Saved archive as {}", archive_path.display()));
    Ok(())
}

/// Extract an archive to a temp dir and validate the CSV contents it must
/// contain. Single home for the extract-then-validate step shared by all three
/// `fetch()` exit paths (local, cached-reuse, post-download) — so no path can
/// silently skip validation.
fn extract_and_validate(archive_path: &Path) -> Result<TempDir> {
    let temp_dir = extract_archive_to_temp(archive_path)?;
    validate_csv_contents(temp_dir.path())?;
    Ok(temp_dir)
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

    if prefix_ambiguous || !has_nested {
        Ok(None)
    } else {
        Ok(prefix)
    }
}

/// Extract zip archive into a temporary directory and return it.
///
/// Validates magic bytes and scans all entries for security issues before
/// extracting. Strips the common top-level directory prefix so that CSV files
/// land directly in the temp root.
fn extract_archive_to_temp(archive_path: &Path) -> Result<TempDir> {
    extract_archive_to_temp_capped(archive_path, MAX_EXTRACT_BYTES)
}

/// Extraction worker with an explicit byte budget. Split out from
/// [`extract_archive_to_temp`] so tests can drive the [`MAX_EXTRACT_BYTES`] cap
/// with a tiny limit instead of generating gigabytes.
fn extract_archive_to_temp_capped(
    archive_path: &Path,
    max_bytes: u64,
) -> Result<TempDir> {
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

    let mut total_written: u64 = 0;

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
            // Bound each entry's copy by the remaining budget, +1 so a breach
            // is detectable (mirrors the download cap's `take(MAX +
            // 1)` idiom). A cumulative check *after* an unbounded
            // `io::copy` would be a hole: one entry could exhaust
            // the disk before the check runs.
            let remaining = max_bytes - total_written;
            let mut limited = (&mut entry).take(remaining + 1);
            let n =
                io::copy(&mut limited, &mut outfile).with_context(|| {
                    format!("Failed to extract {}", outpath.display())
                })?;
            total_written += n;
            if total_written > max_bytes {
                bail!(
                    "Archive extraction exceeded {max_bytes} bytes — refusing \
                     to unpack possible decompression bomb"
                );
            }
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
/// Redirect policy for the MaxMind client (#101).
///
/// Redirects cannot simply be refused: the download endpoint **always**
/// redirects. Measured 2026-07-18 — `download.maxmind.com` answers 302 with a
/// pre-signed URL on a Cloudflare R2 bucket host, and that URL needs no
/// credentials (it returned 206 when fetched with none). So `Policy::none()`
/// would break every fetch, and pinning the host would too, since the target
/// is a different origin whose name embeds a bucket identifier.
///
/// What a policy *can* assert is bounded hops and no scheme downgrade:
///
/// - **Hop limit.** One hop is observed; `MAX_REDIRECTS` leaves headroom
///   without being unbounded.
/// - **No downgrade.** A redirect from `https` to `http` is refused. This is
///   narrower than "targets must be https" deliberately: the rule that matters
///   is that a secure request is never silently downgraded, and stating it that
///   way also keeps the behaviour testable over plain HTTP.
///
/// What a policy **cannot** assert is that credentials are not forwarded
/// across origins — `Policy` only decides follow-or-stop and cannot inspect or
/// modify headers. `reqwest` strips `Authorization` cross-origin, and since the
/// R2 hop is cross-origin that stripping is load-bearing on *every* fetch. It
/// is asserted by test instead: see
/// `credentials_are_not_forwarded_across_origin_redirect`.
fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            return attempt
                .error(format!("exceeded {MAX_REDIRECTS} redirects"));
        }
        let downgrades = attempt.url().scheme() == "http"
            && attempt.previous().iter().any(|u| u.scheme() == "https");
        if downgrades {
            return attempt
                .error("refusing redirect that downgrades https to http");
        }
        attempt.follow()
    })
}

fn send_with_retry<F>(f: F) -> Result<reqwest::blocking::Response>
where
    F: Fn() -> reqwest::Result<reqwest::blocking::Response>,
{
    let mut attempt = 0u32;
    loop {
        match f() {
            Err(e)
                if attempt < MAX_RETRIES
                    && (e.is_timeout() || e.is_connect()) =>
            {
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
            Ok(resp)
                if resp.status().is_server_error() && attempt < MAX_RETRIES =>
            {
                let delay = BASE_DELAY_SECS * 2u64.pow(attempt);
                messages::warn(&format!(
                    "Server error {} (attempt {}/{MAX_RETRIES}). Retrying in \
                     {delay}s...",
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
            format!("Failed to read checksum file {}", checksum_path.display())
        })?;
    let expected_hash =
        checksum_text.split_whitespace().next().ok_or_else(|| {
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
    if filename.is_empty() {
        None
    } else {
        Some(filename)
    }
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
    let mut rdr = ReaderBuilder::new()
        .from_path(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let headers = rdr
        .headers()
        .with_context(|| {
            format!("Failed to read headers from {}", path.display())
        })?
        .clone();
    for col in ["geoname_id", "country_iso_code", "continent_code"] {
        if !headers.iter().any(|h| h == col) {
            bail!("Missing required column {:?} in {}", col, path.display());
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
            bail!("geoname_id {:?} is not numeric in {}", val, path.display());
        }
    }
    Ok(())
}

fn validate_blocks_csv(path: &Path) -> Result<()> {
    let mut rdr = ReaderBuilder::new()
        .from_path(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
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
            bail!("Missing required column {:?} in {}", col, path.display());
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
                && val != "0"
                && val != "1"
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

#[cfg(test)]
mod tests {
    // ── mock HTTP server (#88)
    // ───────────────────────────────────────────────
    //
    // The network path was previously untestable in practice. It needs no
    // production seam: `fetch()` takes its URL from `config.maxmind.url`
    // and enforces no scheme, so pointing that at a local listener
    // drives the real code — `resolve_version`, `check_download_size`,
    // `acquire_remote_archive`, `send_with_retry` — with nothing stubbed.
    //
    // Hand-rolled rather than a dev-dependency: the request shapes are
    // trivial (GET, no body) and this project is deliberately
    // conservative about dependency surface. Responses close the
    // connection, so no keep-alive handling is needed.
    use std::{
        io::BufRead,
        net::{TcpListener, TcpStream},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use zip::write::{SimpleFileOptions, ZipWriter};

    use super::*;

    #[derive(Clone, Debug)]
    struct MockRequest {
        line: String,
        headers: Vec<(String, String)>,
    }

    impl MockRequest {
        /// Header lookup by lowercased name.
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        }

        /// The request target, e.g. `/?suffix=zip`.
        fn target(&self) -> &str {
            self.line.split_whitespace().nth(1).unwrap_or("")
        }
    }

    struct MockReply {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl MockReply {
        fn new(status: u16) -> Self {
            Self {
                status,
                headers: Vec::new(),
                body: Vec::new(),
            }
        }

        fn ok(body: impl Into<Vec<u8>>) -> Self {
            Self {
                status: 200,
                headers: Vec::new(),
                body: body.into(),
            }
        }

        fn header(mut self, k: &str, v: &str) -> Self {
            self.headers.push((k.to_string(), v.to_string()));
            self
        }
    }

    struct MockServer {
        port: u16,
        seen: Arc<Mutex<Vec<MockRequest>>>,
        stop: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl MockServer {
        fn start<F>(router: F) -> Self
        where
            F: Fn(&MockRequest) -> MockReply + Send + 'static,
        {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = listener.local_addr().expect("addr").port();
            listener.set_nonblocking(true).expect("nonblocking");

            let seen = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (seen_t, stop_t) = (Arc::clone(&seen), Arc::clone(&stop));

            let handle = thread::spawn(move || {
                while !stop_t.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if let Some(req) = read_request(&stream) {
                                seen_t.lock().unwrap().push(req.clone());
                                let _ = write_reply(stream, router(&req));
                            }
                        }
                        // Nonblocking accept with a short poll: no hang if the
                        // client makes fewer requests than expected.
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });

            Self {
                port,
                seen,
                stop,
                handle: Some(handle),
            }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }

        fn requests(&self) -> Vec<MockRequest> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    fn read_request(stream: &TcpStream) -> Option<MockRequest> {
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let mut reader = io::BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let request_line = line.trim_end().to_string();
        if request_line.is_empty() {
            return None;
        }

        let mut headers = Vec::new();
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).ok()? == 0 {
                break;
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                headers.push((
                    k.trim().to_ascii_lowercase(),
                    v.trim().to_string(),
                ));
            }
        }

        Some(MockRequest {
            line: request_line,
            headers,
        })
    }

    fn write_reply(mut stream: TcpStream, reply: MockReply) -> io::Result<()> {
        let mut head = format!(
            "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
            reply.status,
            reply.body.len()
        );
        for (k, v) in &reply.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes())?;
        stream.write_all(&reply.body)?;
        stream.flush()
    }

    /// A `Config` whose MaxMind URL points at `url` and whose archive dir is
    /// `archive_dir`. Credentials are dummies that pass the not-configured
    /// check in `fetch()`.
    fn mock_config(url: &str, archive_dir: &Path) -> crate::config::Config {
        crate::config::Config {
            paths: crate::config::Paths {
                archive_dir: archive_dir.display().to_string(),
                archive_prune: 3,
                output_dir: archive_dir.display().to_string(),
            },
            maxmind: crate::config::MaxMind {
                url: url.to_string(),
                account_id: "123456".to_string(),
                license_key: "test-license-key".to_string(),
            },
            logging: None,
            processing: None,
        }
    }

    /// Reply carrying a valid-looking archive filename, so `resolve_version`
    /// succeeds and the flow reaches the download.
    fn versioned_reply(body: &[u8]) -> MockReply {
        MockReply::ok(body.to_vec()).header(
            "Content-Disposition",
            "attachment; filename=\"GeoLite2-Country-CSV_20260101.zip\"",
        )
    }

    /// Credentials must reach MaxMind as HTTP basic auth, and nowhere else.
    #[test]
    fn remote_fetch_sends_basic_auth() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start(|_| MockReply::new(401));
        let cfg = mock_config(&server.url(), dir.path());

        let _ = fetch(&cfg, FetchMode::Remote);

        let reqs = server.requests();
        assert!(!reqs.is_empty(), "server saw no request");
        let auth = reqs[0]
            .header("authorization")
            .expect("no Authorization header sent");
        assert!(
            auth.starts_with("Basic "),
            "expected HTTP basic auth, got {auth:?}"
        );
        assert!(
            reqs[0].target().contains("suffix=zip"),
            "unexpected target {:?}",
            reqs[0].target()
        );
    }

    /// A non-success status must abort with the status reported, not proceed.
    #[test]
    fn non_success_status_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start(|_| MockReply::new(401));
        let cfg = mock_config(&server.url(), dir.path());

        let err = fetch(&cfg, FetchMode::Remote).expect_err("must fail");
        assert!(
            err.to_string().contains("401"),
            "error should name the status: {err}"
        );
    }

    /// 429 is a client error, so `send_with_retry` must NOT retry it —
    /// hammering a rate limit is exactly the wrong response, and MaxMind's cap
    /// is the real constraint on this project's test runs.
    #[test]
    fn rate_limit_is_not_retried() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start(|_| MockReply::new(429));
        let cfg = mock_config(&server.url(), dir.path());

        let _ = fetch(&cfg, FetchMode::Remote);
        assert_eq!(
            server.requests().len(),
            1,
            "a rate-limit response must not be retried"
        );
    }

    /// `resolve_version` reads the version from `Content-Disposition`; without
    /// it there is no version, and proceeding would mis-name the archive.
    #[test]
    fn missing_content_disposition_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start(|_| MockReply::ok("body"));
        let cfg = mock_config(&server.url(), dir.path());

        let err = fetch(&cfg, FetchMode::Remote).expect_err("must fail");
        assert!(
            err.to_string().contains("Content-Disposition"),
            "unhelpful error: {err}"
        );
    }

    /// The header is attacker-influenced and names a file on disk. These are
    /// the traversal shapes the guardian audit reasoned about statically; here
    /// they are executed.
    #[test]
    fn hostile_content_disposition_is_rejected() {
        for hostile in [
            "attachment; filename=\"../../etc/passwd\"",
            "attachment; filename=\"/etc/shadow\"",
            "attachment; filename=\"..\"",
            "attachment; filename=\"\"",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let h = hostile.to_string();
            let server = MockServer::start(move |_| {
                MockReply::ok("x").header("Content-Disposition", &h)
            });
            let cfg = mock_config(&server.url(), dir.path());

            assert!(
                fetch(&cfg, FetchMode::Remote).is_err(),
                "accepted hostile Content-Disposition {hostile:?}"
            );
            // Nothing may be written outside the archive dir, and nothing
            // resembling a traversal target inside it.
            assert!(
                !Path::new("/tmp/passwd").exists(),
                "traversal escaped the archive dir"
            );
        }
    }

    /// End-to-end proof of the `PartialDownload` guard: a checksum mismatch
    /// must fail *and* leave no `.part` file behind.
    #[test]
    fn checksum_mismatch_leaves_no_partial_download() {
        let dir = tempfile::tempdir().unwrap();
        let server = MockServer::start(|req| {
            if req.target().contains("sha256") {
                // Deliberately not the hash of the body below.
                MockReply::ok(
                    "0000000000000000000000000000000000000000000000000000000000000000  x.zip",
                )
            } else {
                versioned_reply(b"not-a-real-archive")
            }
        });
        let cfg = mock_config(&server.url(), dir.path());

        let err = fetch(&cfg, FetchMode::Remote).expect_err("must fail");
        assert!(
            err.to_string().contains("Checksum verification failed"),
            "unexpected error: {err}"
        );

        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".part"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "partial download left behind: {leftovers:?}"
        );
    }

    /// The property `redirect_policy` cannot express, and the reason the R2 hop
    /// is safe (#101). `reqwest` strips `Authorization` cross-origin; since
    /// MaxMind redirects to a different origin on *every* fetch, that stripping
    /// is load-bearing continuously. Two servers are two origins.
    #[test]
    fn credentials_are_not_forwarded_across_origin_redirect() {
        let dir = tempfile::tempdir().unwrap();
        let target = MockServer::start(|_| MockReply::new(401));
        let target_url = target.url();
        let origin = MockServer::start(move |_| {
            MockReply::new(302).header("Location", &target_url)
        });
        let cfg = mock_config(&origin.url(), dir.path());

        let _ = fetch(&cfg, FetchMode::Remote);

        let followed = target.requests();
        assert_eq!(followed.len(), 1, "redirect was not followed");
        assert!(
            followed[0].header("authorization").is_none(),
            "Authorization was forwarded across origins — the license key \
             would leak to the redirect target"
        );
        // And it *was* sent to the intended origin.
        assert!(
            origin.requests()[0].header("authorization").is_some(),
            "credentials never reached the configured endpoint"
        );
    }

    /// The hop limit `redirect_policy` does express. The server redirects to
    /// itself, so without a bound this would never terminate; the location is
    /// shared so the router can name a URL that does not exist until after the
    /// server has bound its port.
    #[test]
    fn redirect_loop_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let location = Arc::new(Mutex::new(String::new()));
        let for_router = Arc::clone(&location);

        let server = MockServer::start(move |_| {
            let to = for_router.lock().unwrap().clone();
            MockReply::new(302).header("Location", &to)
        });
        *location.lock().unwrap() = server.url();

        let cfg = mock_config(&server.url(), dir.path());
        assert!(
            fetch(&cfg, FetchMode::Remote).is_err(),
            "an unbounded redirect chain must fail rather than loop"
        );
        assert!(
            server.requests().len() <= MAX_REDIRECTS + 1,
            "followed more than {MAX_REDIRECTS} redirects: {} requests",
            server.requests().len()
        );
    }

    // ── partial-download cleanup ─────────────────────────────────────────────

    /// The default: any early return removes the `.part` file. Six error
    /// paths in `acquire_remote_archive` previously leaked it, and because
    /// `prune_csv_archives` matches only `.zip`/`.zip.sha256`, leaked files
    /// were never reclaimed.
    #[test]
    fn partial_download_is_removed_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("archive.zip.part");
        fs::write(&part, b"half a download").unwrap();

        drop(PartialDownload::new(&part));
        assert!(!part.exists(), "armed guard must delete the .part file");
    }

    /// After a successful rename the file has moved; the guard must not chase
    /// it (and must not delete anything at the old path).
    #[test]
    fn disarmed_guard_keeps_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("archive.zip.part");
        fs::write(&part, b"complete").unwrap();

        let mut guard = PartialDownload::new(&part);
        guard.disarm();
        drop(guard);

        assert!(part.exists(), "disarmed guard must not delete");
    }

    /// A guard whose file never got created — e.g. `File::create` failed —
    /// must drop silently rather than warn about a missing file.
    #[test]
    fn missing_partial_download_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("never-created.zip.part");
        assert!(!part.exists());

        drop(PartialDownload::new(&part));
        assert!(!part.exists());
    }

    // ── zip fixtures ─────────────────────────────────────────────────────────

    /// One entry to place in a test zip.
    struct E {
        name: &'static str,
        size: usize,
        exec: bool,
    }

    fn clean(name: &'static str, size: usize) -> E {
        E {
            name,
            size,
            exec: false,
        }
    }

    /// Build a zip at `path` from `entries`. Names are written verbatim — the
    /// writer does not normalise `..` for plain `start_file` — so this can
    /// craft the malicious entries the security scanner must reject.
    fn write_zip(path: &Path, entries: &[E]) {
        let file = File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        for e in entries {
            let mut opts = SimpleFileOptions::default();
            if e.exec {
                opts = opts.unix_permissions(0o755);
            }
            zip.start_file(e.name, opts).unwrap();
            zip.write_all(&vec![b'x'; e.size]).unwrap();
        }
        zip.finish().unwrap();
    }

    fn open_zip(path: &Path) -> ZipArchive<File> {
        ZipArchive::new(File::open(path).unwrap()).unwrap()
    }

    // ── parse_content_disposition_filename ───────────────────────────────────

    #[test]
    fn cd_unquoted_filename() {
        assert_eq!(
            parse_content_disposition_filename(
                "attachment; filename=GeoLite2-Country-CSV_20260227.zip"
            ),
            Some("GeoLite2-Country-CSV_20260227.zip")
        );
    }

    #[test]
    fn cd_quoted_filename() {
        assert_eq!(
            parse_content_disposition_filename(
                "attachment; filename=\"GeoLite2-Country-CSV_20260227.zip\""
            ),
            Some("GeoLite2-Country-CSV_20260227.zip")
        );
    }

    #[test]
    fn cd_case_insensitive_key() {
        assert_eq!(
            parse_content_disposition_filename("attachment; FileName=x.zip"),
            Some("x.zip")
        );
    }

    #[test]
    fn cd_missing_filename_is_none() {
        assert_eq!(parse_content_disposition_filename("attachment"), None);
    }

    #[test]
    fn cd_empty_filename_is_none() {
        assert_eq!(
            parse_content_disposition_filename("attachment; filename="),
            None
        );
        assert_eq!(
            parse_content_disposition_filename("attachment; filename=\"\""),
            None
        );
    }

    // ── find_latest_local_csv_archive ────────────────────────────────────────

    #[test]
    fn find_latest_picks_highest_version() {
        let dir = TempDir::new().unwrap();
        for date in ["20260101", "20260315", "20260227"] {
            fs::write(
                dir.path().join(format!("GeoLite2-Country-CSV_{date}.zip")),
                b"",
            )
            .unwrap();
        }
        let (path, version) =
            find_latest_local_csv_archive(dir.path()).unwrap();
        assert_eq!(version.as_str(), "20260315");
        assert!(path.ends_with("GeoLite2-Country-CSV_20260315.zip"));
    }

    #[test]
    fn find_latest_skips_nonmatching_names() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("GeoLite2-Country-CSV_20260101.zip"), b"")
            .unwrap();
        // wrong product, checksum sidecar, and unrelated file — all ignored
        fs::write(dir.path().join("GeoLite2-City-CSV_20260901.zip"), b"")
            .unwrap();
        fs::write(
            dir.path().join("GeoLite2-Country-CSV_20260101.zip.sha256"),
            b"",
        )
        .unwrap();
        fs::write(dir.path().join("notes.txt"), b"").unwrap();
        let (_, version) = find_latest_local_csv_archive(dir.path()).unwrap();
        assert_eq!(version.as_str(), "20260101");
    }

    #[test]
    fn find_latest_errors_when_empty() {
        let dir = TempDir::new().unwrap();
        assert!(find_latest_local_csv_archive(dir.path()).is_err());
    }

    // ── verify_zip_magic ─────────────────────────────────────────────────────

    #[test]
    fn zip_magic_accepts_real_zip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("real.zip");
        write_zip(&path, &[clean("a.csv", 4)]);
        assert!(verify_zip_magic(&path).is_ok());
    }

    #[test]
    fn zip_magic_rejects_non_zip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("fake.zip");
        fs::write(&path, b"not a zip at all").unwrap();
        assert!(verify_zip_magic(&path).is_err());
    }

    // ── scan_zip_entries (security scanner) ──────────────────────────────────

    #[test]
    fn scan_rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("evil.zip");
        write_zip(&path, &[clean("../escape.txt", 4)]);
        let err = scan_zip_entries(&mut open_zip(&path)).unwrap_err();
        assert!(err.to_string().contains("traversal"), "{err}");
    }

    #[test]
    fn scan_rejects_absolute_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("abs.zip");
        // drive-letter form triggers the `:/` branch and survives the writer
        // verbatim (a leading `/` can be stripped by some tooling).
        write_zip(&path, &[clean("C:/evil.txt", 4)]);
        let err = scan_zip_entries(&mut open_zip(&path)).unwrap_err();
        assert!(err.to_string().contains("absolute"), "{err}");
    }

    #[test]
    fn scan_rejects_executable_bits() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("exec.zip");
        write_zip(
            &path,
            &[E {
                name: "run.sh",
                size: 4,
                exec: true,
            }],
        );
        let err = scan_zip_entries(&mut open_zip(&path)).unwrap_err();
        assert!(err.to_string().contains("executable"), "{err}");
    }

    #[test]
    fn scan_detects_common_prefix() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested.zip");
        write_zip(
            &path,
            &[clean("GeoLite2/a.csv", 4), clean("GeoLite2/b.csv", 4)],
        );
        assert_eq!(
            scan_zip_entries(&mut open_zip(&path)).unwrap(),
            Some("GeoLite2".to_string())
        );
    }

    #[test]
    fn scan_flat_archive_has_no_prefix() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("flat.zip");
        write_zip(&path, &[clean("a.csv", 4), clean("b.csv", 4)]);
        assert_eq!(scan_zip_entries(&mut open_zip(&path)).unwrap(), None);
    }

    // ── extract_archive_to_temp_capped ───────────────────────────────────────

    #[test]
    fn extract_within_budget_succeeds() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ok.zip");
        write_zip(&path, &[clean("data.csv", 1_000)]);
        let out = extract_archive_to_temp_capped(&path, 10_000)
            .expect("extraction within budget should succeed");
        assert!(out.path().join("data.csv").exists());
    }

    #[test]
    fn extract_exceeding_budget_bails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bomb.zip");
        write_zip(&path, &[clean("data.csv", 1_000)]);
        let err = extract_archive_to_temp_capped(&path, 100)
            .expect_err("extraction past the budget must be refused");
        assert!(err.to_string().contains("decompression bomb"), "{err}");
    }

    #[test]
    fn extract_strips_common_prefix() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested.zip");
        write_zip(&path, &[clean("GeoLite2/data.csv", 20)]);
        let out = extract_archive_to_temp_capped(&path, 10_000).unwrap();
        assert!(out.path().join("data.csv").exists());
        assert!(!out.path().join("GeoLite2").exists());
    }

    #[test]
    fn extract_rejects_traversal() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("evil.zip");
        write_zip(&path, &[clean("../escape.txt", 4)]);
        assert!(extract_archive_to_temp_capped(&path, 10_000).is_err());
    }

    // ── verify_cached_archive ────────────────────────────────────────────────

    #[test]
    fn cached_archive_matching_checksum_is_true() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("a.zip");
        let checksum = dir.path().join("a.zip.sha256");
        fs::write(&archive, b"payload").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"payload"));
        fs::write(&checksum, format!("{hash}  a.zip\n")).unwrap();
        assert!(verify_cached_archive(&archive, &checksum).unwrap());
    }

    #[test]
    fn cached_archive_mismatch_is_false() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("a.zip");
        let checksum = dir.path().join("a.zip.sha256");
        fs::write(&archive, b"payload").unwrap();
        fs::write(&checksum, format!("{}  a.zip\n", "0".repeat(64))).unwrap();
        assert!(!verify_cached_archive(&archive, &checksum).unwrap());
    }

    #[test]
    fn cached_archive_bad_checksum_format_errors() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("a.zip");
        let checksum = dir.path().join("a.zip.sha256");
        fs::write(&archive, b"payload").unwrap();
        fs::write(&checksum, b"").unwrap();
        assert!(verify_cached_archive(&archive, &checksum).is_err());
    }

    // ── CSV validation ───────────────────────────────────────────────────────

    #[test]
    fn locations_csv_valid_ok() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("loc.csv");
        fs::write(
            &path,
            "geoname_id,country_iso_code,continent_code\n6252001,US,NA\n",
        )
        .unwrap();
        assert!(validate_locations_csv(&path).is_ok());
    }

    #[test]
    fn locations_csv_missing_column_bails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("loc.csv");
        fs::write(&path, "geoname_id,country_iso_code\n6252001,US\n").unwrap();
        assert!(validate_locations_csv(&path).is_err());
    }

    #[test]
    fn blocks_csv_valid_ok() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("blk.csv");
        fs::write(
            &path,
            "network,geoname_id,is_anonymous_proxy,is_satellite_provider\n1.0.\
             0.0/24,6252001,0,0\n",
        )
        .unwrap();
        assert!(validate_blocks_csv(&path).is_ok());
    }

    #[test]
    fn blocks_csv_missing_column_bails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("blk.csv");
        fs::write(
            &path,
            "network,geoname_id,is_anonymous_proxy\n1.0.0.0/24,6252001,0\n",
        )
        .unwrap();
        assert!(validate_blocks_csv(&path).is_err());
    }
}
