/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process, thread,
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

/// `account_id`/`license_key` are already-decrypted plaintext, supplied by
/// the caller — decryption (with its interactive passphrase prompt) is
/// deliberately not this function's job. See `action.rs`, which calls
/// `secrets::decrypt` before this function for `FetchMode::Remote`, and
/// passes empty strings for `FetchMode::Local` (never read below: the
/// early return above happens before either is touched). Keeping this
/// signature plaintext-in means `fetch.rs`'s existing mock-HTTP unit test
/// suite is unaffected by #103 — it already constructs these as plain
/// strings and runs under `cargo test`, with no terminal available for a
/// passphrase prompt.
pub fn fetch(
    config: &Config,
    mode: FetchMode,
    account_id: &str,
    license_key: &str,
) -> Result<(TempDir, Version)> {
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

    if account_id.is_empty() || license_key.is_empty() {
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

/// The temporary path an in-progress download is written to, before it is
/// renamed over `archive_path`.
///
/// The PID is in the name deliberately (#100, guardian F-1). This used to be
/// `archive_path.with_extension("zip.part")` — derived from the version alone,
/// so two `xtgeoip fetch` processes resolving the same version shared a single
/// temp file. Their `io::copy` writes could interleave, and either process's
/// [`PartialDownload`] guard could remove a path the other was still writing.
///
/// A PID is exactly the right amount of uniqueness for that: the failure needs
/// two processes running *at once*, and two live processes cannot share one.
/// It is not a general-purpose unique name — a PID can repeat after the
/// original exits, and separate PID namespaces sharing one `archive_dir` could
/// collide — but neither case is the concurrent-writer problem this addresses,
/// and both are still caught downstream by SHA-256 verification.
///
/// `NamedTempFile` would give unconditional uniqueness, at the cost of
/// replacing `PartialDownload` and its six tests inside guardian-signed code,
/// for a LOW finding that already fails closed. Not worth the audit surface.
fn part_path(archive_path: &Path) -> PathBuf {
    archive_path.with_extension(format!("zip.{}.part", process::id()))
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
    // Download to a per-process .part file; rename atomically on success.
    let tmp_path = part_path(archive_path);
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
mod tests;
