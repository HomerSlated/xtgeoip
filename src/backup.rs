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

/// gzip level for bin archives (#99).
///
/// Not `Compression::default()` (6), which is **strictly dominated** on this
/// data — measured over the real 507-file output directory, mean of 3:
///
/// | level | time | size |
/// |-------|--------|----------|
/// | 1     | 131 ms | 3.89 MB  |
/// | **4** | **360 ms** | **3.27 MB** |
/// | 6     | 807 ms | 3.31 MB  |
/// | 9     | 2.6 s  | 3.31 MB  |
///
/// Level 4 is 2.2× faster than 6 *and* marginally smaller: zlib changes both
/// search depth and lazy-matching strategy with level, and past 4 the extra
/// effort buys nothing here (6–9 all land on 3.31 MB). Compression dominated
/// backup wall time — 96–98.5% of it — so this is the whole operation getting
/// ~2× faster.
///
/// The speed win holds for any input; "also smaller" is a property of this
/// data and may not generalise. Level 0 is not an option: storing uncompressed
/// is *slower* (197 ms) because writing 11.38 MB costs more than compressing
/// it.
const COMPRESSION_LEVEL: u32 = 4;

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
            bail!("Malformed manifest at line {}", idx + 1);
        }

        if file_name.contains('/')
            || file_name.contains('\\')
            || file_name == ".."
            || file_name == "."
        {
            bail!(
                "Manifest contains unsafe file name {:?} at line {}",
                file_name,
                idx + 1
            );
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
        let actual_hash = blake3::hash(&data).to_string();

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

/// Write a tar.gz archive to `path` (inner step; no atomicity).
fn write_tarball(path: &Path, files: &[PathBuf]) -> Result<()> {
    let tar_gz = fs::File::create(path)?;
    let enc = GzEncoder::new(tar_gz, Compression::new(COMPRESSION_LEVEL));
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
                    anyhow!(
                        "Version file missing: {}. Use -f to force operation",
                        version_path(data_dir).display()
                    )
                })?;

            if version.contains('/') || version.contains('\\') {
                bail!(
                    "Version string contains unsafe characters: {:?}",
                    version
                );
            }

            let manifest_path = manifest_path_for_version(data_dir, &version);
            if !manifest_path.exists() {
                bail!(
                    "Manifest missing: {}\nExpected manifest not found. Use \
                     -f to force",
                    manifest_path.display()
                );
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
        bail!("Nothing to back up");
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
        bail!("Nothing to delete");
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::TempDir;

    use super::*;

    fn file_with(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    /// Read an archive back: entry names paired with their contents.
    ///
    /// Test-only. `xtgeoip` deliberately has no restore — see
    /// `docs/design/98-state-ownership-recovery.md` §0. This exists to prove
    /// what `write_tarball` produced, not to offer recovery.
    fn read_archive(path: &Path) -> Vec<(String, Vec<u8>)> {
        use std::io::Read;
        let f = fs::File::open(path).unwrap();
        let dec = flate2::read::GzDecoder::new(f);
        let mut ar = tar::Archive::new(dec);
        let mut out = Vec::new();
        for entry in ar.entries().unwrap() {
            let mut e = entry.unwrap();
            let name = e.path().unwrap().to_string_lossy().into_owned();
            let mut buf = Vec::new();
            e.read_to_end(&mut buf).unwrap();
            out.push((name, buf));
        }
        out.sort();
        out
    }

    /// The level change (#99) is only safe if archives stay readable and
    /// byte-exact. gzip level affects encoding, never decoded content — this
    /// pins that, since `backup.rs` had no tests before.
    #[test]
    fn tarball_round_trips_contents_intact() {
        let src = TempDir::new().unwrap();
        let out = TempDir::new().unwrap();
        let files = vec![
            file_with(src.path(), "AA.iv4", b"\x01\x02\x03\x04"),
            file_with(src.path(), "AA.iv6", &[0xFFu8; 64]),
            file_with(src.path(), VERSION_FILE, b"20260714"),
        ];

        let archive = out.path().join("test.tar.gz");
        write_tarball(&archive, &files).unwrap();

        let got = read_archive(&archive);
        assert_eq!(
            got,
            vec![
                ("AA.iv4".to_string(), b"\x01\x02\x03\x04".to_vec()),
                ("AA.iv6".to_string(), vec![0xFFu8; 64]),
                (VERSION_FILE.to_string(), b"20260714".to_vec()),
            ]
        );
    }

    /// Entries are stored by file name only — a leading path would make the
    /// archive extract into unexpected locations.
    #[test]
    fn tarball_entries_are_flat() {
        let src = TempDir::new().unwrap();
        let out = TempDir::new().unwrap();
        let nested = src.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let files = vec![file_with(&nested, "ZZ.iv4", b"data")];

        let archive = out.path().join("flat.tar.gz");
        write_tarball(&archive, &files).unwrap();

        let names: Vec<String> =
            read_archive(&archive).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["ZZ.iv4".to_string()]);
    }

    /// Missing files are skipped rather than aborting the backup.
    #[test]
    fn tarball_skips_nonexistent_files() {
        let src = TempDir::new().unwrap();
        let out = TempDir::new().unwrap();
        let files = vec![
            file_with(src.path(), "AA.iv4", b"present"),
            src.path().join("GONE.iv4"),
        ];

        let archive = out.path().join("partial.tar.gz");
        write_tarball(&archive, &files).unwrap();

        let names: Vec<String> =
            read_archive(&archive).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, vec!["AA.iv4".to_string()]);
    }

    /// `create_tarball` writes to `.part` and renames on success, so a failed
    /// write can never leave a half-written archive in place.
    #[test]
    fn create_tarball_leaves_no_part_file() {
        let src = TempDir::new().unwrap();
        let out = TempDir::new().unwrap();
        let files = vec![file_with(src.path(), "AA.iv4", b"x")];

        let archive = out.path().join("atomic.tar.gz");
        create_tarball(&archive, &files).unwrap();

        assert!(archive.exists(), "archive must exist");
        let part = out.path().join("atomic.tar.gz.part");
        assert!(!part.exists(), "stale .part left behind");
    }

    /// Guards the constant itself: 0 would be slower *and* far larger, and
    /// anything above 5 is pure waste on this data (#99).
    #[test]
    fn compression_level_is_in_the_useful_range() {
        assert!(
            (1..=5).contains(&COMPRESSION_LEVEL),
            "level {COMPRESSION_LEVEL} is outside the measured Pareto \
             frontier — see the constant's docs"
        );
    }
}
