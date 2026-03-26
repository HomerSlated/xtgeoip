/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufWriter, Write, copy},
    path::{Path, PathBuf},
};

use csv::ReaderBuilder;
use glob::glob;
use ipnetwork::IpNetwork;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

// -------------------------
// Country data structures
// -------------------------
#[derive(Default)]
struct CountryRanges {
    pool_v4: Vec<(u32, u32)>,
    pool_v6: Vec<(u128, u128)>,
}

// -------------------------
// Main build function
// -------------------------
pub fn build(
    source_dir: &Path,
    target_dir: &Path,
    version: &str,
) -> std::io::Result<()> {
    let legacy = std::env::args()
        .skip(1)
        .any(|arg| arg == "-l" || arg == "--legacy");
    if legacy {
        println!(
            "Warning: Legacy Mode activated. See documentation for collisions."
        );
    }

    let (country_id, mut country_name) = load_countries(source_dir, legacy)?;

    // Ensure special codes exist
    country_name.entry("A1".into()).or_insert_with(|| "Anonymous Proxy".into());
    country_name.entry("A2".into()).or_insert_with(|| "Satellite Provider".into());
    country_name.entry("O1".into()).or_insert_with(|| "Other Country".into());

    let mut country_ranges: BTreeMap<String, CountryRanges> = country_name
        .keys()
        .map(|cc| (cc.clone(), CountryRanges::default()))
        .collect();
    for cc in ["A1", "A2", "O1"] {
        country_ranges.entry(cc.to_string()).or_default();
    }

    // Parallel CIDR parsing
    load_blocks_parallel(source_dir, &country_id, &mut country_ranges, false)?;
    load_blocks_parallel(source_dir, &country_id, &mut country_ranges, true)?;

    // Ensure output directory exists
    std::fs::create_dir_all(target_dir)?;

    // -------------------------
    // --- NEW: Check for overwrites & orphaned files ---
    // -------------------------
    check_existing_files(target_dir)?;
    let orphaned = find_orphaned_files(target_dir, version)?;
    write_orphaned_file(target_dir, &orphaned)?;

    // Parallel writing of country files (binary, headerless)
    country_ranges.par_iter().for_each(|(iso_code, cr)| {
        let file_base = target_dir.join(iso_code.to_uppercase());
        let _ = write_country_v4(&file_base, &cr.pool_v4);
        let _ = write_country_v6(&file_base, &cr.pool_v6);
    });

    // Report statistics
    let countries_processed = country_name.len();
    let ipv4_country_ranges: usize =
        country_ranges.values().map(|cr| cr.pool_v4.len()).sum();
    let ipv6_country_ranges: usize =
        country_ranges.values().map(|cr| cr.pool_v6.len()).sum();

    println!("Countries processed: {}", countries_processed);
    println!("IPv4 country ranges: {}", ipv4_country_ranges);
    println!("IPv6 country ranges: {}", ipv6_country_ranges);

    // Compute SHA256 checksums
    let checksum_path = target_dir.join(format!("GeoLite2-Country-bin_{version}.sha256"));
    let checksum_file = File::create(checksum_path)?;
    let mut checksum_writer = BufWriter::new(checksum_file);

    for iso_code in country_ranges.keys() {
        for ext in ["iv4", "iv6"] {
            let file_name = format!("{}.{}", iso_code.to_uppercase(), ext);
            let file_path = target_dir.join(&file_name);

            let mut file = File::open(&file_path)?;
            let mut hasher = Sha256::new();
            copy(&mut file, &mut hasher)?;
            let digest = hasher.finalize();

            writeln!(checksum_writer, "{:x}  {}", digest, file_name)?;
        }
    }

    // Write version file
    fs::write(target_dir.join("version"), format!("{version}\n"))?;

    Ok(())
}

// -------------------------
// Helper: Check for existing IV files that will be overwritten
// -------------------------
fn check_existing_files(output_dir: &Path) -> std::io::Result<()> {
    let iv_files: Vec<_> = glob(&format!("{}/*[0-9A-Z][0-9A-Z].iv[46]", output_dir.display()))?
        .filter_map(Result::ok)
        .collect();

    if !iv_files.is_empty() {
        println!(
            "Warning: {} IV files will be overwritten in {}",
            iv_files.len(),
            output_dir.display()
        );
    }

    Ok(())
}

// -------------------------
// Helper: Detect orphaned files
// -------------------------
fn find_orphaned_files(output_dir: &Path, current_version: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut orphaned = Vec::new();

    // All IV files
    let all_iv_files: Vec<_> = glob(&format!("{}/*[0-9A-Z][0-9A-Z].iv[46]", output_dir.display()))?
        .filter_map(Result::ok)
        .collect();

    // All SHA256 files
    let all_sha_files: Vec<_> = glob(&format!("{}/*.sha256", output_dir.display()))?
        .filter_map(Result::ok)
        .collect();

    // Current manifest
    let current_manifest = output_dir.join(format!("GeoLite2-Country-bin_{}.sha256", current_version));
    let mut manifest_files = Vec::new();
    if current_manifest.exists() {
        let manifest_lines = std::fs::read_to_string(&current_manifest)?;
        for line in manifest_lines.lines() {
            if let Some((_, file_name)) = line.split_once(' ') {
                manifest_files.push(output_dir.join(file_name.trim()));
            }
        }
        manifest_files.push(current_manifest.clone());
    }

    // Version file is never orphaned
    let version_file = output_dir.join("version");

    for file in all_iv_files.iter().chain(all_sha_files.iter()) {
        if file != &version_file && !manifest_files.contains(file) {
            orphaned.push(file.clone());
        }
    }

    Ok(orphaned)
}

// -------------------------
// Helper: Write orphaned file list to "orphaned"
// -------------------------
fn write_orphaned_file(output_dir: &Path, orphaned: &[PathBuf]) -> std::io::Result<()> {
    if orphaned.is_empty() {
        return Ok(());
    }

    let orphan_path = output_dir.join("orphaned");
    let mut f = File::create(&orphan_path)?;
    for file in orphaned {
        writeln!(f, "{}", file.display())?;
    }

    println!(
        "Warning: {} orphaned files detected. They will be left after this build.\n\
         Run `xtgeoip -c -f` to clean, or manually remove files listed in {}",
        orphaned.len(),
        orphan_path.display()
    );

    Ok(())
}
