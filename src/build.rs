/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    fs::{self, File},
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::bail;
use csv::ReaderBuilder;
use ipnetwork::IpNetwork;
use memmap2::Mmap;
use rayon::prelude::*;

use crate::messages;

#[derive(Default)]
struct CountryRanges {
    pool_v4: Vec<(u32, u32)>,
    pool_v6: Vec<(u128, u128)>,
}

struct BlockIndices {
    net: usize,
    id: usize,
    rid: usize,
    proxy: usize,
    sat: usize,
}

pub fn build(
    source_dir: &Path,
    target_dir: &Path,
    version: &str,
    legacy: bool,
) -> anyhow::Result<()> {
    if legacy {
        messages::warn(
            "Legacy Mode activated. See documentation for collisions.",
        );
    }

    let (country_id, mut country_name) = load_countries(source_dir, legacy)?;

    country_name
        .entry("A1".into())
        .or_insert_with(|| "Anonymous Proxy".into());
    country_name
        .entry("A2".into())
        .or_insert_with(|| "Satellite Provider".into());
    country_name
        .entry("O1".into())
        .or_insert_with(|| "Other Country".into());

    let mut country_ranges: BTreeMap<String, CountryRanges> = country_name
        .keys()
        .map(|cc| (cc.clone(), CountryRanges::default()))
        .collect();

    for cc in ["A1", "A2", "O1"] {
        country_ranges.entry(cc.to_string()).or_default();
    }

    // Load IPv4 and IPv6 block files in parallel
    let (v4_result, v6_result) = rayon::join(
        || load_blocks_v4(source_dir, &country_id),
        || load_blocks_v6(source_dir, &country_id),
    );
    let v4_pools = v4_result?;
    let v6_pools = v6_result?;

    for (cc, pool) in v4_pools {
        country_ranges.entry(cc).or_default().pool_v4 = pool;
    }
    for (cc, pool) in v6_pools {
        country_ranges.entry(cc).or_default().pool_v6 = pool;
    }

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
        messages::warn(&format!(
            "{} country files (iv4/iv6) will be overwritten.",
            overwrite_count
        ));
    }

    // -------------------------
    // Write country files in parallel, collecting (filename, hash) pairs
    // -------------------------
    let write_results: Vec<anyhow::Result<(String, String)>> = country_ranges
        .par_iter()
        .flat_map(|(iso, cr)| {
            let base = target_dir.join(iso.to_uppercase());
            vec![
                write_country_v4(&base, &cr.pool_v4),
                write_country_v6(&base, &cr.pool_v6),
            ]
        })
        .collect();

    let mut checksums: Vec<(String, String)> =
        Vec::with_capacity(write_results.len());
    let mut write_errors: Vec<anyhow::Error> = Vec::new();
    for result in write_results {
        match result {
            Ok(entry) => checksums.push(entry),
            Err(e) => write_errors.push(e),
        }
    }
    if !write_errors.is_empty() {
        for e in &write_errors {
            messages::error(&format!("{e:#}"));
        }
        bail!("{} file write(s) failed during build", write_errors.len());
    }
    checksums.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    // -------------------------
    // Version file
    // -------------------------
    fs::write(target_dir.join("version"), format!("{version}\n"))?;

    // -------------------------
    // Blake3 manifest (single write, no re-read of files)
    // -------------------------
    let manifest_name = format!("GeoLite2-Country-bin_{version}.blake3");
    let manifest_path = target_dir.join(&manifest_name);
    let manifest_content: String = checksums
        .iter()
        .map(|(fname, hash)| format!("{hash}  {fname}\n"))
        .collect();
    fs::write(&manifest_path, manifest_content.as_bytes())?;

    // -------------------------
    // Detect orphaned files
    // -------------------------
    let all_existing: Vec<_> = fs::read_dir(target_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(OsStr::to_str).unwrap_or("");
            let fname = p.file_name().and_then(OsStr::to_str).unwrap_or("");
            fname != "version"
                && (ext == "iv4" || ext == "iv6" || ext == "blake3")
        })
        .collect();

    let written_set: HashSet<_> = files_to_write
        .iter()
        .chain(std::iter::once(&manifest_path))
        .cloned()
        .collect();

    let orphaned: Vec<_> = all_existing
        .into_iter()
        .filter(|p| !written_set.contains(p))
        .collect();

    if !orphaned.is_empty() {
        let orphaned_path = target_dir.join("orphaned");
        messages::warn(&format!(
            "{} orphaned files detected in \"{}\":",
            orphaned.len(),
            target_dir.display()
        ));
        for p in &orphaned {
            messages::warn(&format!("  {}", p.display()));
        }
        let list = orphaned
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        match fs::write(&orphaned_path, &list) {
            Ok(()) => messages::warn(&format!(
                "Run `xtgeoip build -c -f` or delete files listed in \"{}\" \
                 for a clean install.",
                orphaned_path.display()
            )),
            Err(e) => messages::warn(&format!(
                "Could not write orphaned file list to \"{}\": {e:#}",
                orphaned_path.display()
            )),
        }
    }

    // -------------------------
    // Summary
    // -------------------------
    messages::info(&format!("Countries processed: {}", country_name.len()));
    let ipv4_count: usize =
        country_ranges.values().map(|cr| cr.pool_v4.len()).sum();
    let ipv6_count: usize =
        country_ranges.values().map(|cr| cr.pool_v6.len()).sum();
    messages::info(&format!("IPv4 country ranges: {}", ipv4_count));
    messages::info(&format!("IPv6 country ranges: {}", ipv6_count));

    Ok(())
}

// -------------------------
// Load countries
// -------------------------
fn load_countries(
    source_dir: &Path,
    legacy: bool,
) -> anyhow::Result<(HashMap<String, String>, BTreeMap<String, String>)> {
    let file_path = source_dir.join("GeoLite2-Country-Locations-en.csv");
    let file = File::open(&file_path)?;
    let mmap = mmap_file(&file)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(mmap.as_ref());
    let headers = rdr.headers()?.clone();

    let idx_geoname = headers
        .iter()
        .position(|h| h == "geoname_id")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "geoname_id column missing in \
                 GeoLite2-Country-Locations-en.csv"
            )
        })?;
    let idx_iso = headers
        .iter()
        .position(|h| h == "country_iso_code")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "country_iso_code column missing in \
                 GeoLite2-Country-Locations-en.csv"
            )
        })?;
    let idx_name = headers
        .iter()
        .position(|h| h == "country_name")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "country_name column missing in \
                 GeoLite2-Country-Locations-en.csv"
            )
        })?;
    let idx_continent = headers
        .iter()
        .position(|h| h == "continent_code")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "continent_code column missing in \
                 GeoLite2-Country-Locations-en.csv"
            )
        })?;

    let mut country_id: HashMap<String, String> = HashMap::new();
    let mut country_name: BTreeMap<String, String> = BTreeMap::new();

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
            // Geoname 6255148 = Asia (continent), 6255147 = Europe (continent).
            // These are MaxMind CSV entries where country_iso_code is blank but
            // continent_code is set (AS or EU). Legacy mode blindly maps the
            // continent code to the country code, which creates a collision
            // between Asia (AS) and American Samoa (AS), and a
            // non-existent EU country code. Correct behaviour maps
            // these to O1 (Other Country, ISO 3166 reserved).
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
// Block index parsing (shared between v4 and v6)
// -------------------------
fn parse_block_indices(
    headers: &csv::StringRecord,
    file_name: &str,
) -> anyhow::Result<BlockIndices> {
    Ok(BlockIndices {
        net: headers.iter().position(|h| h == "network").ok_or_else(|| {
            anyhow::anyhow!("network column missing in {}", file_name)
        })?,
        id: headers.iter().position(|h| h == "geoname_id").ok_or_else(
            || anyhow::anyhow!("geoname_id column missing in {}", file_name),
        )?,
        rid: headers
            .iter()
            .position(|h| h == "registered_country_geoname_id")
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "registered_country_geoname_id column missing in {}",
                    file_name
                )
            })?,
        proxy: headers
            .iter()
            .position(|h| h == "is_anonymous_proxy")
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "is_anonymous_proxy column missing in {}",
                    file_name
                )
            })?,
        sat: headers
            .iter()
            .position(|h| h == "is_satellite_provider")
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "is_satellite_provider column missing in {}",
                    file_name
                )
            })?,
    })
}

// -------------------------
// IPv4 block loading
// -------------------------
fn load_blocks_v4(
    source_dir: &Path,
    country_id: &HashMap<String, String>,
) -> anyhow::Result<HashMap<String, Vec<(u32, u32)>>> {
    const FILE_NAME: &str = "GeoLite2-Country-Blocks-IPv4.csv";
    let file = File::open(source_dir.join(FILE_NAME))?;
    let mmap = mmap_file(&file)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(mmap.as_ref());
    let headers = rdr.headers()?.clone();
    let idx = parse_block_indices(&headers, FILE_NAME)?;

    let skipped = AtomicUsize::new(0);
    let parsed: Vec<(String, Option<(u32, u32)>)> = rdr
        .into_records()
        .par_bridge()
        .filter_map(|r| {
            r.map_err(|_| {
                skipped.fetch_add(1, Ordering::Relaxed);
            })
            .ok()
        })
        .map(|rec| {
            let id = rec.get(idx.id).unwrap_or("");
            let rid = rec.get(idx.rid).unwrap_or("");
            let proxy = rec.get(idx.proxy).unwrap_or("") == "1";
            let sat = rec.get(idx.sat).unwrap_or("") == "1";
            let network = rec.get(idx.net).unwrap_or("");
            let cc = resolve_country_code(proxy, sat, id, rid, country_id);
            let range = if network.is_empty() {
                None
            } else {
                cidr_to_range_ipv4(network)
            };
            (cc, range)
        })
        .collect();

    let n = skipped.load(Ordering::Relaxed);
    if n > 0 {
        messages::warn(&format!("{n} malformed rows skipped in {FILE_NAME}"));
    }

    let mut pools: HashMap<String, Vec<(u32, u32)>> = HashMap::new();
    for (cc, range_opt) in parsed {
        if let Some(range) = range_opt {
            pools.entry(cc).or_default().push(range);
        }
    }
    pools
        .par_iter_mut()
        .for_each(|(_, v)| *v = merge_ranges_v4(v));
    Ok(pools)
}

// -------------------------
// IPv6 block loading
// -------------------------
fn load_blocks_v6(
    source_dir: &Path,
    country_id: &HashMap<String, String>,
) -> anyhow::Result<HashMap<String, Vec<(u128, u128)>>> {
    const FILE_NAME: &str = "GeoLite2-Country-Blocks-IPv6.csv";
    let file = File::open(source_dir.join(FILE_NAME))?;
    let mmap = mmap_file(&file)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(mmap.as_ref());
    let headers = rdr.headers()?.clone();
    let idx = parse_block_indices(&headers, FILE_NAME)?;

    let skipped = AtomicUsize::new(0);
    let parsed: Vec<(String, Option<(u128, u128)>)> = rdr
        .into_records()
        .par_bridge()
        .filter_map(|r| {
            r.map_err(|_| {
                skipped.fetch_add(1, Ordering::Relaxed);
            })
            .ok()
        })
        .map(|rec| {
            let id = rec.get(idx.id).unwrap_or("");
            let rid = rec.get(idx.rid).unwrap_or("");
            let proxy = rec.get(idx.proxy).unwrap_or("") == "1";
            let sat = rec.get(idx.sat).unwrap_or("") == "1";
            let network = rec.get(idx.net).unwrap_or("");
            let cc = resolve_country_code(proxy, sat, id, rid, country_id);
            let range = if network.is_empty() {
                None
            } else {
                cidr_to_range_ipv6(network)
            };
            (cc, range)
        })
        .collect();

    let n = skipped.load(Ordering::Relaxed);
    if n > 0 {
        messages::warn(&format!("{n} malformed rows skipped in {FILE_NAME}"));
    }

    let mut pools: HashMap<String, Vec<(u128, u128)>> = HashMap::new();
    for (cc, range_opt) in parsed {
        if let Some(range) = range_opt {
            pools.entry(cc).or_default().push(range);
        }
    }
    pools
        .par_iter_mut()
        .for_each(|(_, v)| *v = merge_ranges_v6(v));
    Ok(pools)
}

fn resolve_country_code(
    proxy: bool,
    sat: bool,
    id: &str,
    rid: &str,
    country_id: &HashMap<String, String>,
) -> String {
    if proxy {
        return "A1".to_string();
    }
    if sat {
        return "A2".to_string();
    }
    let key = if !id.is_empty() { id } else { rid };
    if key.is_empty() {
        return "O1".to_string();
    }
    country_id
        .get(key)
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| "O1".to_string())
}

// -------------------------
// CIDR → Range
// -------------------------
fn cidr_to_range_ipv4(cidr: &str) -> Option<(u32, u32)> {
    let net: IpNetwork = cidr.parse().ok()?;
    match net {
        IpNetwork::V4(v4) => {
            Some((u32::from(v4.network()), u32::from(v4.broadcast())))
        }
        _ => None,
    }
}

fn cidr_to_range_ipv6(cidr: &str) -> Option<(u128, u128)> {
    let net: IpNetwork = cidr.parse().ok()?;
    match net {
        IpNetwork::V6(v6) => {
            Some((u128::from(v6.network()), u128::from(v6.broadcast())))
        }
        _ => None,
    }
}

// -------------------------
// Merge ranges
// -------------------------
fn merge_ranges_v4(ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    if ranges.is_empty() {
        return vec![];
    }
    let mut sorted = ranges.to_vec();
    sorted.sort_unstable_by_key(|r| r.0);
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(sorted.len());
    for &(start, end) in &sorted {
        if let Some(last) = merged.last_mut()
            && start <= last.1.saturating_add(1)
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn merge_ranges_v6(ranges: &[(u128, u128)]) -> Vec<(u128, u128)> {
    if ranges.is_empty() {
        return vec![];
    }
    let mut sorted = ranges.to_vec();
    sorted.sort_unstable_by_key(|r| r.0);
    let mut merged: Vec<(u128, u128)> = Vec::with_capacity(sorted.len());
    for &(start, end) in &sorted {
        if let Some(last) = merged.last_mut()
            && start <= last.1.saturating_add(1)
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

// -------------------------
// mmap helper
// -------------------------
fn mmap_file(file: &File) -> anyhow::Result<Mmap> {
    // Safety: caller must not mutate the file while the mapping is live
    Ok(unsafe { Mmap::map(file)? })
}

// -------------------------
// Write country files: pre-built buffer, single syscall, blake3 hash
// -------------------------
fn write_country_v4(
    file_base: &Path,
    ranges: &[(u32, u32)],
) -> anyhow::Result<(String, String)> {
    let file_path = file_base.with_extension("iv4");
    let mut buf = Vec::with_capacity(ranges.len() * 8);
    for &(start, end) in ranges {
        buf.extend_from_slice(&start.to_be_bytes());
        buf.extend_from_slice(&end.to_be_bytes());
    }
    let hash = blake3::hash(&buf).to_string();
    fs::write(&file_path, &buf)?;
    let fname = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Ok((fname, hash))
}

fn write_country_v6(
    file_base: &Path,
    ranges: &[(u128, u128)],
) -> anyhow::Result<(String, String)> {
    let file_path = file_base.with_extension("iv6");
    let mut buf = Vec::with_capacity(ranges.len() * 16);
    for &(start, end) in ranges {
        buf.extend_from_slice(&start.to_be_bytes());
        buf.extend_from_slice(&end.to_be_bytes());
    }
    let hash = blake3::hash(&buf).to_string();
    fs::write(&file_path, &buf)?;
    let fname = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Ok((fname, hash))
}
