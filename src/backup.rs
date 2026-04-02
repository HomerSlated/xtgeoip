/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    collections::BTreeMap,
    fs::{self},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use flate2::{Compression, write::GzEncoder};
use glob::glob;
use sha2::{Digest, Sha256};
use tar::Builder;

use crate::{config::Config, messages::{info, warn, error}};

const VERSION_FILE: &str = "version";

fn version_path(data_dir: &Path) -> PathBuf {
    data_dir.join(VERSION_FILE)
}

fn manifest_path_for_version(data_dir: &Path, version: &str) -> PathBuf {
    data_dir.join(format!("GeoLite2-Country-bin_{}.sha256", version))
}

/// Collect IV files: 2-char country codes, with .iv4 or .iv6
fn iv_files(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> =
        glob(&format!("{}/*[0-9A-Z][0-9A-Z].iv[46]", data_dir.display()))?
            .filter_map(Result::ok)
            .collect();
    files.sort();
    Ok(files)
}

/// Collect all .sha256 files
fn all_sha256_files(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> =
        glob(&format!("{}/*.sha256", data_dir.display()))?
            .filter_map(Result::ok)
            .collect();
    files.sort();
    Ok(files)
}

/// Verify manifest checksums
fn verify_manifest_files(
    data_dir: &Path,
    manifest_path: &Path,
) -> Result<Vec<PathBuf>> {
    let manifest = fs::read_to_string(manifest_path)
        .map_err(|e| { error(&format!("Failed to read manifest: {}", e)); e })?;
    let mut verified_files = Vec::new();

    for (idx, line) in manifest.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() { continue; }

        let (expected_hash, file_name) = line
            .split_once("  ")
            .or_else(|| line.split_once(' '))
            .ok_or_else(|| {
                let msg = format!("Malformed manifest at line {}", idx + 1);
                error(&msg);
                anyhow!(msg)
            })?;

        let file_name = file_name.trim();
        if file_name.is_empty() {
            let msg = format!("Malformed manifest at line {}", idx + 1);
            error(&msg);
            return Err(anyhow!(msg));
        }

        let file_path = data_dir.join(file_name);
        if !file_path.exists() {
            let msg = format!(
                "Manifest mismatch: {} not found. Operation aborted.",
                file_name
            );
            error(&msg);
            return Err(anyhow!(msg));
        }

        let data = fs::read(&file_path)?;
        let actual_hash = format!("{:x}", Sha256::digest(&data));
        if actual_hash != expected_hash {
            let msg = format!(
                "Manifest mismatch: {} checksum failed. Operation aborted.",
                file_name
            );
            error(&msg);
            return Err(anyhow!(msg));
        }

        verified_files.push(file_path);
    }

    Ok(verified_files)
}

/// Create tar.gz archive from files
fn create_tarball(output_path: &Path, files: &[PathBuf]) -> Result<()> {
    let tar_gz = fs::File::create(output_path)?;
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = Builder::new(enc);

    for file in files {
        if file.exists() {
            let name = file.file_name()
                .ok_or_else(|| {
                    let msg = format!("Invalid file path {}", file.display());
                    error(&msg);
                    anyhow!(msg)
                })?;
            tar.append_path_with_name(file, name)?;
        }
    }

    tar.finish()?;
    Ok(())
}

/// Gather files for backup or delete
fn gather_files(data_dir: &Path, force: bool) -> Result<(Vec<PathBuf>, String, Option<PathBuf>)> {
    let version_result = fs::read_to_string(version_path(data_dir));
    let version = version_result
        .as_ref()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown_version".to_string());

    let manifest_path = manifest_path_for_version(data_dir, &version);

    if force {
        let mut files = iv_files(data_dir)?;
        let version_file = version_path(data_dir);
        if version_file.exists() { files.push(version_file); }
        files.extend(all_sha256_files(data_dir)?);
        return Ok((files, version, Some(manifest_path)));
    }

    // Non-force: manifest optional
    let manifest_opt = if manifest_path.exists() { Some(manifest_path.clone()) } else { None };

    if version_result.is_err() {
        error(&format!(
            "Version file missing: {}. Use -f to force operation",
            version_path(data_dir).display()
        ));
        return Err(anyhow!("Version file missing"));
    }

    Ok((Vec::new(), version, manifest_opt))
}

/// Backup binary data
pub fn backup(data_dir: &Path, backup_dir: &Path, force: bool) -> Result<()> {
    fs::create_dir_all(backup_dir).with_context(|| {
        let msg = format!("Failed to create backup directory {}", backup_dir.display());
        error(&msg);
        msg
    })?;

    let (mut files, version, manifest_opt) = gather_files(data_dir, force)?;

    if force {
        if files.is_empty() {
            error("Nothing to back up");
            return Err(anyhow!("Nothing to back up"));
        }
        let output_path = backup_dir.join(format!("GeoLite2-Country-bin_unverified_{}.tar.gz", version));
        create_tarball(&output_path, &files)?;
        info(&format!("Backed up unverified binary data to {}", output_path.display()));
        return Ok(());
    }

    let manifest_path = manifest_opt.ok_or_else(|| {
        let msg = format!(
            "Manifest missing: {}. Use -f to force backup",
            manifest_path_for_version(data_dir, &version).display()
        );
        error(&msg);
        anyhow!(msg)
    })?;

    files = verify_manifest_files(data_dir, &manifest_path)?;
    files.push(version_path(data_dir));
    files.push(manifest_path.clone());

    let output_path = backup_dir.join(format!("GeoLite2-Country-bin_{}.tar.gz", version));
    create_tarball(&output_path, &files)?;
    info(&format!("Backed up binary data to {}", output_path.display()));

    Ok(())
}

/// Delete binary data
pub fn delete(data_dir: &Path, force: bool) -> Result<()> {
    let (mut files, _version, _manifest_opt) = gather_files(data_dir, force)?;

    if force {
        if files.is_empty() {
            error("Nothing to delete");
            return Err(anyhow!("Nothing to delete"));
        }
        for file in &files { fs::remove_file(file)?; }

        let orphan_path = data_dir.join("orphaned");
        if orphan_path.exists() { fs::remove_file(orphan_path)?; }

        info(&format!("Force deleted binary data files from {}", data_dir.display()));
        return Ok(());
    }

    let manifest_path = _manifest_opt.ok_or_else(|| {
        let msg = format!(
            "Manifest missing: {}. Use -f to force delete",
            manifest_path_for_version(data_dir, &_version).display()
        );
        error(&msg);
        anyhow!(msg)
    })?;

    files = verify_manifest_files(data_dir, &manifest_path)?;
    for file in &files { fs::remove_file(file)?; }
    fs::remove_file(version_path(data_dir))?;
    fs::remove_file(manifest_path)?;

    info(&format!("Deleted old binary data files from {}", data_dir.display()));
    Ok(())
}

/// Prune old archives
pub fn prune_archives(cfg: &Config, prune_csv: bool, prune_bin: bool) -> Result<()> {
    let archive_dir = Path::new(&cfg.paths.archive_dir);
    let keep = cfg.paths.archive_prune;

    if keep < 1 {
        error("Invalid configuration: paths.archive_prune must be >= 1");
        return Err(anyhow!("Invalid configuration"));
    }

    if !archive_dir.exists() || !archive_dir.is_dir() {
        error(&format!("Archive directory {} is missing or invalid", archive_dir.display()));
        return Err(anyhow!("Invalid archive directory"));
    }

    let mut csv_removed = 0;
    let mut bin_removed = 0;

    if prune_csv {
        csv_removed = prune_csv_archives(archive_dir, keep)?;
    }
    if prune_bin {
        bin_removed = prune_bin_archives(archive_dir, keep)?;
    }

    match (csv_removed, bin_removed) {
        (0, 0) => info("No archives needed pruning."),
        (c, 0) => info(&format!("Pruned {c} old CSV archives.")),
        (0, b) => info(&format!("Pruned {b} old bin archives.")),
        (c, b) => info(&format!("Pruned {c} old CSV archives and {b} old bin archives.")),
    }

    Ok(())
}

/// ... include prune_csv_archives, prune_bin_archives, extract_version as before ...

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
