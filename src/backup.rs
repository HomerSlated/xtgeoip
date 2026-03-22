/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, bail};
use flate2::{Compression, write::GzEncoder};
use glob::glob;
use sha2::{Digest, Sha256};
use tar::Builder;

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
    let version_result = fs::read_to_string(version_path(data_dir));
    let version = version_result
        .as_ref()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown_version".to_string());
    let manifest_path = manifest_path_for_version(data_dir, &version);

    if force {
        let mut files = iv_files(data_dir)?;
        let version_file = version_path(data_dir);
        if version_file.exists() {
            files.push(version_file);
        }
        for sha in all_sha256_files(data_dir)? {
            files.push(sha);
        }
        return Ok((files, version, Some(manifest_path)));
    }

    if version_result.is_err() {
        bail!(
            "Version file missing: {}\nUse -f to force operation",
            version_path(data_dir).display()
        );
    }

    let manifest_opt = if manifest_path.exists() {
        Some(manifest_path.clone())
    } else {
        None
    };

    Ok((Vec::new(), version, manifest_opt))
}

/// Backup IV files, version file, and manifest. Force option allows backup even
/// if version/manifest missing.
pub fn backup(data_dir: &Path, backup_dir: &Path, force: bool) -> Result<()> {
    fs::create_dir_all(backup_dir)?;

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
        println!(
            "Backed up unverified binary data to {}",
            output_path.display()
        );
        return Ok(());
    }

    // Non-force: verify manifest
    let manifest_path = manifest_opt.ok_or_else(|| {
        anyhow!(
            "Manifest missing: {}\nExpected manifest not found. Use -f to \
             force backup",
            manifest_path_for_version(data_dir, &version).display()
        )
    })?;

    files = verify_manifest_files(data_dir, &manifest_path)?;
    files.push(version_path(data_dir));
    files.push(manifest_path.clone());

    let output_path =
        backup_dir.join(format!("GeoLite2-Country-bin_{}.tar.gz", version));
    create_tarball(&output_path, &files)?;
    println!("Backed up binary data to {}", output_path.display());

    Ok(())
}

/// Delete IV files, version file, and manifest. Force option allows deletion
/// even if version/manifest missing.
pub fn delete(data_dir: &Path, force: bool) -> Result<()> {
    let (mut files, version, manifest_opt) = gather_files(data_dir, force)?;

    if force {
        if files.is_empty() {
            bail!("Nothing to delete");
        }
        for file in files {
            fs::remove_file(file)?;
        }
        println!(
            "Force deleted binary data files from {}",
            data_dir.display()
        );
        return Ok(());
    }

    let manifest_path = manifest_opt.ok_or_else(|| {
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

    println!("Deleted old binary data files from {}", data_dir.display());
    Ok(())
}
