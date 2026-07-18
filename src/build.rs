/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::bail;
use csv::ReaderBuilder;
use ipnetwork::IpNetwork;
use memmap2::Mmap;
use rayon::prelude::*;

use crate::{messages, version::Version};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum CountryCode {
    Iso([u8; 2]),
    A1,
    A2,
    O1,
}

impl CountryCode {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "A1" => Some(Self::A1),
            "A2" => Some(Self::A2),
            "O1" => Some(Self::O1),
            _ => {
                let b = s.as_bytes();
                if b.len() == 2
                    && b[0].is_ascii_alphabetic()
                    && b[1].is_ascii_alphabetic()
                {
                    Some(Self::Iso([
                        b[0].to_ascii_uppercase(),
                        b[1].to_ascii_uppercase(),
                    ]))
                } else {
                    None
                }
            }
        }
    }
}

impl std::fmt::Display for CountryCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Iso(b) => write!(f, "{}{}", b[0] as char, b[1] as char),
            Self::A1 => write!(f, "A1"),
            Self::A2 => write!(f, "A2"),
            Self::O1 => write!(f, "O1"),
        }
    }
}

pub fn build(
    source_dir: &Path,
    target_dir: &Path,
    version: &Version,
    legacy: bool,
) -> anyhow::Result<()> {
    if legacy {
        messages::warn(
            "Legacy Mode activated. See documentation for collisions.",
        );
    }

    let (country_id, mut country_name) = load_countries(source_dir, legacy)?;
    country_name
        .entry(CountryCode::A1)
        .or_insert_with(|| "Anonymous Proxy".into());
    country_name
        .entry(CountryCode::A2)
        .or_insert_with(|| "Satellite Provider".into());
    country_name
        .entry(CountryCode::O1)
        .or_insert_with(|| "Other Country".into());

    let country_count = country_name.len();
    let (v4_result, v6_result) = rayon::join(
        || load_blocks_v4(source_dir, &country_id, country_count),
        || load_blocks_v6(source_dir, &country_id, country_count),
    );
    let v4_pools = v4_result?;
    let v6_pools = v6_result?;

    let mut country_ranges: BTreeMap<CountryCode, CountryRanges> = country_name
        .keys()
        .map(|&cc| (cc, CountryRanges::default()))
        .collect();
    for cc in [CountryCode::A1, CountryCode::A2, CountryCode::O1] {
        country_ranges.entry(cc).or_default();
    }
    for (cc, pool) in v4_pools {
        country_ranges.entry(cc).or_default().pool_v4 = pool;
    }
    for (cc, pool) in v6_pools {
        country_ranges.entry(cc).or_default().pool_v6 = pool;
    }

    let (written_paths, checksums) =
        write_outputs(&country_ranges, target_dir)?;
    let manifest_path = generate_manifest(target_dir, version, checksums)?;
    detect_orphans(target_dir, &written_paths, &manifest_path)?;

    messages::info(&format!("Countries processed: {}", country_count));
    let ipv4_count: usize =
        country_ranges.values().map(|cr| cr.pool_v4.len()).sum();
    let ipv6_count: usize =
        country_ranges.values().map(|cr| cr.pool_v6.len()).sum();
    messages::info(&format!("IPv4 country ranges: {}", ipv4_count));
    messages::info(&format!("IPv6 country ranges: {}", ipv6_count));

    Ok(())
}

type WriteOutputs = (Vec<PathBuf>, Vec<(String, String)>);

fn write_outputs(
    country_ranges: &BTreeMap<CountryCode, CountryRanges>,
    target_dir: &Path,
) -> anyhow::Result<WriteOutputs> {
    fs::create_dir_all(target_dir)?;

    let files_to_write: Vec<_> = country_ranges
        .keys()
        .flat_map(|cc| {
            let base = target_dir.join(cc.to_string());
            vec![base.with_extension("iv4"), base.with_extension("iv6")]
        })
        .collect();

    let overwrite_count = files_to_write.iter().filter(|f| f.exists()).count();
    if overwrite_count > 0 {
        messages::warn(&format!(
            "{} country files (iv4/iv6) will be overwritten.",
            overwrite_count
        ));
    }

    let write_results: Vec<anyhow::Result<(String, String)>> = country_ranges
        .par_iter()
        .flat_map(|(cc, cr)| {
            let base = target_dir.join(cc.to_string());
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

    Ok((files_to_write, checksums))
}

fn generate_manifest(
    target_dir: &Path,
    version: &Version,
    checksums: Vec<(String, String)>,
) -> anyhow::Result<PathBuf> {
    fs::write(target_dir.join("version"), format!("{version}\n"))?;

    let manifest_name = version.bin_manifest_name();
    let manifest_path = target_dir.join(&manifest_name);
    let manifest_content: String = checksums
        .iter()
        .map(|(fname, hash)| format!("{hash}  {fname}\n"))
        .collect();
    fs::write(&manifest_path, manifest_content.as_bytes())?;

    Ok(manifest_path)
}

fn detect_orphans(
    target_dir: &Path,
    written: &[PathBuf],
    manifest_path: &Path,
) -> anyhow::Result<()> {
    let all_existing: Vec<_> = fs::read_dir(target_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(OsStr::to_str).unwrap_or("");
            let fname = p.file_name().and_then(OsStr::to_str).unwrap_or("");
            fname != "version"
                && (ext == "iv4"
                    || ext == "iv6"
                    || ext == "blake3"
                    || ext == "sha256")
        })
        .collect();

    let mut written_set: HashSet<PathBuf> =
        HashSet::with_capacity(written.len() + 1);
    written_set.extend(written.iter().cloned());
    written_set.insert(manifest_path.to_path_buf());

    let orphaned: Vec<_> = all_existing
        .into_iter()
        .filter(|p| !written_set.contains(p))
        .collect();

    if orphaned.is_empty() {
        return Ok(());
    }

    // Stale manifests (.blake3/.sha256) are unconditionally superseded by
    // the new manifest — delete them silently.
    let (stale_manifests, stale_iv): (Vec<_>, Vec<_>) =
        orphaned.into_iter().partition(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("blake3") | Some("sha256")
            )
        });

    for path in &stale_manifests {
        if let Err(e) = fs::remove_file(path) {
            messages::warn(&format!(
                "Failed to delete stale manifest {}: {e:#}",
                path.display()
            ));
        }
    }

    // Orphaned iv4/iv6 files require user action (e.g. legacy→normal
    // mode transition leaving EU.iv4/EU.iv6 behind).
    if !stale_iv.is_empty() {
        let orphaned_path = target_dir.join("orphaned");
        messages::warn(&format!(
            "{} orphaned files detected in \"{}\":",
            stale_iv.len(),
            target_dir.display()
        ));
        for p in &stale_iv {
            messages::warn(&format!("  {}", p.display()));
        }
        let list = stale_iv
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

    Ok(())
}

// -------------------------
// Load countries
// -------------------------
fn load_countries(
    source_dir: &Path,
    legacy: bool,
) -> anyhow::Result<(HashMap<String, CountryCode>, BTreeMap<CountryCode, String>)>
{
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

    let mut country_id: HashMap<String, CountryCode> = HashMap::new();
    let mut country_name: BTreeMap<CountryCode, String> = BTreeMap::new();

    for record in rdr.records() {
        let rec = record?;
        let geoname = rec.get(idx_geoname).unwrap_or("").to_string();
        let name = rec.get(idx_name).unwrap_or("").to_string();

        if let Some(cc) = CountryCode::parse(rec.get(idx_iso).unwrap_or("")) {
            country_id.insert(geoname, cc);
            country_name.entry(cc).or_insert(name);
        } else if geoname == "6255148" || geoname == "6255147" {
            // Geoname 6255148 = Asia (continent), 6255147 = Europe (continent).
            // These are MaxMind CSV entries where country_iso_code is blank but
            // continent_code is set (AS or EU). Legacy mode blindly maps the
            // continent code to the country code, which creates a collision
            // between Asia (AS) and American Samoa (AS), and a
            // non-existent EU country code. Correct behaviour maps
            // these to O1 (Other Country, ISO 3166 reserved).
            let cc = if legacy {
                CountryCode::parse(rec.get(idx_continent).unwrap_or(""))
                    .unwrap_or(CountryCode::O1)
            } else {
                CountryCode::O1
            };
            country_id.insert(geoname, cc);
            country_name.entry(cc).or_insert(name);
        } else {
            country_id.insert(geoname, CountryCode::O1);
            country_name.entry(CountryCode::O1).or_insert(name);
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
    country_id: &HashMap<String, CountryCode>,
    country_count: usize,
) -> anyhow::Result<HashMap<CountryCode, Vec<(u32, u32)>>> {
    const FILE_NAME: &str = "GeoLite2-Country-Blocks-IPv4.csv";
    let file = File::open(source_dir.join(FILE_NAME))?;
    let mmap = mmap_file(&file)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(mmap.as_ref());
    let headers = rdr.headers()?.clone();
    let idx = parse_block_indices(&headers, FILE_NAME)?;

    let skipped = AtomicUsize::new(0);
    // Rows without a usable range are dropped here rather than carried into
    // `parsed` and skipped during grouping (#38): the `Option` cost 4 bytes
    // per element on this path and a branch per row in the loop below.
    let parsed: Vec<(CountryCode, (u32, u32))> = rdr
        .into_records()
        .par_bridge()
        .filter_map(|r| {
            r.map_err(|_| {
                skipped.fetch_add(1, Ordering::Relaxed);
            })
            .ok()
        })
        .filter_map(|rec| {
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
            range.map(|r| (cc, r))
        })
        .collect();

    let n = skipped.load(Ordering::Relaxed);
    if n > 0 {
        messages::warn(&format!("{n} malformed rows skipped in {FILE_NAME}"));
    }

    let mut pools: HashMap<CountryCode, Vec<(u32, u32)>> =
        HashMap::with_capacity(country_count);
    for (cc, range) in parsed {
        pools.entry(cc).or_default().push(range);
    }
    pools.par_iter_mut().for_each(|(_, v)| *v = merge_ranges(v));
    Ok(pools)
}

// -------------------------
// IPv6 block loading
// -------------------------
fn load_blocks_v6(
    source_dir: &Path,
    country_id: &HashMap<String, CountryCode>,
    country_count: usize,
) -> anyhow::Result<HashMap<CountryCode, Vec<(u128, u128)>>> {
    const FILE_NAME: &str = "GeoLite2-Country-Blocks-IPv6.csv";
    let file = File::open(source_dir.join(FILE_NAME))?;
    let mmap = mmap_file(&file)?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .from_reader(mmap.as_ref());
    let headers = rdr.headers()?.clone();
    let idx = parse_block_indices(&headers, FILE_NAME)?;

    let skipped = AtomicUsize::new(0);
    // As in load_blocks_v4, but the saving is larger here: `Option` around a
    // `(u128, u128)` costs 16 bytes at 16-byte alignment, so dropping it
    // takes each element from 64 to 48 bytes — ~25% of this path's transient
    // allocation, which is the larger of the two (#38).
    let parsed: Vec<(CountryCode, (u128, u128))> = rdr
        .into_records()
        .par_bridge()
        .filter_map(|r| {
            r.map_err(|_| {
                skipped.fetch_add(1, Ordering::Relaxed);
            })
            .ok()
        })
        .filter_map(|rec| {
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
            range.map(|r| (cc, r))
        })
        .collect();

    let n = skipped.load(Ordering::Relaxed);
    if n > 0 {
        messages::warn(&format!("{n} malformed rows skipped in {FILE_NAME}"));
    }

    let mut pools: HashMap<CountryCode, Vec<(u128, u128)>> =
        HashMap::with_capacity(country_count);
    for (cc, range) in parsed {
        pools.entry(cc).or_default().push(range);
    }
    pools.par_iter_mut().for_each(|(_, v)| *v = merge_ranges(v));
    Ok(pools)
}

fn resolve_country_code(
    proxy: bool,
    sat: bool,
    id: &str,
    rid: &str,
    country_id: &HashMap<String, CountryCode>,
) -> CountryCode {
    if proxy {
        return CountryCode::A1;
    }
    if sat {
        return CountryCode::A2;
    }
    let key = if !id.is_empty() { id } else { rid };
    if key.is_empty() {
        return CountryCode::O1;
    }
    country_id.get(key).copied().unwrap_or(CountryCode::O1)
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
trait IpInt: Copy + Ord {
    fn saturating_inc(self) -> Self;
}
impl IpInt for u32 {
    fn saturating_inc(self) -> u32 {
        self.saturating_add(1)
    }
}
impl IpInt for u128 {
    fn saturating_inc(self) -> u128 {
        self.saturating_add(1)
    }
}

fn merge_ranges<T: IpInt>(ranges: &[(T, T)]) -> Vec<(T, T)> {
    if ranges.is_empty() {
        return vec![];
    }
    let mut sorted = ranges.to_vec();
    sorted.sort_unstable_by_key(|r| r.0);
    let mut merged: Vec<(T, T)> = Vec::with_capacity(sorted.len());
    for &(start, end) in &sorted {
        if let Some(last) = merged.last_mut()
            && start <= last.1.saturating_inc()
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
    let mut hasher = blake3::Hasher::new();
    for &(start, end) in ranges {
        let s = start.to_be_bytes();
        let e = end.to_be_bytes();
        buf.extend_from_slice(&s);
        buf.extend_from_slice(&e);
        hasher.update(&s);
        hasher.update(&e);
    }
    let hash = hasher.finalize().to_string();
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
    let mut buf = Vec::with_capacity(ranges.len() * 32);
    let mut hasher = blake3::Hasher::new();
    for &(start, end) in ranges {
        let s = start.to_be_bytes();
        let e = end.to_be_bytes();
        buf.extend_from_slice(&s);
        buf.extend_from_slice(&e);
        hasher.update(&s);
        hasher.update(&e);
    }
    let hash = hasher.finalize().to_string();
    fs::write(&file_path, &buf)?;
    let fname = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Ok((fname, hash))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    };

    use tempfile::TempDir;

    use super::*;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, b"").unwrap();
        p
    }

    // ── CountryCode ───────────────────────────────────────

    #[test]
    fn country_code_parse_iso_uppercase() {
        assert_eq!(
            CountryCode::parse("US"),
            Some(CountryCode::Iso([b'U', b'S']))
        );
    }

    #[test]
    fn country_code_parse_iso_lowercase_normalised() {
        assert_eq!(
            CountryCode::parse("us"),
            Some(CountryCode::Iso([b'U', b'S']))
        );
    }

    #[test]
    fn country_code_parse_special_a1() {
        assert_eq!(CountryCode::parse("A1"), Some(CountryCode::A1));
    }

    #[test]
    fn country_code_parse_special_a2() {
        assert_eq!(CountryCode::parse("A2"), Some(CountryCode::A2));
    }

    #[test]
    fn country_code_parse_special_o1() {
        assert_eq!(CountryCode::parse("O1"), Some(CountryCode::O1));
    }

    #[test]
    fn country_code_parse_rejects_empty() {
        assert!(CountryCode::parse("").is_none());
    }

    #[test]
    fn country_code_parse_rejects_single_char() {
        assert!(CountryCode::parse("U").is_none());
    }

    #[test]
    fn country_code_parse_rejects_digit_prefix() {
        assert!(CountryCode::parse("1S").is_none());
    }

    #[test]
    fn country_code_display_iso() {
        assert_eq!(CountryCode::Iso([b'G', b'B']).to_string(), "GB");
    }

    #[test]
    fn country_code_display_specials() {
        assert_eq!(CountryCode::A1.to_string(), "A1");
        assert_eq!(CountryCode::A2.to_string(), "A2");
        assert_eq!(CountryCode::O1.to_string(), "O1");
    }

    // ── merge_ranges ──────────────────────────────────────

    #[test]
    fn merge_ranges_empty() {
        let out: Vec<(u32, u32)> = merge_ranges(&[]);
        assert!(out.is_empty());
    }

    #[test]
    fn merge_ranges_single() {
        assert_eq!(merge_ranges(&[(10u32, 20u32)]), vec![(10, 20)]);
    }

    #[test]
    fn merge_ranges_adjacent_merged() {
        // 5.saturating_inc() == 6 == start of next range → single span
        assert_eq!(merge_ranges(&[(0u32, 5u32), (6u32, 10u32)]), vec![(0, 10)]);
    }

    #[test]
    fn merge_ranges_overlapping_merged() {
        assert_eq!(
            merge_ranges(&[(0u32, 10u32), (5u32, 15u32)]),
            vec![(0, 15)]
        );
    }

    #[test]
    fn merge_ranges_disjoint_preserved() {
        // 5.saturating_inc() == 6 < 7 → gap, no merge
        assert_eq!(
            merge_ranges(&[(0u32, 5u32), (7u32, 10u32)]),
            vec![(0, 5), (7, 10)]
        );
    }

    #[test]
    fn merge_ranges_unsorted_input() {
        assert_eq!(
            merge_ranges(&[(7u32, 10u32), (0u32, 5u32)]),
            vec![(0, 5), (7, 10)]
        );
    }

    #[test]
    fn merge_ranges_u32_max_no_overflow() {
        let hi = u32::MAX;
        assert_eq!(merge_ranges(&[(hi - 1, hi)]), vec![(hi - 1, hi)]);
    }

    // ── cidr_to_range_ipv4 ────────────────────────────────

    #[test]
    fn cidr_ipv4_slash24() {
        let net = u32::from(std::net::Ipv4Addr::new(192, 168, 1, 0));
        let bcast = u32::from(std::net::Ipv4Addr::new(192, 168, 1, 255));
        assert_eq!(cidr_to_range_ipv4("192.168.1.0/24"), Some((net, bcast)));
    }

    #[test]
    fn cidr_ipv4_slash32_host() {
        let addr = u32::from(std::net::Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(cidr_to_range_ipv4("10.0.0.1/32"), Some((addr, addr)));
    }

    #[test]
    fn cidr_ipv4_invalid_returns_none() {
        assert!(cidr_to_range_ipv4("not-a-cidr").is_none());
    }

    #[test]
    fn cidr_ipv4_rejects_v6_cidr() {
        assert!(cidr_to_range_ipv4("::1/128").is_none());
    }

    // ── cidr_to_range_ipv6 ────────────────────────────────

    #[test]
    fn cidr_ipv6_slash128_host() {
        let addr = u128::from(std::net::Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
        assert_eq!(cidr_to_range_ipv6("::1/128"), Some((addr, addr)));
    }

    #[test]
    fn cidr_ipv6_slash64() {
        let net = u128::from(std::net::Ipv6Addr::new(
            0x2001, 0x0db8, 0, 0, 0, 0, 0, 0,
        ));
        let bcast = u128::from(std::net::Ipv6Addr::new(
            0x2001, 0x0db8, 0, 0, 0xffff, 0xffff, 0xffff, 0xffff,
        ));
        assert_eq!(cidr_to_range_ipv6("2001:db8::/64"), Some((net, bcast)));
    }

    #[test]
    fn cidr_ipv6_invalid_returns_none() {
        assert!(cidr_to_range_ipv6("garbage").is_none());
    }

    #[test]
    fn cidr_ipv6_rejects_v4_cidr() {
        assert!(cidr_to_range_ipv6("1.2.3.4/8").is_none());
    }

    // ── resolve_country_code ──────────────────────────────

    fn make_map(pairs: &[(&str, CountryCode)]) -> HashMap<String, CountryCode> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn resolve_proxy_returns_a1() {
        let m = make_map(&[("1", CountryCode::Iso([b'U', b'S']))]);
        assert_eq!(
            resolve_country_code(true, false, "1", "", &m),
            CountryCode::A1
        );
    }

    #[test]
    fn resolve_sat_returns_a2() {
        let m = make_map(&[]);
        assert_eq!(
            resolve_country_code(false, true, "", "", &m),
            CountryCode::A2
        );
    }

    #[test]
    fn resolve_proxy_beats_sat() {
        let m = make_map(&[]);
        assert_eq!(
            resolve_country_code(true, true, "", "", &m),
            CountryCode::A1
        );
    }

    #[test]
    fn resolve_id_lookup() {
        let de = CountryCode::Iso([b'D', b'E']);
        let m = make_map(&[("42", de)]);
        assert_eq!(resolve_country_code(false, false, "42", "", &m), de);
    }

    #[test]
    fn resolve_rid_fallback_when_id_empty() {
        let fr = CountryCode::Iso([b'F', b'R']);
        let m = make_map(&[("99", fr)]);
        assert_eq!(resolve_country_code(false, false, "", "99", &m), fr);
    }

    #[test]
    fn resolve_empty_geoname_returns_o1() {
        let m = make_map(&[]);
        assert_eq!(
            resolve_country_code(false, false, "", "", &m),
            CountryCode::O1
        );
    }

    #[test]
    fn resolve_unknown_id_returns_o1() {
        let m = make_map(&[]);
        assert_eq!(
            resolve_country_code(false, false, "999", "", &m),
            CountryCode::O1
        );
    }

    // ── detect_orphans ────────────────────────────────────

    #[test]
    fn detect_orphans_clean_run() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let iv4 = touch(p, "US.iv4");
        let iv6 = touch(p, "US.iv6");
        let manifest = touch(p, "GeoLite2-Country-bin_20260101.blake3");
        detect_orphans(p, &[iv4, iv6], &manifest).unwrap();
        assert!(!p.join("orphaned").exists());
    }

    #[test]
    fn detect_orphans_foreign_file_untouched() {
        // Regression: files with extensions outside iv4/iv6/blake3/sha256
        // must be structurally invisible to detect_orphans.
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let conf = touch(p, "xtgeoip.conf.example");
        let manifest = touch(p, "GeoLite2-Country-bin_20260101.blake3");
        detect_orphans(p, &[], &manifest).unwrap();
        assert!(conf.exists(), "foreign file must survive detect_orphans");
    }

    #[test]
    fn detect_orphans_version_file_untouched() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let ver = touch(p, "version");
        let manifest = touch(p, "GeoLite2-Country-bin_20260101.blake3");
        detect_orphans(p, &[], &manifest).unwrap();
        assert!(
            ver.exists(),
            "version file must not be touched by detect_orphans"
        );
    }

    #[test]
    fn detect_orphans_stale_blake3_deleted() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let old = touch(p, "GeoLite2-Country-bin_20260101.blake3");
        let new_manifest = touch(p, "GeoLite2-Country-bin_20260606.blake3");
        detect_orphans(p, &[], &new_manifest).unwrap();
        assert!(!old.exists(), "stale blake3 manifest must be deleted");
        assert!(new_manifest.exists());
    }

    #[test]
    fn detect_orphans_stale_sha256_deleted() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let old = touch(p, "GeoLite2-Country-bin_20260101.sha256");
        let manifest = touch(p, "GeoLite2-Country-bin_20260606.blake3");
        detect_orphans(p, &[], &manifest).unwrap();
        assert!(!old.exists(), "stale sha256 manifest must be deleted");
    }

    #[test]
    fn detect_orphans_orphaned_iv_listed_not_deleted() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let stale = touch(p, "EU.iv4");
        let manifest = touch(p, "GeoLite2-Country-bin_20260606.blake3");
        detect_orphans(p, &[], &manifest).unwrap();
        assert!(stale.exists(), "orphaned iv4 must not be deleted");
        assert!(
            p.join("orphaned").exists(),
            "orphaned list file must be created"
        );
    }
}
