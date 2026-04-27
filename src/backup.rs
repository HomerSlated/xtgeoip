/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    collections::BTreeMap,
    fs::{self},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use flate2::{Compression, write::GzEncoder};
use tar::Builder;

use crate::{
    config::Config,
    messages::{error, info, warn},
    version::Version,
};

const VERSION_FILE: &str = "version";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupMode {
    Verified,
    Force,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneMode {
    Csv,
    Bin,
}

fn version_path(data_dir: &Path) -> PathBuf {
    data_dir.join(VERSION_FILE)
}

fn manifest_path_for_version(data_dir: &Path, version: &str) -> PathBuf {
    data_dir.join(format!("GeoLite2-Country-bin_{}.blake3", version))
}

/// Collect IV files: 2-char uppercase/digit country codes, extension iv4 or
/// iv6.
fn iv_files(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(data_dir)
        .with_context(|| {
            format!("Cannot read directory {}", data_dir.display())
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
                return false;
            };
            if ext != "iv4" && ext != "iv6" {
                return false;
            }
            let stem = &name[..name.len() - ext.len() - 1];
            stem.len() == 2
                && stem
                    .chars()
                    .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
        })
        .collect();
    files.sort();
    Ok(files)
}

/// Collect all .blake3 manifest files in the data directory.
fn all_blake3_files(data_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(data_dir)
        .with_context(|| {
            format!("Cannot read directory {}", data_dir.display())
        })?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("blake3"))
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
            .ok_or_else(|| anyhow!("Malformed manifest at line {}", idx + 1))?;

        let file_name = file_name.trim();

        if file_name.is_empty() {
            let msg = format!("Malformed manifest at line {}", idx + 1);
            error(&msg);
            bail!(msg);
        }

        if file_name.contains('/')
            || file_name.contains('\\')
            || file_name == ".."
            || file_name == "."
        {
            let msg = format!(
                "Manifest contains unsafe file name {:?} at line {}",
                file_name,
                idx + 1
            );
            error(&msg);
            bail!(msg);
        }

        let file_path = data_dir.join(file_name);
        if !file_path.exists() {
            let msg = format!(
                "Manifest mismatch\n{}: file not found\nOperation aborted, no \
                 files have been modified",
                file_name
            );
            error(&msg);
            bail!(msg);
        }

        let data = fs::read(&file_path)?;
        let actual_hash = blake3::hash(&data).to_string();

        if actual_hash != expected_hash {
            let msg = format!(
                "Manifest mismatch\n{}: checksum failed\nOperation aborted, \
                 no files have been modified",
                file_name
            );
            error(&msg);
            bail!(msg);
        }

        verified_files.push(file_path);
    }

    Ok(verified_files)
}

/// Write a tar.gz archive to `path` (inner step; no atomicity).
fn write_tarball(path: &Path, files: &[PathBuf]) -> Result<()> {
    let tar_gz = fs::File::create(path)?;
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

/// Create tar.gz archive atomically: write to `.part`, rename on success.
fn create_tarball(output_path: &Path, files: &[PathBuf]) -> Result<()> {
    let mut tmp_name = output_path.as_os_str().to_os_string();
    tmp_name.push(".part");
    let tmp_path = PathBuf::from(tmp_name);

    if let Err(e) = write_tarball(&tmp_path, files) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    fs::rename(&tmp_path, output_path).with_context(|| {
        format!(
            "Failed to rename {} to {}",
            tmp_path.display(),
            output_path.display()
        )
    })
}

/// Collect files for a force backup/delete — no verification, just grab
/// everything
/// Collect files for backup/delete. Force mode grabs everything; Verified mode
/// requires a valid version file, existing manifest, and passing checksums.
fn gather_files(
    data_dir: &Path,
    mode: BackupMode,
) -> Result<(Vec<PathBuf>, String, Option<PathBuf>)> {
    match mode {
        BackupMode::Force => {
            let version = fs::read_to_string(version_path(data_dir))
                .map(|s| {
                    let v = s.trim().to_string();
                    if v.contains('/') || v.contains('\\') {
                        "unknown_version".to_string()
                    } else {
                        v
                    }
                })
                .unwrap_or_else(|_| "unknown_version".to_string());

            let mut files = iv_files(data_dir)?;
            let version_file = version_path(data_dir);
            if version_file.exists() {
                files.push(version_file);
            }
            files.extend(all_blake3_files(data_dir)?);
            Ok((files, version, None))
        }
        BackupMode::Verified => {
            let version = fs::read_to_string(version_path(data_dir))
                .map(|s| s.trim().to_string())
                .map_err(|_| {
                    let msg = format!(
                        "Version file missing: {}. Use -f to force operation",
                        version_path(data_dir).display()
                    );
                    error(&msg);
                    anyhow!(msg)
                })?;

            if version.contains('/') || version.contains('\\') {
                bail!(
                    "Version string contains unsafe characters: {:?}",
                    version
                );
            }

            let manifest_path = manifest_path_for_version(data_dir, &version);
            if !manifest_path.exists() {
                let msg = format!(
                    "Manifest missing: {}\nExpected manifest not found. Use \
                     -f to force",
                    manifest_path.display()
                );
                error(&msg);
                bail!(msg);
            }

            let files = verify_manifest_files(data_dir, &manifest_path)?;
            Ok((files, version, Some(manifest_path)))
        }
    }
}

/// Backup IV files, version file, and manifest.
pub fn backup(
    data_dir: &Path,
    backup_dir: &Path,
    mode: BackupMode,
) -> Result<()> {
    fs::create_dir_all(backup_dir).with_context(|| {
        format!("Failed to create backup directory {}", backup_dir.display())
    })?;

    let (mut files, version, manifest) = gather_files(data_dir, mode)?;
    if files.is_empty() {
        let msg = "Nothing to back up";
        error(msg);
        bail!(msg);
    }

    let output_path = match mode {
        BackupMode::Force => backup_dir.join(format!(
            "GeoLite2-Country-bin_unverified_{}.tar.gz",
            version
        )),
        BackupMode::Verified => {
            files.push(version_path(data_dir));
            if let Some(m) = manifest {
                files.push(m);
            }
            backup_dir.join(format!("GeoLite2-Country-bin_{}.tar.gz", version))
        }
    };

    files.sort();
    files.dedup();
    create_tarball(&output_path, &files)?;

    let label = match mode {
        BackupMode::Force => "unverified binary",
        BackupMode::Verified => "binary",
    };
    info(&format!(
        "Backed up {label} data to {}",
        output_path.display()
    ));
    Ok(())
}

fn delete_all(data_dir: &Path, files: &[PathBuf]) -> Result<()> {
    let mut failed: Vec<(&PathBuf, std::io::Error)> = Vec::new();
    for f in files {
        if let Err(e) = fs::remove_file(f) {
            failed.push((f, e));
        }
    }
    if !failed.is_empty() {
        for (f, e) in &failed {
            error(&format!("Failed to delete {}: {e:#}", f.display()));
        }
        let list = failed
            .iter()
            .map(|(p, _)| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let orphaned_path = data_dir.join("orphaned");
        if let Err(e) = fs::write(&orphaned_path, &list) {
            error(&format!(
                "Could not write failed-delete list to \"{}\": {e:#}",
                orphaned_path.display()
            ));
        }
        bail!("{} file(s) could not be deleted", failed.len());
    }
    Ok(())
}

pub fn delete(data_dir: &Path, mode: BackupMode) -> Result<()> {
    let (files, _version, manifest) = gather_files(data_dir, mode)?;
    if files.is_empty() {
        let msg = "Nothing to delete";
        error(msg);
        bail!(msg);
    }

    let all_files: Vec<PathBuf> = match mode {
        BackupMode::Force => {
            let orphan_path = data_dir.join("orphaned");
            files
                .into_iter()
                .chain(orphan_path.exists().then_some(orphan_path))
                .collect()
        }
        BackupMode::Verified => files
            .into_iter()
            .chain([version_path(data_dir)])
            .chain(manifest)
            .collect(),
    };

    let n = all_files.len();
    delete_all(data_dir, &all_files)?;

    match mode {
        BackupMode::Force => info(&format!(
            "Force deleted {n} file(s) from {}",
            data_dir.display()
        )),
        BackupMode::Verified => {
            info(&format!("Deleted {n} file(s) from {}", data_dir.display()))
        }
    }
    Ok(())
}

/// Summary of pruning operations.
struct PruneSummary {
    csv_removed: usize,
    bin_removed: usize,
}

/// Prune old GeoLite2 archives according to the config.
///
/// Behaviour:
/// - Fails hard if the archive directory is missing or invalid.
/// - Fails if `paths.archive_prune < 1`.
/// - Prints a concise summary on success.
/// - Prints a partial summary before failing on error.
pub fn prune_archives(cfg: &Config, mode: PruneMode) -> Result<()> {
    let archive_dir = Path::new(&cfg.paths.archive_dir);
    let keep = cfg.paths.archive_prune;

    if keep < 1 {
        let msg = "Invalid configuration: paths.archive_prune must be >= 1";
        error(msg);
        bail!(msg);
    }

    if !archive_dir.exists() || !archive_dir.is_dir() {
        let msg = format!(
            "Archive directory {} is missing or invalid",
            archive_dir.display()
        );
        error(&msg);
        bail!(msg);
    }

    let mut summary = PruneSummary {
        csv_removed: 0,
        bin_removed: 0,
    };

    if mode == PruneMode::Csv {
        match prune_csv_archives(archive_dir, keep) {
            Ok(count) => summary.csv_removed = count,
            Err(e) => {
                print_prune_summary(&summary);
                return Err(e);
            }
        }
    }

    if mode == PruneMode::Bin {
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
        (0, 0) => info("No archives needed pruning."),
        (c, 0) => info(&format!("Pruned {c} old CSV archives.")),
        (0, b) => info(&format!("Pruned {b} old bin archives.")),
        (c, b) => info(&format!(
            "Pruned {c} old CSV archives and {b} old bin archives."
        )),
    }
}

fn prune_csv_archives(dir: &Path, keep: usize) -> Result<usize> {
    let entries = fs::read_dir(dir).with_context(|| {
        format!("Cannot read archive directory {}", dir.display())
    })?;

    // Map: version -> Vec<PathBuf> (zip + sha256)
    let mut by_version: BTreeMap<Version, Vec<PathBuf>> = BTreeMap::new();

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

        match Version::parse(name) {
            Some(ver) => {
                by_version.entry(ver).or_default().push(path.clone());
            }
            None => {
                warn(&format!(
                    "Skipping CSV archive with unparseable name: {name}"
                ));
            }
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
    let mut by_version: BTreeMap<Version, Vec<PathBuf>> = BTreeMap::new();

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

        match Version::parse(name) {
            Some(ver) => {
                by_version.entry(ver).or_default().push(path.clone());
            }
            None => {
                warn(&format!(
                    "Skipping bin archive with unparseable name: {name}"
                ));
            }
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
