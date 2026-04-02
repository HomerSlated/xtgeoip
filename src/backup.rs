/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    collections::BTreeMap,
    fs::{self},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use flate2::{Compression, write::GzEncoder};
use glob::glob;
use log::info;
use sha2::{Digest, Sha256};
use tar::Builder;

use crate::config::Config;

const VERSION_FILE: &str = "version";

fn version_path(data_dir: &Path) -> PathBuf {
    data_dir.join(VERSION_FILE)
}

fn manifest_path_for_version(data_dir: &Path, version: &str) -> PathBuf {
    data_dir.join(format!("GeoLite2-Country-bin_{}.sha256", version))
}

/// Collect IV files: 2-char country codes, with .iv4 or .iv6, including digits
/// and letters.
fn iv_files(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> =
        glob(&format!("{}/*[0-9A-Z][0-9A-Z].iv[46]", data_dir.display()))?
            .filter_map(Result::ok)
            .collect();
    files.sort();
    Ok(files)
}

/// Collect all .sha256 files in the data directory.
fn all_sha256_files(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> =
        glob(&format!("{}/*.sha256", data_dir.display()))?
            .filter_map(Result::ok)
            .collect();
    files.sort();
    Ok(files)
}

/// Verify manifest checksums.
fn verify_manifest_files(
    data_dir: &Path,
    manifest_path: &Path,
) -> Result<Vec<PathBuf>> {
    let manifest = fs::read_to_string(manifest_path)?;
    let mut verified_files = Vec::new();

    for (idx, line) in manifest.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (expected_hash, file_name) = line
            .split_once("  ")
            .or_else(|| line.split_once(' '))
            .ok_or_else(|| anyhow!("Malformed manifest at line {}", idx + 1))?;

        let file_name = file_name.trim();
        if file_name.is_empty() {
            bail!("Malformed manifest at line {}", idx + 1);
        }

        let file_path = data_dir.join(file_name);
        if !file_path.exists() {
            bail!(
                "Manifest mismatch\n{}: file not found\nOperation aborted, no \
                 files have been modified",
                file_name
            );
        }

        let data = fs::read(&file_path)?;
        let actual_hash = format!("{:x}", Sha256::digest(&data));
        if actual_hash != expected_hash {
            bail!(
                "Manifest mismatch\n{}: checksum failed\nOperation aborted, \
                 no files have been modified",
                file_name
            );
        }

        verified_files.push(file_path);
    }

    Ok(verified_files)
}

/// Create tar.gz archive from a list of files.
fn create_tarball(output_path: &Path, files: &[PathBuf]) -> Result<()> {
    let tar_gz = fs::File::create(output_path)?;
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = Builder::new(enc);

    for file in files {
        if file.exists() {
            let name = file.file_name().ok_or_else(|| {
                anyhow!("Invalid file path {}", file.display())
            })?;
            tar.append_path_with_name(file, name)?;
        }
    }

    tar.finish()?;
    Ok(())
}

/// Helper: collect files for backup or delete.
/// Returns (files_to_process, version, optional_manifest_path)
fn gather_files(
    data_dir: &Path,
    force: bool,
) -> Result<(Vec<PathBuf>, String, Option<PathBuf>)> {
    // Try reading the version file
    let version_result = fs::read_to_string(version_path(data_dir));
    let version = version_result
        .as_ref()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown_version".to_string());

    let manifest_path = manifest_path_for_version(data_dir, &version);

    if force {
        // Include all files without verifying version/manifest
        let mut files = iv_files(data_dir)?;
        let version_file = version_path(data_dir);
        if version_file.exists() {
            files.push(version_file);
        }
        files.extend(all_sha256_files(data_dir)?);
        return Ok((files, version, Some(manifest_path)));
    }

    // Non-force: optional manifest if it exists
    let manifest_opt = if manifest_path.exists() {
        Some(manifest_path.clone())
    } else {
        None
    };

    // If version file is missing, propagate error to caller
    if version_result.is_err() {
        return Err(anyhow!(
            "Version file missing: {}. Use -f to force operation",
            version_path(data_dir).display()
        ));
    }

    // At this point, we have a valid version and possibly a manifest
    Ok((Vec::new(), version, manifest_opt))
}

/// Backup IV files, version file, and manifest. Force option allows backup even
/// if version/manifest missing.
pub fn backup(data_dir: &Path, backup_dir: &Path, force: bool) -> Result<()> {
    // Ensure backup directory exists
    fs::create_dir_all(backup_dir)
        .with_context(|| format!("Failed to create backup directory {}", backup_dir.display()))?;

    // Gather files (will return error if version missing and not forced)
    let (mut files, version, manifest_opt) = gather_files(data_dir, force)?;

    if force {
        if files.is_empty() {
            bail!("Nothing to back up");
        }

        let output_path = backup_dir.join(format!(
            "GeoLite2-Country-bin_unverified_{}.tar.gz",
            version
        ));
        create_tarball(&output_path, &files)?;
        info!("Backed up unverified binary data to {}", output_path.display());
        return Ok(());
    }

    // Non-force: verify manifest
    let manifest_path = manifest_opt.ok_or_else(|| {
        anyhow!(
            "Manifest missing: {}\nExpected manifest not found. Use -f to force backup",
            manifest_path_for_version(data_dir, &version).display()
        )
    })?;

    // Verify checksums and include version + manifest files
    files = verify_manifest_files(data_dir, &manifest_path)?;
    files.push(version_path(data_dir));
    files.push(manifest_path.clone());

    let output_path = backup_dir.join(format!("GeoLite2-Country-bin_{}.tar.gz", version));
    create_tarball(&output_path, &files)?;
    info!("Backed up binary data to {}", output_path.display());

    Ok(())
}

/// Delete IV files, version file, and manifest. Force option allows deletion
/// even if version/manifest missing.
pub fn delete(data_dir: &Path, force: bool) -> Result<()> {
    let (mut files, version, _manifest_opt) = gather_files(data_dir, force)?;

    if force {
        if files.is_empty() {
            bail!("Nothing to delete");
        }
        for file in files {
            fs::remove_file(file)?;
        }

        // Remove orphaned file if it exists
        let orphan_path = data_dir.join("orphaned");
        if orphan_path.exists() {
            fs::remove_file(orphan_path)?;
        }

        info!(
            "Force deleted binary data files from {}",
            data_dir.display()
        );
        return Ok(());
    }

    // Non-force delete only deletes verified manifest files + version
    let manifest_path = _manifest_opt.ok_or_else(|| {
        anyhow!(
            "Manifest missing: {}\nUse -f to force delete",
            manifest_path_for_version(data_dir, &version).display()
        )
    })?;

    files = verify_manifest_files(data_dir, &manifest_path)?;
    for file in files {
        fs::remove_file(file)?;
    }
    fs::remove_file(version_path(data_dir))?;
    fs::remove_file(manifest_path)?;

    info!("Deleted old binary data files from {}", data_dir.display());
    Ok(())
}

/// Summary of pruning operations.
struct PruneSummary {
    csv_removed: usize,
    bin_removed: usize,
}

/// Prune old GeoLite2 archives according to the config.
///
/// - `prune_csv` => operate on GeoLite2-Country-CSV_* archives (+ .sha256)
/// - `prune_bin` => operate on GeoLite2-Country-bin_* archives (verified +
///   unverified)
///
/// Behaviour:
/// - Fails hard if the archive directory is missing or invalid.
/// - Fails if `paths.archive_prune < 1`.
/// - Prints a concise summary on success.
/// - Prints a partial summary before failing on error.
pub fn prune_archives(
    cfg: &Config,
    prune_csv: bool,
    prune_bin: bool,
) -> Result<()> {
    let archive_dir = Path::new(&cfg.paths.archive_dir);
    let keep = cfg.paths.archive_prune;

    if keep < 1 {
        bail!("Invalid configuration: paths.archive_prune must be >= 1");
    }

    if !archive_dir.exists() || !archive_dir.is_dir() {
        bail!(
            "Archive directory {} is missing or invalid",
            archive_dir.display()
        );
    }

    let mut summary = PruneSummary {
        csv_removed: 0,
        bin_removed: 0,
    };

    if prune_csv {
        match prune_csv_archives(archive_dir, keep) {
            Ok(count) => summary.csv_removed = count,
            Err(e) => {
                print_prune_summary(&summary);
                return Err(e);
            }
        }
    }

    if prune_bin {
        match prune_bin_archives(archive_dir, keep) {
            Ok(count) => summary.bin_removed = count,
            Err(e) => {
                print_prune_summary(&summary);
                return Err(e);
            }
        }
    }

    print_prune_summary(&summary);
    Ok(())
}

fn print_prune_summary(summary: &PruneSummary) {
    match (summary.csv_removed, summary.bin_removed) {
        (0, 0) => info!("No archives needed pruning."),
        (c, 0) => info!("Pruned {c} old CSV archives."),
        (0, b) => info!("Pruned {b} old bin archives."),
        (c, b) => {
            info!("Pruned {c} old CSV archives and {b} old bin archives.")
        }
    }
}

fn prune_csv_archives(dir: &Path, keep: usize) -> Result<usize> {
    let entries = fs::read_dir(dir).with_context(|| {
        format!("Cannot read archive directory {}", dir.display())
    })?;

    // Map: version -> Vec<PathBuf> (zip + sha256)
    let mut by_version: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if !name.starts_with("GeoLite2-Country-CSV_") {
            continue;
        }

        if !(name.ends_with(".zip") || name.ends_with(".zip.sha256")) {
            continue;
        }

        if let Some(ver) = extract_version(name) {
            by_version.entry(ver).or_default().push(path.clone());
        }
    }

    if by_version.is_empty() {
        return Ok(0);
    }

    let total_versions = by_version.len();
    if total_versions <= keep {
        return Ok(0);
    }

    let mut removed = 0;
    let to_remove = total_versions - keep;

    for (_, files) in by_version.into_iter().take(to_remove) {
        for path in files {
            if path.exists() {
                fs::remove_file(&path).with_context(|| {
                    format!(
                        "Failed to remove CSV archive file {}",
                        path.display()
                    )
                })?;
                removed += 1;
            }
        }
    }

    Ok(removed)
}

fn prune_bin_archives(dir: &Path, keep: usize) -> Result<usize> {
    let entries = fs::read_dir(dir).with_context(|| {
        format!("Cannot read archive directory {}", dir.display())
    })?;

    // Map: version -> Vec<PathBuf> (verified + unverified)
    let mut by_version: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if !name.starts_with("GeoLite2-Country-bin_") {
            continue;
        }

        if !name.ends_with(".tar.gz") {
            continue;
        }

        // Ignore the unique unknown-version file
        if name.contains("unknown_version") {
            continue;
        }

        if let Some(ver) = extract_version(name) {
            by_version.entry(ver).or_default().push(path.clone());
        }
    }

    if by_version.is_empty() {
        return Ok(0);
    }

    let total_versions = by_version.len();
    if total_versions <= keep {
        return Ok(0);
    }

    let mut removed = 0;
    let to_remove = total_versions - keep;

    for (_, files) in by_version.into_iter().take(to_remove) {
        for path in files {
            if path.exists() {
                fs::remove_file(&path).with_context(|| {
                    format!(
                        "Failed to remove bin archive file {}",
                        path.display()
                    )
                })?;
                removed += 1;
            }
        }
    }

    Ok(removed)
}

/// Extract the version component from a GeoLite2 archive filename.
///
/// Examples:
/// - GeoLite2-Country-CSV_20260324.zip
/// - GeoLite2-Country-CSV_20260324.zip.sha256
/// - GeoLite2-Country-bin_20260324.tar.gz
/// - GeoLite2-Country-bin_unverified_20260324.tar.gz
fn extract_version(name: &str) -> Option<String> {
    let idx = name.rfind('_')?;
    let part = &name[idx + 1..];
    let digits: String =
        part.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() == 8 {
        Some(digits)
    } else {
        None
    }
}
