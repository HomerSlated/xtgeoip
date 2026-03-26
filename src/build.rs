/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, BufWriter, Write, copy},
    path::{Path, PathBuf},
};

use csv::ReaderBuilder;
use ipnetwork::IpNetwork;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use glob::glob;

/// Country data with IPv4 and IPv6 ranges
#[derive(Default)]
struct CountryRanges {
    pool_v4: Vec<(u32, u32)>,
    pool_v6: Vec<(u128, u128)>,
}

/// Main build function
pub fn build(
    source_dir: &Path,
    target_dir: &Path,
    version: &str,
) -> io::Result<()> {
    let legacy = std::env::args()
        .skip(1)
        .any(|arg| arg == "-l" || arg == "--legacy");
    if legacy {
        println!(
            "Warning: Legacy Mode activated. See documentation for collisions."
        );
    }

    let (country_id, mut country_name) = load_countries(source_dir, legacy)?;

    // Ensure special codes exist in the name map (but don't overwrite if already set)
    country_name.entry("A1".into()).or_insert_with(|| "Anonymous Proxy".into());
    country_name.entry("A2".into()).or_insert_with(|| "Satellite Provider".into());
    country_name.entry("O1".into()).or_insert_with(|| "Other Country".into());

    // Initialize ranges for all known country codes
    let mut country_ranges: BTreeMap<String, CountryRanges> = country_name
        .keys()
        .map(|cc| (cc.clone(), CountryRanges::default()))
        .collect();

    // Ensure special codes exist in ranges even if not in CSV
    for cc in ["A1", "A2", "O1"] {
        country_ranges.entry(cc.to_string()).or_default();
    }

    // Check for existing files before overwriting
    check_existing_files(target_dir)?;

    // Parallel CIDR parsing
    load_blocks_parallel(source_dir, &country_id, &mut country_ranges, false)?;
    load_blocks_parallel(source_dir, &country_id, &mut country_ranges, true)?;

    // Ensure output directory exists
    fs::create_dir_all(target_dir)?;

    // Parallel writing of country files
    country_ranges.par_iter().for_each(|(iso_code, cr)| {
        let file_base = target_dir.join(iso_code.to_uppercase());
        let _ = write_country_v4(&file_base, &cr.pool_v4);
        let _ = write_country_v6(&file_base, &cr.pool_v6);
    });

    let countries_processed = country_name.len();
    let ipv4_country_ranges: usize = country_ranges.values().map(|cr| cr.pool_v4.len()).sum();
    let ipv6_country_ranges: usize = country_ranges.values().map(|cr| cr.pool_v6.len()).sum();

    println!("Countries processed: {}", countries_processed);
    println!("IPv4 country ranges: {}", ipv4_country_ranges);
    println!("IPv6 country ranges: {}", ipv6_country_ranges);

    let checksum_path = target_dir.join(format!("GeoLite2-Country-bin_{version}.sha256"));
    let checksum_file = File::create(&checksum_path)?;
    let mut checksum_writer = BufWriter::new(checksum_file);

    for iso_code in country_ranges.keys() {
        for ext in ["iv4", "iv6"] {
            let file_name = format!("{}.{}", iso_code.to_uppercase(), ext);
            let file_path = target_dir.join(&file_name);

            if !file_path.exists() {
                continue;
            }

            let mut file = File::open(&file_path)?;
            let mut hasher = Sha256::new();
            copy(&mut file, &mut hasher)?;
            let digest = hasher.finalize();

            writeln!(checksum_writer, "{:x}  {}", digest, file_name)?;
        }
    }

    fs::write(target_dir.join("version"), format!("{version}\n"))?;

    // Find orphaned files
    let orphaned_files = find_orphaned_files(target_dir, version)?;
    if !orphaned_files.is_empty() {
        let orphaned_path = target_dir.join("orphaned");
        let mut f = File::create(&orphaned_path)?;
        for file in &orphaned_files {
            writeln!(f, "{}", file.display())?;
        }
        println!(
            "Warning: {} orphaned files detected. See {} for list. Run `xtgeoip -c -f` to remove.",
            orphaned_files.len(),
            orphaned_path.display()
        );
    }

    Ok(())
}

// -------------------------
// Helper: glob -> io::Result
// -------------------------
fn glob_to_io(pattern: &str) -> io::Result<Vec<PathBuf>> {
    match glob(pattern) {
        Ok(paths) => Ok(paths.filter_map(Result::ok).collect()),
        Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
    }
}

// -------------------------
// Check existing files for overwrite
// -------------------------
fn check_existing_files(output_dir: &Path) -> io::Result<()> {
    let pattern = format!("{}/*[0-9A-Z][0-9A-Z].iv[46]", output_dir.display());
    let iv_files = glob_to_io(&pattern)?;
    if !iv_files.is_empty() {
        println!(
            "Warning: {} existing iv4/iv6 files will be overwritten",
            iv_files.len()
        );
    }
    Ok(())
}

// -------------------------
// Find orphaned files
// -------------------------
fn find_orphaned_files(output_dir: &Path, current_version: &str) -> io::Result<Vec<PathBuf>> {
    let mut orphaned = vec![];

    let all_iv_files = glob_to_io(&format!("{}/*[0-9A-Z][0-9A-Z].iv[46]", output_dir.display()))?;
    let all_sha_files = glob_to_io(&format!("{}/*.sha256", output_dir.display()))?;

    for file in all_iv_files.iter().chain(all_sha_files.iter()) {
        let fname = file.file_name().unwrap().to_string_lossy();
        if !fname.contains(current_version) && fname != "version" && fname != "orphaned" {
            orphaned.push(file.clone());
        }
    }

    Ok(orphaned)
}

// -------------------------
// The rest of your functions remain unchanged:
// load_countries, load_blocks_parallel, resolve_country_code, 
// cidr_to_range_ipv4, cidr_to_range_ipv6, merge_ranges_v4, merge_ranges_v6,
// write_country_v4, write_country_v6
// -------------------------
