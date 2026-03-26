/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File},
    io::{BufWriter, Write, copy},
    path::Path,
};

use csv::ReaderBuilder;
use ipnetwork::IpNetwork;
use rayon::prelude::*;
use sha2::{Digest, Sha256};

/// Country data with IPv4 and IPv6 ranges
#[derive(Default)]
struct CountryRanges {
    pool_v4: Vec<(u32, u32)>,
    pool_v6: Vec<(u128, u128)>,
}

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

    // Ensure special codes exist in the name map (but don't overwrite if already set)
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

    load_blocks_parallel(source_dir, &country_id, &mut country_ranges, false)?;
    load_blocks_parallel(source_dir, &country_id, &mut country_ranges, true)?;

    std::fs::create_dir_all(target_dir)?;

    // -------------------------
    // Prepare files we will write
    // -------------------------
    let files_to_write: Vec<_> = country_ranges
        .keys()
        .flat_map(|iso| {
            let base = target_dir.join(iso.to_uppercase());
            vec![base.with_extension("iv4"), base.with_extension("iv6")]
        })
        .collect();

    // -------------------------
    // Detect overwrites
    // -------------------------
    let overwrite_count = files_to_write.iter().filter(|f| f.exists()).count();
    if overwrite_count > 0 {
        println!(
            "Warning: {} country files (iv4/iv6) will be overwritten.",
            overwrite_count
        );
    }

    // -------------------------
    // Write country files
    // -------------------------
    country_ranges.par_iter().for_each(|(iso, cr)| {
        let base = target_dir.join(iso.to_uppercase());
        let _ = write_country_v4(&base, &cr.pool_v4);
        let _ = write_country_v6(&base, &cr.pool_v6);
    });

    // -------------------------
    // Version file
    // -------------------------
    fs::write(target_dir.join("version"), format!("{version}\n"))?;

    // -------------------------
    // SHA256 manifest
    // -------------------------
    let checksum_name = format!("GeoLite2-Country-bin_{version}.sha256");
    let checksum_path = target_dir.join(&checksum_name);
    let checksum_file = File::create(&checksum_path)?;
    let mut checksum_writer = BufWriter::new(checksum_file);

    for iso in country_ranges.keys() {
        for ext in ["iv4", "iv6"] {
            let file_name = format!("{}.{}", iso.to_uppercase(), ext);
            let file_path = target_dir.join(&file_name);

            let mut file = File::open(&file_path)?;
            let mut hasher = Sha256::new();
            copy(&mut file, &mut hasher)?;
            let digest = hasher.finalize();

            writeln!(checksum_writer, "{:x}  {}", digest, file_name)?;
        }
    }

    // -------------------------
    // Detect orphaned files
    // -------------------------
    let all_existing: Vec<_> = fs::read_dir(target_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(OsStr::to_str).unwrap_or("");
            let fname = p.file_name().and_then(OsStr::to_str).unwrap_or("");
            fname != "version" && (ext == "iv4" || ext == "iv6" || ext == "sha256")
        })
        .collect();

    let written_files: Vec<_> = files_to_write
        .iter()
        .chain(std::iter::once(&checksum_path))
        .collect();

    let orphaned: Vec<_> = all_existing
        .into_iter()
        .filter(|p| !written_files.contains(&p))
        .collect();

    if !orphaned.is_empty() {
        let orphaned_path = target_dir.join("orphaned");
        fs::write(&orphaned_path, orphaned.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n"))?;
        println!(
            "Warning: {} orphaned files detected in \"{}\".",
            orphaned.len(),
            target_dir.display()
        );
        println!(
            "Please run `xtgeoip build -c -f` or manually delete files listed in \"{}\" for a clean install.",
            orphaned_path.display()
        );
    }

    // -------------------------
    // Summary
    // -------------------------
    println!("Countries processed: {}", country_name.len());
    let ipv4_count: usize = country_ranges.values().map(|cr| cr.pool_v4.len()).sum();
    let ipv6_count: usize = country_ranges.values().map(|cr| cr.pool_v6.len()).sum();
    println!("IPv4 country ranges: {}", ipv4_count);
    println!("IPv6 country ranges: {}", ipv6_count);

    Ok(())
}

// -------------------------
// Load countries
// -------------------------
fn load_countries(
    source_dir: &Path,
    legacy: bool,
) -> std::io::Result<(BTreeMap<String, String>, BTreeMap<String, String>)> {
    let file_path = source_dir.join("GeoLite2-Country-Locations-en.csv");
    let file = File::open(file_path)?;
    let mut rdr = ReaderBuilder::new().has_headers(true).from_reader(file);
    let headers = rdr.headers()?.clone();

    let idx_geoname = headers.iter().position(|h| h == "geoname_id").expect("geoname_id column missing");
    let idx_iso = headers.iter().position(|h| h == "country_iso_code").expect("country_iso_code column missing");
    let idx_name = headers.iter().position(|h| h == "country_name").expect("country_name column missing");
    let idx_continent = headers.iter().position(|h| h == "continent_code").expect("continent_code column missing");

    let mut country_id = BTreeMap::new();
    let mut country_name = BTreeMap::new();

    for record in rdr.records() {
        let rec = record?;
        let geoname = rec.get(idx_geoname).unwrap_or("").to_string();
        let iso = rec.get(idx_iso).unwrap_or("").to_string();
        let name = rec.get(idx_name).unwrap_or("").to_string();
        let continent = rec.get(idx_continent).unwrap_or("").to_string();

        if !iso.is_empty() {
            country_id.insert(geoname.clone(), iso.clone());
            country_name.entry(iso.clone()).or_insert(name);
        } else if geoname == "6255148" || geoname == "6255147" {
            if legacy {
                country_id.insert(geoname.clone(), continent.clone());
                country_name.entry(continent.clone()).or_insert(name);
            } else {
                country_id.insert(geoname.clone(), "O1".to_string());
                country_name.entry("O1".to_string()).or_insert(name);
            }
        } else {
            country_id.insert(geoname.clone(), "".to_string());
            country_name.entry("O1".to_string()).or_insert(name);
        }
    }

    Ok((country_id, country_name))
}

// -------------------------
// Parallel block loading
// -------------------------
fn load_blocks_parallel(
    source_dir: &Path,
    country_id: &BTreeMap<String, String>,
    country_ranges: &mut BTreeMap<String, CountryRanges>,
    ipv6: bool,
) -> std::io::Result<()> {
    let file_name = if ipv6 {
        "GeoLite2-Country-Blocks-IPv6.csv"
    } else {
        "GeoLite2-Country-Blocks-IPv4.csv"
    };
    let file_path = source_dir.join(file_name);
    let file = File::open(file_path)?;
    let mut rdr = ReaderBuilder::new().has_headers(true).from_reader(file);
    let headers = rdr.headers()?.clone();

    let idx_net = headers.iter().position(|h| h == "network").expect("network column missing");
    let idx_id = headers.iter().position(|h| h == "geoname_id").expect("geoname_id column missing");
    let idx_rid = headers.iter().position(|h| h == "registered_country_geoname_id").expect("registered_country_geoname_id column missing");
    let idx_proxy = headers.iter().position(|h| h == "is_anonymous_proxy").expect("is_anonymous_proxy column missing");
    let idx_sat = headers.iter().position(|h| h == "is_satellite_provider").expect("is_satellite_provider column missing");

    let records: Vec<_> = rdr.records().collect::<Result<_, _>>()?;

    let parsed: Vec<(String, Option<(u128, u128)>)> = records
        .into_par_iter()
        .map(|rec| {
            let id = rec.get(idx_id).unwrap_or("");
            let rid = rec.get(idx_rid).unwrap_or("");
            let proxy = rec.get(idx_proxy).unwrap_or("") == "1";
            let sat = rec.get(idx_sat).unwrap_or("") == "1";
            let network = rec.get(idx_net).unwrap_or("");

            let cc = resolve_country_code(proxy, sat, id, rid, country_id);

            if network.is_empty() {
                return (cc, None);
            }

            let range = if ipv6 {
                cidr_to_range_ipv6(network)
            } else {
                cidr_to_range_ipv4(network).map(|(s, e)| (s as u128, e as u128))
            };

            (cc, range)
        })
        .collect();

    for (cc, range_opt) in parsed {
        if let Some((start, end)) = range_opt {
            if ipv6 {
                country_ranges.entry(cc).or_default().pool_v6.push((start, end));
            } else {
                country_ranges.entry(cc).or_default().pool_v4.push((start as u32, end as u32));
            }
        }
    }

    if ipv6 {
        country_ranges.par_iter_mut().for_each(|(_, cr)| cr.pool_v6 = merge_ranges_v6(&cr.pool_v6));
    } else {
        country_ranges.par_iter_mut().for_each(|(_, cr)| cr.pool_v4 = merge_ranges_v4(&cr.pool_v4));
    }

    Ok(())
}

fn resolve_country_code(proxy: bool, sat: bool, id: &str, rid: &str, country_id: &BTreeMap<String, String>) -> String {
    if proxy { return "A1".to_string(); }
    if sat { return "A2".to_string(); }
    let key = if !id.is_empty() { id } else { rid };
    if key.is_empty() { return "O1".to_string(); }
    country_id.get(key).filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| key.to_string())
}

// -------------------------
// CIDR → Range
// -------------------------
fn cidr_to_range_ipv4(cidr: &str) -> Option<(u32, u32)> {
    let net: IpNetwork = cidr.parse().ok()?;
    match net { IpNetwork::V4(v4) => Some((u32::from(v4.network()), u32::from(v4.broadcast()))), _ => None }
}

fn cidr_to_range_ipv6(cidr: &str) -> Option<(u128, u128)> {
    let net: IpNetwork = cidr.parse().ok()?;
    match net { IpNetwork::V6(v6) => Some((u128::from(v6.network()), u128::from(v6.broadcast()))), _ => None }
}

// -------------------------
// Merge ranges
// -------------------------
fn merge_ranges_v4(ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    if ranges.is_empty() { return vec![]; }
    let mut sorted = ranges.to_vec();
    sorted.sort_unstable_by_key(|r| r.0);
    let mut merged = vec![sorted[0]];
    for &(start, end) in &sorted[1..] {
        let last = merged.last_mut().unwrap();
        if start <= last.1.saturating_add(1) { last.1 = last.1.max(end); } else { merged.push((start, end)); }
    }
    merged
}

fn merge_ranges_v6(ranges: &[(u128, u128)]) -> Vec<(u128, u128)> {
    if ranges.is_empty() { return vec![]; }
    let mut sorted = ranges.to_vec();
    sorted.sort_unstable_by_key(|r| r.0);
    let mut merged = vec![sorted[0]];
    for &(start, end) in &sorted[1..] {
        let last = merged.last_mut().unwrap();
        if start <= last.1.saturating_add(1) { last.1 = last.1.max(end); } else { merged.push((start, end)); }
    }
    merged
}

// -------------------------
// Write country files (binary, headerless)
// -------------------------
fn write_country_v4(file_base: &Path, ranges: &[(u32, u32)]) -> std::io::Result<()> {
    let file_name = file_base.with_extension("iv4");
    let file = File::create(file_name)?;
    let mut writer = BufWriter::new(file);
    for &(start, end) in ranges { writer.write_all(&start.to_be_bytes())?; writer.write_all(&end.to_be_bytes())?; }
    Ok(())
}

fn write_country_v6(file_base: &Path, ranges: &[(u128, u128)]) -> std::io::Result<()> {
    let file_name = file_base.with_extension("iv6");
    let file = File::create(file_name)?;
    let mut writer = BufWriter::new(file);
    for &(start, end) in ranges { writer.write_all(&start.to_be_bytes())?; writer.write_all(&end.to_be_bytes())?; }
    Ok(())
}
