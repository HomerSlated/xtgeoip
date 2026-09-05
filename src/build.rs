/// xtgeoip © Haze N Sparkle 2026 (MIT)
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsStr,
    fs::{self, File},
    net::Ipv6Addr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::bail;
use csv::ReaderBuilder;
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
    let index = CountryIndex::new(&country_id, &country_name);
    let (v4_result, v6_result) = rayon::join(
        || load_blocks(source_dir, BLOCKS_V4, &index, cidr_v4_bytes),
        || load_blocks(source_dir, BLOCKS_V6, &index, cidr_v6_bytes),
    );
    let v4_pools = v4_result?;
    let v6_pools = v6_result?;

    // `index.order` is `country_name`'s key order by construction, so zipping
    // the two dense pools against it reproduces exactly the `BTreeMap` the
    // per-code `HashMap`s used to build — including the countries with no
    // ranges at all, which still get their (empty) `.iv4`/`.iv6` pair.
    let country_ranges: BTreeMap<CountryCode, CountryRanges> = index
        .order
        .iter()
        .zip(v4_pools)
        .zip(v6_pools)
        .map(|((&cc, pool_v4), pool_v6)| {
            (cc, CountryRanges { pool_v4, pool_v6 })
        })
        .collect();

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
        bail!(
            "{} file write(s) failed during build.\n{} country file(s) were \
             written before the failure, so {} now holds a partially written \
             database while `version` and the manifest still describe the \
             previous one. Operations that verify before touching files (a \
             backup or a clean without -f) will refuse to run while the two \
             disagree. Fix the cause and re-run: a successful build rewrites \
             every country file and regenerates the manifest.",
            write_errors.len(),
            checksums.len(),
            target_dir.display()
        );
    }
    checksums.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    Ok((files_to_write, checksums))
}

/// Write the manifest, then the `version` file that points at it.
///
/// The order is the whole point. `version` is a pointer: `backup::gather_files`
/// in `Verified` mode reads it, derives the manifest name from it, and aborts
/// with "Manifest missing … Use -f to force" when that file is absent. Writing
/// the pointer first meant a failure between the two writes left `version`
/// naming a manifest that was never written — a dangling pointer that blocks
/// every verified operation on data that is otherwise intact.
///
/// In this order the same failure leaves the *previous* `version` and the
/// *previous* manifest, which still agree with each other, plus an unreferenced
/// new manifest. Verified reads follow the old pointer and never see it; the
/// stray file is swept up by `detect_orphans` on the next successful build, and
/// collected by `all_blake3_files` on the force path.
///
/// This is not an atomic swap (#24 stages 2–3, rejected — `b4ec1db` lost data).
/// Nothing is renamed, staged, or rolled back; one write simply precedes the
/// other.
fn generate_manifest(
    target_dir: &Path,
    version: &Version,
    checksums: Vec<(String, String)>,
) -> anyhow::Result<PathBuf> {
    let manifest_name = version.bin_manifest_name();
    let manifest_path = target_dir.join(&manifest_name);
    let manifest_content: String = checksums
        .iter()
        .map(|(fname, hash)| format!("{hash}  {fname}\n"))
        .collect();
    fs::write(&manifest_path, manifest_content.as_bytes())?;

    fs::write(target_dir.join("version"), format!("{version}\n"))?;

    Ok(manifest_path)
}

/// Could this program have written this file?
///
/// The man page's FILE OWNERSHIP section promises that unowned files are
/// "**never** touched, by any operation", and says the guarantee is
/// "enforced structurally, not by convention". That was true of the clean
/// path (`backup::iv_files` applies the two-character stem test) but was
/// not applied here: `detect_orphans` selected on extension alone, so an
/// operator's own `checksums.sha256` or a packaging step's `SHA256SUMS.sha256`
/// in `output_dir` was classified as a stale manifest and deleted silently
/// by the next `build`. The documented exception is narrower than the code
/// was — it covers `*.blake3`/`*.sha256` *from an earlier build*, not every
/// file with those extensions.
///
/// The two arms mirror the two things this program writes:
///
/// * data files — `<CC>.iv4` / `<CC>.iv6`, stem exactly two characters from
///   `[A-Z0-9]`, the same test `backup::iv_files` uses;
/// * manifests — `GeoLite2-Country-bin_<version>.blake3`, plus the legacy
///   `.sha256` spelling, both produced by `Version::bin_manifest_name`.
///
/// The `version` file is owned but is never an orphan (it is rewritten on
/// every build), so it is excluded here rather than special-cased below.
fn is_ours(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    let Some(ext) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };
    let stem = &name[..name.len() - ext.len() - 1];

    match ext {
        "iv4" | "iv6" => {
            stem.len() == 2
                && stem
                    .chars()
                    .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
        }
        "blake3" | "sha256" => {
            stem.starts_with("GeoLite2-Country-bin_")
                && Version::parse(name).is_some()
        }
        _ => false,
    }
}

fn detect_orphans(
    target_dir: &Path,
    written: &[PathBuf],
    manifest_path: &Path,
) -> anyhow::Result<()> {
    let all_existing: Vec<_> = fs::read_dir(target_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_ours(p))
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

const BLOCKS_V4: &str = "GeoLite2-Country-Blocks-IPv4.csv";
const BLOCKS_V6: &str = "GeoLite2-Country-Blocks-IPv6.csv";

/// Byte ranges the blocks CSV is split into, per rayon worker.
///
/// More ranges than workers, so a chunk that happens to hold denser rows
/// cannot leave a core idle at the tail. Swept on this 2-core host over the
/// real 2026-09-01 archive, whole-`build` min of 5 interleaved runs:
///
/// | per thread | 1 | 2 | **4** | 8 | 16 |
/// |---|---:|---:|---:|---:|---:|
/// | ms | 300.0 | 287.4 | **260.6** | 269.7 | 255.9 |
///
/// One range per worker is the clear loser; past four the curve is flat and
/// the remaining spread is inside the run-to-run noise on this box, so four
/// is chosen as the knee rather than as a measured optimum.
///
/// It is a work-splitting knob, not a correctness one: every value above was
/// verified to emit a byte-identical output tree.
const CHUNKS_PER_THREAD: usize = 4;

/// Country lookup, in the two shapes the loaders need.
///
/// Replaces the `HashMap<String, CountryCode>` that the per-row path used to
/// probe ~1.09M times. Two things change: the key is an integer rather than a
/// string (no SipHash over the geoname digits) and the value is a dense index
/// rather than a code, so a chunk can accumulate straight into
/// `Vec<Vec<_>>` by position instead of hashing a `CountryCode` per row.
struct CountryIndex {
    /// Dense index → code, in `BTreeMap` key order. This ordering is
    /// load-bearing: `build` turns it back into the `BTreeMap` whose order
    /// decides output file names and manifest order.
    order: Vec<CountryCode>,
    /// Sorted `(geoname_id, dense index)`. Keys are unique because
    /// `parse_geoname` accepts only canonical decimal, which is injective.
    by_id: Vec<(u32, u16)>,
    /// Geonames `parse_geoname` rejects — non-numeric, out of `u32` range, or
    /// written with a leading zero. Real archives have none, so this is
    /// normally empty and never probed. It exists because the dense table
    /// cannot represent such a key, and quietly resolving one to `O1` would
    /// be a wrong *country* rather than a missing one.
    by_str: HashMap<String, u16>,
    a1: u16,
    a2: u16,
    o1: u16,
}

/// Parse a geoname ID, canonical decimal only.
///
/// Two rules beyond "digits", both there to keep the dense table faithful to
/// the `HashMap<String, _>` it replaces:
///
/// * **Checked arithmetic.** A geoname above `u32::MAX` must not wrap — a
///   wrapped value can land on a *real* geoname in the sorted table and resolve
///   to the wrong country with no error at all. The string map simply missed
///   and yielded `O1`.
/// * **No leading zeros.** `"0123"` and `"123"` are distinct `HashMap` keys but
///   the same integer. Rejecting the non-canonical spelling on both the build
///   side and the lookup side keeps them distinct here too.
#[inline]
fn parse_geoname(b: &[u8]) -> Option<u32> {
    if b.is_empty() || b.len() > 10 || (b.len() > 1 && b[0] == b'0') {
        return None;
    }
    let mut v: u32 = 0;
    for &c in b {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u32::from(c - b'0'))?;
    }
    Some(v)
}

impl CountryIndex {
    fn new(
        country_id: &HashMap<String, CountryCode>,
        country_name: &BTreeMap<CountryCode, String>,
    ) -> Self {
        let order: Vec<CountryCode> = country_name.keys().copied().collect();
        let idx_of: HashMap<CountryCode, u16> = order
            .iter()
            .enumerate()
            .map(|(i, &c)| (c, i as u16))
            .collect();

        let mut by_id: Vec<(u32, u16)> = Vec::with_capacity(country_id.len());
        let mut by_str: HashMap<String, u16> = HashMap::new();
        for (geoname, cc) in country_id {
            // Every code in `country_id` was inserted into `country_name`
            // alongside it, so the index always exists.
            let i = idx_of[cc];
            match parse_geoname(geoname.as_bytes()) {
                Some(n) => by_id.push((n, i)),
                None => {
                    by_str.insert(geoname.clone(), i);
                }
            }
        }
        by_id.sort_unstable();

        Self {
            order,
            by_id,
            by_str,
            a1: idx_of[&CountryCode::A1],
            a2: idx_of[&CountryCode::A2],
            o1: idx_of[&CountryCode::O1],
        }
    }

    /// The dense index for one block row.
    ///
    /// Mirrors [`resolve_country_code`], which is retained under `cfg(test)`
    /// as the oracle this is checked against.
    #[inline]
    fn resolve(&self, proxy: bool, sat: bool, id: &[u8], rid: &[u8]) -> u16 {
        if proxy {
            return self.a1;
        }
        if sat {
            return self.a2;
        }
        let key = if !id.is_empty() { id } else { rid };
        if key.is_empty() {
            return self.o1;
        }
        match parse_geoname(key) {
            Some(n) => match self.by_id.binary_search_by_key(&n, |e| e.0) {
                Ok(p) => self.by_id[p].1,
                Err(_) => self.o1,
            },
            None => std::str::from_utf8(key)
                .ok()
                .and_then(|k| self.by_str.get(k).copied())
                .unwrap_or(self.o1),
        }
    }
}

/// Split `bytes` after the header line into `n` ranges, each ending on a
/// newline.
///
/// Sound only for a quote-free file; see the guard in [`load_blocks`].
fn chunk_bounds(bytes: &[u8], n: usize) -> Vec<(usize, usize)> {
    let Some(nl) = memchr::memchr(b'\n', bytes) else {
        return vec![];
    };
    let start = nl + 1;
    if start >= bytes.len() {
        return vec![];
    }
    let per = (bytes.len() - start) / n.max(1) + 1;
    let mut out = Vec::with_capacity(n);
    let mut s = start;
    while s < bytes.len() {
        let mut e = (s + per).min(bytes.len());
        while e < bytes.len() && bytes[e - 1] != b'\n' {
            e += 1;
        }
        out.push((s, e));
        s = e;
    }
    out
}

// -------------------------
// Block loading
// -------------------------

/// Parse one blocks CSV into per-country range pools, indexed densely.
///
/// The shape, and why it is not the `par_bridge` over `StringRecord` it
/// replaced:
///
/// * **Chunked, not bridged.** `par_bridge` pulls rows from one sequential
///   iterator through a mutex, so the csv reader itself stays serial. Splitting
///   the mmap into byte ranges gives each worker its own reader, and scales
///   past the two-way `rayon::join` in [`build`].
/// * **`ByteRecord`, reused.** The old path allocated a `StringRecord` per row
///   and validated UTF-8 for every field. Reading into one record per chunk
///   allocates nothing per row; the UTF-8 check is reinstated explicitly below
///   so that malformed rows are still counted and dropped exactly as before.
/// * **Dense index, not a `HashMap` regroup.** The old path collected
///   `Vec<(CountryCode, range)>` and then regrouped it on *one* thread — a
///   serial section between two parallel ones. Each chunk now accumulates
///   straight into `Vec<Vec<_>>` by position and the merge is an `append`.
///
/// Measured on the 2026-09-01 archive: the two files together fall from
/// 450.5 ms to 192.8 ms.
fn load_blocks<T, F>(
    source_dir: &Path,
    file_name: &str,
    countries: &CountryIndex,
    parse_cidr: F,
) -> anyhow::Result<Vec<Vec<(T, T)>>>
where
    T: IpInt + Send,
    F: Fn(&[u8]) -> Option<(T, T)> + Sync,
{
    let file = File::open(source_dir.join(file_name))?;
    let mmap = mmap_file(&file)?;
    let bytes: &[u8] = mmap.as_ref();

    let mut hdr = ReaderBuilder::new().has_headers(true).from_reader(bytes);
    let headers = hdr.headers()?.clone();
    let idx = parse_block_indices(&headers, file_name)?;
    let expected_fields = headers.len();

    // Splitting at newlines is only sound while nothing is quoted: a quoted
    // newline would cut a record in half, and a quoted comma would shift every
    // field boundary after it — inside one chunk only, with no parse error and
    // no warning. That is a wrong-country answer, not a crash, so it is
    // checked on every run rather than assumed. Both blocks files held zero
    // `"` bytes across 44 MB of the 2026-09-01 archive; a future one that does
    // not falls back to a single range, which the csv reader then parses with
    // its full quoting rules, slower and correct.
    let quoted = memchr::memchr(b'"', bytes).is_some();
    if quoted {
        messages::warn(&format!(
            "{file_name} contains quoted fields; parsing it serially."
        ));
    }
    let ranges = if quoted {
        vec![(0, bytes.len())]
    } else {
        chunk_bounds(bytes, rayon::current_num_threads() * CHUNKS_PER_THREAD)
    };

    let ncc = countries.order.len();
    let skipped = AtomicUsize::new(0);

    let mut pools: Vec<Vec<(T, T)>> = ranges
        .par_iter()
        .map(|&(s, e)| {
            let mut rdr = ReaderBuilder::new()
                .has_headers(quoted)
                .flexible(true)
                .from_reader(&bytes[s..e]);
            let mut rec = csv::ByteRecord::new();
            let mut pools: Vec<Vec<(T, T)>> = vec![Vec::new(); ncc];
            let mut local_skipped = 0usize;

            loop {
                match rdr.read_byte_record(&mut rec) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(_) => {
                        // A reader over an in-memory slice cannot make
                        // progress after an error, so count the row and stop
                        // rather than spin on it.
                        local_skipped += 1;
                        break;
                    }
                }

                // Two checks the csv crate used to make for us, restored here
                // because the chunked reader cannot make them the same way.
                // `flexible(true)` is required — csv pins the expected width
                // to the *first record it sees*, which for a chunk is a data
                // row, so a single short row would otherwise condemn every
                // other row in that chunk. Comparing against the header width
                // is what the whole-file reader did. The UTF-8 check restores
                // what `StringRecord` did: the old path decoded every field
                // and counted a decode failure as a skipped row, and
                // `ByteRecord` does not decode at all.
                if rec.len() != expected_fields
                    || std::str::from_utf8(rec.as_slice()).is_err()
                {
                    local_skipped += 1;
                    continue;
                }

                let network = rec.get(idx.net).unwrap_or(b"");
                if network.is_empty() {
                    continue;
                }
                let Some(range) = parse_cidr(network) else {
                    continue;
                };
                let ci = countries.resolve(
                    rec.get(idx.proxy).unwrap_or(b"") == b"1",
                    rec.get(idx.sat).unwrap_or(b"") == b"1",
                    rec.get(idx.id).unwrap_or(b""),
                    rec.get(idx.rid).unwrap_or(b""),
                );
                pools[ci as usize].push(range);
            }

            skipped.fetch_add(local_skipped, Ordering::Relaxed);
            pools
        })
        .reduce(
            || vec![Vec::new(); ncc],
            |mut acc, part| {
                for (slot, mut v) in part.into_iter().enumerate() {
                    if acc[slot].is_empty() {
                        acc[slot] = v;
                    } else {
                        acc[slot].append(&mut v);
                    }
                }
                acc
            },
        );

    let n = skipped.load(Ordering::Relaxed);
    if n > 0 {
        messages::warn(&format!("{n} malformed rows skipped in {file_name}"));
    }

    pools.par_iter_mut().for_each(merge_ranges_in_place);
    Ok(pools)
}

/// The pre-O-001 country resolver, retained as the oracle for
/// [`CountryIndex::resolve`].
///
/// Production no longer calls it: the dense index resolves a row to a `u16`
/// slot without ever constructing a `CountryCode`. It stays because the index
/// is new code on a wrong-answer-shaped path, and a differential test against
/// the implementation it replaced is worth more than assertions written from
/// the same understanding that produced the replacement.
#[cfg(test)]
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

/// Parse an IPv4 CIDR straight from CSV bytes.
///
/// Replaces `IpNetwork::parse`, which was the single most expensive step in
/// the row loop: it allocates nothing, never builds a `str`, and skips the
/// enum dispatch that made every IPv6 row try the IPv4 grammar first.
///
/// The address half is as strict as `std`'s `Ipv4Addr`, which is what the
/// crate it replaces defers to — including the leading-zero rule, so
/// `01.2.3.4/8` is rejected here exactly as it was before. The prefix half is
/// deliberately *lenient* in the same three ways that crate is: see
/// [`parse_prefix`], and note that a missing `/` means a full-length prefix
/// rather than a parse failure. [`cidr_to_range_ipv4`] is kept as the oracle
/// and the two are checked against each other over a generated corpus.
#[inline]
fn cidr_v4_bytes(b: &[u8]) -> Option<(u32, u32)> {
    // A bare address means a full-length prefix, as in the crate this
    // replaces — a third divergence the differential test found.
    let (addr_bytes, prefix) = match memchr::memchr(b'/', b) {
        Some(i) => (&b[..i], parse_prefix(&b[i + 1..], 32)?),
        None => (b, 32),
    };

    let mut addr: u32 = 0;
    let mut octet: u32 = 0;
    let mut digits = 0u32;
    let mut octets = 0u32;
    for &c in addr_bytes {
        if c.is_ascii_digit() {
            // A second digit after a leading `0` is rejected by `Ipv4Addr`.
            if digits > 0 && octet == 0 {
                return None;
            }
            octet = octet * 10 + u32::from(c - b'0');
            digits += 1;
            if octet > 255 || digits > 3 {
                return None;
            }
        } else if c == b'.' {
            if digits == 0 || octets == 3 {
                return None;
            }
            addr = (addr << 8) | octet;
            octet = 0;
            digits = 0;
            octets += 1;
        } else {
            return None;
        }
    }
    if digits == 0 || octets != 3 {
        return None;
    }
    addr = (addr << 8) | octet;

    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    Some((addr & mask, (addr & mask) | !mask))
}

/// Parse an IPv6 CIDR straight from CSV bytes.
///
/// The address half is handed to `std`'s `Ipv6Addr`, which is what
/// `ipnetwork` used underneath; the saving here is the enum dispatch, the
/// `String` the csv crate no longer builds, and the `IpNetwork::V4` grammar
/// that every one of the 526,635 IPv6 rows used to be tried against first.
#[inline]
fn cidr_v6_bytes(b: &[u8]) -> Option<(u128, u128)> {
    let (addr_bytes, prefix) = match memchr::memchr(b'/', b) {
        Some(i) => (&b[..i], parse_prefix(&b[i + 1..], 128)?),
        None => (b, 128),
    };
    let addr = u128::from(
        Ipv6Addr::from_str(std::str::from_utf8(addr_bytes).ok()?).ok()?,
    );
    let mask = if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    };
    Some((addr & mask, (addr & mask) | !mask))
}

/// Parse a CIDR prefix length exactly as `u8::from_str` would.
///
/// Two of its quirks are load-bearing, and both were found by the
/// differential test rather than by reading: a leading `+` is accepted
/// (`/+8`), and so is any run of leading zeros (`/0128`). Neither appears in
/// a MaxMind archive, but "the parser is stricter than the one it replaced"
/// is still a behaviour change, and a silent one — the row would be dropped
/// rather than rejected loudly.
///
/// Note the contrast with [`parse_geoname`], which *must* reject a leading
/// zero: there the string is a map key and two spellings must stay distinct,
/// whereas here the number is only ever a shift width.
#[inline]
fn parse_prefix(b: &[u8], max: u32) -> Option<u32> {
    let digits = match b.first()? {
        b'+' => &b[1..],
        _ => b,
    };
    if digits.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u32::from(c - b'0'))?;
        if v > max {
            return None;
        }
    }
    Some(v)
}

/// The pre-O-001 IPv4 CIDR parser, retained as the oracle for
/// [`cidr_v4_bytes`]. See [`resolve_country_code`] for why the old
/// implementations stay in the tree.
#[cfg(test)]
fn cidr_to_range_ipv4(cidr: &str) -> Option<(u32, u32)> {
    let net: ipnetwork::IpNetwork = cidr.parse().ok()?;
    match net {
        ipnetwork::IpNetwork::V4(v4) => {
            Some((u32::from(v4.network()), u32::from(v4.broadcast())))
        }
        _ => None,
    }
}

/// The pre-O-001 IPv6 CIDR parser, retained as the oracle for
/// [`cidr_v6_bytes`].
#[cfg(test)]
fn cidr_to_range_ipv6(cidr: &str) -> Option<(u128, u128)> {
    let net: ipnetwork::IpNetwork = cidr.parse().ok()?;
    match net {
        ipnetwork::IpNetwork::V6(v6) => {
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

/// Sort and coalesce a country's ranges without allocating a second pool.
///
/// The old `merge_ranges` copied its input, built a second `Vec`, and returned
/// it to replace the first — two full copies of every country's ranges. This
/// sorts in place and compacts with a write cursor, which matters here because
/// the pools now arrive owned rather than borrowed.
///
/// The result does not depend on the order rows arrived in, which is what lets
/// the chunked loader replace the bridged one: after sorting by start, ties
/// resolve through `max(end)`, so the merged output is a function of the *set*
/// of ranges, not of the sequence.
fn merge_ranges_in_place<T: IpInt>(ranges: &mut Vec<(T, T)>) {
    if ranges.is_empty() {
        return;
    }
    ranges.sort_unstable_by_key(|r| r.0);
    let mut w = 0;
    for i in 1..ranges.len() {
        let (start, end) = ranges[i];
        if start <= ranges[w].1.saturating_inc() {
            ranges[w].1 = ranges[w].1.max(end);
        } else {
            w += 1;
            ranges[w] = (start, end);
        }
    }
    ranges.truncate(w + 1);
}

/// Allocating wrapper around [`merge_ranges_in_place`], kept for the tests
/// that describe the merge rules on literal inputs.
#[cfg(test)]
fn merge_ranges<T: IpInt>(ranges: &[(T, T)]) -> Vec<(T, T)> {
    let mut v = ranges.to_vec();
    merge_ranges_in_place(&mut v);
    v
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
    // One BLAKE3 call over the finished buffer, not two per range. The
    // digest is identical — BLAKE3 is a streaming hash, so `update(a)`
    // then `update(b)` is by definition `update(a ++ b)`, and `buf` is
    // exactly that concatenation. What changes is throughput: the wide
    // AVX2/AVX-512 path needs >= 1 KiB per call, and fed 4 or 16 bytes at
    // a time it degrades to one 64-byte block per call. Measured 318 ->
    // 1,725 MB/s on the production volume (O-003).
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
    let mut buf = Vec::with_capacity(ranges.len() * 32);
    for &(start, end) in ranges {
        buf.extend_from_slice(&start.to_be_bytes());
        buf.extend_from_slice(&end.to_be_bytes());
    }
    // One BLAKE3 call over the finished buffer, not two per range. The
    // digest is identical — BLAKE3 is a streaming hash, so `update(a)`
    // then `update(b)` is by definition `update(a ++ b)`, and `buf` is
    // exactly that concatenation. What changes is throughput: the wide
    // AVX2/AVX-512 path needs >= 1 KiB per call, and fed 4 or 16 bytes at
    // a time it degrades to one 64-byte block per call. Measured 318 ->
    // 1,725 MB/s on the production volume (O-003).
    let hash = blake3::hash(&buf).to_string();
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
        assert_eq!(CountryCode::parse("US"), Some(CountryCode::Iso(*b"US")));
    }

    #[test]
    fn country_code_parse_iso_lowercase_normalised() {
        assert_eq!(CountryCode::parse("us"), Some(CountryCode::Iso(*b"US")));
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
        assert_eq!(CountryCode::Iso(*b"GB").to_string(), "GB");
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
        let m = make_map(&[("1", CountryCode::Iso(*b"US"))]);
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
        let de = CountryCode::Iso(*b"DE");
        let m = make_map(&[("42", de)]);
        assert_eq!(resolve_country_code(false, false, "42", "", &m), de);
    }

    #[test]
    fn resolve_rid_fallback_when_id_empty() {
        let fr = CountryCode::Iso(*b"FR");
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

    /// F-002. `version` is a pointer and the manifest is its target;
    /// `backup::gather_files` in `Verified` mode reads the pointer, derives
    /// the manifest name from it, and aborts when that file is absent. This
    /// is the invariant the write order exists to preserve.
    #[test]
    fn a_written_manifest_is_the_one_the_version_pointer_names() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let version =
            Version::parse("GeoLite2-Country-CSV_20260324.zip").unwrap();

        let manifest = generate_manifest(
            p,
            &version,
            vec![("AA.iv4".into(), "ab".into())],
        )
        .unwrap();

        let pointer = fs::read_to_string(p.join("version")).unwrap();
        assert_eq!(pointer.trim(), version.as_str());
        assert_eq!(
            p.join(format!("GeoLite2-Country-bin_{}.blake3", pointer.trim())),
            manifest,
            "the pointer must resolve to the manifest that was written"
        );
        assert!(manifest.exists());
    }

    /// F-002. Writing the pointer *first* meant a failure between the two
    /// writes left `version` naming a manifest that was never written — a
    /// dangling pointer that blocks every verified operation on data that is
    /// otherwise intact ("Manifest missing … Use -f to force"). With the
    /// manifest written first, the same failure leaves the previous pointer
    /// and the previous manifest still agreeing with each other.
    ///
    /// A directory standing where the manifest goes makes `fs::write` fail
    /// with EISDIR, standing in for the ENOSPC/EACCES/EIO this guards.
    #[test]
    fn a_failed_manifest_write_leaves_the_version_pointer_untouched() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let version =
            Version::parse("GeoLite2-Country-CSV_20260324.zip").unwrap();

        fs::write(p.join("version"), "20260101\n").unwrap();
        let stale = touch(p, "GeoLite2-Country-bin_20260101.blake3");
        fs::create_dir(p.join(version.bin_manifest_name())).unwrap();

        generate_manifest(p, &version, vec![]).unwrap_err();

        assert_eq!(
            fs::read_to_string(p.join("version")).unwrap(),
            "20260101\n",
            "a failed manifest write must not advance the version pointer"
        );
        assert!(
            stale.exists(),
            "the manifest the surviving pointer names must still be there"
        );
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

    /// F-003. The stale-manifest exception is scoped by the man page to
    /// "*.blake3, *.sha256 **from an earlier build**". Selecting on
    /// extension alone made every file with those extensions eligible, so
    /// an operator's own checksum file in `output_dir` was deleted
    /// silently by the next build — a direct breach of the "never touched"
    /// guarantee for unowned files.
    #[test]
    fn detect_orphans_foreign_checksum_files_untouched() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let foreign = [
            touch(p, "SHA256SUMS.sha256"),
            touch(p, "checksums.sha256"),
            touch(p, "release.blake3"),
            // Right extension, right shape, wrong product.
            touch(p, "GeoLite2-City-bin_20260101.blake3"),
        ];
        let manifest = touch(p, "GeoLite2-Country-bin_20260606.blake3");

        detect_orphans(p, &[], &manifest).unwrap();

        for f in &foreign {
            assert!(
                f.exists(),
                "unowned file must never be deleted: {}",
                f.display()
            );
        }
        // Nor may they be reported as stale-owned; they are not ours to
        // have an opinion about.
        assert!(
            !p.join("orphaned").exists(),
            "unowned files must not be listed as orphans"
        );
    }

    /// The same structural test the clean path uses: a two-character stem
    /// from [A-Z0-9]. A file that merely ends in .iv4 is not ours.
    #[test]
    fn detect_orphans_foreign_iv_files_are_invisible() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let foreign = [
            touch(p, "backup.iv4"), // stem too long
            touch(p, "us.iv4"),     // lowercase
            touch(p, "U.iv6"),      // too short
            touch(p, "U-.iv6"),     // not [A-Z0-9]
        ];
        let manifest = touch(p, "GeoLite2-Country-bin_20260606.blake3");

        detect_orphans(p, &[], &manifest).unwrap();

        for f in &foreign {
            assert!(f.exists(), "unowned file deleted: {}", f.display());
        }
        assert!(
            !p.join("orphaned").exists(),
            "unowned files must not be listed as orphans"
        );
    }

    /// The exception still works for what it was written for: a manifest
    /// this program produced, from an earlier version, is superseded.
    #[test]
    fn detect_orphans_our_own_stale_manifest_still_deleted() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        let old_blake3 = touch(p, "GeoLite2-Country-bin_20260101.blake3");
        let old_sha256 = touch(p, "GeoLite2-Country-bin_20260101.sha256");
        let manifest = touch(p, "GeoLite2-Country-bin_20260606.blake3");

        detect_orphans(p, &[], &manifest).unwrap();

        assert!(!old_blake3.exists(), "our stale manifest must be deleted");
        assert!(!old_sha256.exists(), "our stale manifest must be deleted");
        assert!(manifest.exists(), "the current manifest must survive");
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

    // ── O-003: one hash call over the buffer, same digest ────────────────

    /// O-003 replaced two `hasher.update()` calls per range with a single
    /// call over the finished buffer. The manifest digests are published
    /// and compared against on every subsequent *verified* operation, so
    /// the equivalence is load-bearing rather than incidental: pin it
    /// against the incremental form the optimisation removed.
    #[test]
    fn hashing_the_whole_buffer_matches_the_incremental_form_v4() {
        let dir = TempDir::new().unwrap();
        let ranges: Vec<(u32, u32)> =
            (0..1000u32).map(|i| (i * 16, i * 16 + 15)).collect();

        let (_, actual) =
            write_country_v4(&dir.path().join("XX"), &ranges).unwrap();

        let mut hasher = blake3::Hasher::new();
        for &(start, end) in &ranges {
            hasher.update(&start.to_be_bytes());
            hasher.update(&end.to_be_bytes());
        }
        assert_eq!(actual, hasher.finalize().to_string());

        // And the digest must still describe the bytes actually on disk.
        let written = fs::read(dir.path().join("XX.iv4")).unwrap();
        assert_eq!(written.len(), ranges.len() * 8);
        assert_eq!(actual, blake3::hash(&written).to_string());
    }

    #[test]
    fn hashing_the_whole_buffer_matches_the_incremental_form_v6() {
        let dir = TempDir::new().unwrap();
        let ranges: Vec<(u128, u128)> =
            (0..1000u128).map(|i| (i << 64, (i << 64) + 255)).collect();

        let (_, actual) =
            write_country_v6(&dir.path().join("XX"), &ranges).unwrap();

        let mut hasher = blake3::Hasher::new();
        for &(start, end) in &ranges {
            hasher.update(&start.to_be_bytes());
            hasher.update(&end.to_be_bytes());
        }
        assert_eq!(actual, hasher.finalize().to_string());

        let written = fs::read(dir.path().join("XX.iv6")).unwrap();
        assert_eq!(written.len(), ranges.len() * 32);
        assert_eq!(actual, blake3::hash(&written).to_string());
    }

    /// The empty case: a country with no ranges of one family still gets a
    /// digest, and it must be BLAKE3 of the empty input rather than
    /// anything special-cased.
    #[test]
    fn an_empty_range_set_hashes_the_empty_buffer() {
        let dir = TempDir::new().unwrap();
        let (_, hash) = write_country_v4(&dir.path().join("ZZ"), &[]).unwrap();
        assert_eq!(hash, blake3::hash(b"").to_string());
        assert_eq!(fs::read(dir.path().join("ZZ.iv4")).unwrap().len(), 0);
    }

    // ── O-001: the byte parsers against the crate they replaced ──

    /// A deterministic corpus, so a failure is reproducible.
    fn lcg(seed: &mut u64) -> u64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        *seed
    }

    /// The hand-rolled IPv4 parser must agree with `ipnetwork` — which is
    /// what production used until O-001 — on every input, not just on the
    /// ones the archive happens to contain. 20,000 generated CIDRs plus the
    /// edge cases a generator will not produce.
    #[test]
    fn ipv4_byte_parser_agrees_with_ipnetwork() {
        let mut seed = 0x5DEECE66Du64;
        let mut cases: Vec<String> = Vec::with_capacity(20_100);
        for _ in 0..20_000 {
            let a = lcg(&mut seed);
            // Prefixes range past 32 on purpose: rejection must match too.
            let pfx = (lcg(&mut seed) % 40) as u32;
            cases.push(format!(
                "{}.{}.{}.{}/{}",
                a as u8,
                (a >> 8) as u8,
                (a >> 16) as u8,
                (a >> 24) as u8,
                pfx
            ));
        }
        cases.extend(
            [
                "0.0.0.0/0",
                "255.255.255.255/32",
                "1.2.3.4/033",
                "01.2.3.4/8",
                "1.02.3.4/8",
                "00.0.0.0/8",
                "1.2.3.4/33",
                "1.2.3.4/999",
                "1.2.3.4/+8",
                "1.2.3.4/-8",
                "1.2.3.4/",
                "1.2.3.4",
                "1.2.3/24",
                "1.2.3.4.5/24",
                "1.2.3.256/24",
                "1.2.3.4444/24",
                "1.2..4/24",
                ".1.2.3/24",
                "1.2.3./24",
                "",
                "/24",
                "not-a-cidr",
                " 1.2.3.4/24",
                "1.2.3.4/24 ",
                "::1/128",
                "2001:db8::/64",
            ]
            .map(String::from),
        );

        for case in &cases {
            assert_eq!(
                cidr_v4_bytes(case.as_bytes()),
                cidr_to_range_ipv4(case),
                "IPv4 parsers disagree on {case:?}"
            );
        }
    }

    /// As above for IPv6. The generator walks the whole 128-bit space and
    /// both compressed and uncompressed spellings, since the address half is
    /// the part that is delegated rather than hand-written.
    #[test]
    fn ipv6_byte_parser_agrees_with_ipnetwork() {
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut cases: Vec<String> = Vec::with_capacity(20_100);
        for _ in 0..20_000 {
            let hi = lcg(&mut seed);
            let lo = lcg(&mut seed);
            let addr = Ipv6Addr::from(((hi as u128) << 64) | lo as u128);
            let pfx = (lcg(&mut seed) % 140) as u32;
            cases.push(format!("{addr}/{pfx}"));
        }
        cases.extend(
            [
                "::/0",
                "::1/128",
                "2001:db8::/64",
                "2001:0db8:0000:0000:0000:0000:0000:0000/64",
                "::ffff:1.2.3.4/120",
                "2001:db8::/0128",
                "2001:db8::/129",
                "2001:db8::/999",
                "2001:db8::/+64",
                "2001:db8::/",
                "2001:db8::",
                "2001:db8:::/64",
                "garbage",
                "",
                "/64",
                "1.2.3.4/8",
                " ::1/128",
            ]
            .map(String::from),
        );

        for case in &cases {
            assert_eq!(
                cidr_v6_bytes(case.as_bytes()),
                cidr_to_range_ipv6(case),
                "IPv6 parsers disagree on {case:?}"
            );
        }
    }

    /// The first of the two silent-wrong-country holes the dense index could
    /// have opened: an out-of-range geoname must be rejected, not wrapped
    /// onto a real one. `4294967296` is `u32::MAX + 1`, which wraps to 0.
    #[test]
    fn parse_geoname_rejects_what_would_alias_a_real_id() {
        assert_eq!(parse_geoname(b"49518"), Some(49518));
        assert_eq!(parse_geoname(b"0"), Some(0));
        assert_eq!(parse_geoname(b"4294967295"), Some(u32::MAX));
        assert_eq!(parse_geoname(b"4294967296"), None, "must not wrap to 0");
        assert_eq!(parse_geoname(b"9999999999"), None, "must not wrap");
        assert_eq!(parse_geoname(b"12345678901"), None, "too long");
        assert_eq!(parse_geoname(b"0123"), None, "non-canonical spelling");
        assert_eq!(parse_geoname(b"00"), None);
        assert_eq!(parse_geoname(b""), None);
        assert_eq!(parse_geoname(b"12a"), None);
        assert_eq!(parse_geoname(b"-1"), None);
    }

    fn index_fixture() -> (HashMap<String, CountryCode>, CountryIndex) {
        let de = CountryCode::parse("DE").unwrap();
        let fr = CountryCode::parse("FR").unwrap();
        let mut country_id = HashMap::new();
        country_id.insert("42".to_string(), de);
        country_id.insert("99".to_string(), fr);
        country_id.insert("4294967295".to_string(), de);
        // The second hole: a geoname the dense table cannot hold. The string
        // map resolved it, so the index must too.
        country_id.insert("X7".to_string(), fr);
        country_id.insert("0123".to_string(), de);

        let mut country_name = BTreeMap::new();
        for (cc, name) in [
            (de, "Germany"),
            (fr, "France"),
            (CountryCode::A1, "Anonymous Proxy"),
            (CountryCode::A2, "Satellite Provider"),
            (CountryCode::O1, "Other Country"),
        ] {
            country_name.insert(cc, name.to_string());
        }

        let index = CountryIndex::new(&country_id, &country_name);
        (country_id, index)
    }

    /// The dense index must resolve every row exactly as the string-keyed
    /// resolver it replaced — including the two cases it cannot represent
    /// directly, which is the whole reason `by_str` exists.
    #[test]
    fn country_index_agrees_with_the_resolver_it_replaced() {
        let (country_id, index) = index_fixture();
        let cases: Vec<(bool, bool, &str, &str)> = vec![
            (false, false, "42", ""),
            (false, false, "99", ""),
            (false, false, "", "42"),
            (false, false, "42", "99"),
            (false, false, "", ""),
            (false, false, "7", ""),
            (false, false, "4294967295", ""),
            (false, false, "4294967296", ""),
            (false, false, "X7", ""),
            (false, false, "0123", ""),
            (false, false, "123", ""),
            (true, false, "42", ""),
            (false, true, "42", ""),
            (true, true, "42", ""),
            (true, false, "", ""),
        ];
        for (proxy, sat, id, rid) in cases {
            let slot = index.resolve(proxy, sat, id.as_bytes(), rid.as_bytes());
            assert_eq!(
                index.order[slot as usize],
                resolve_country_code(proxy, sat, id, rid, &country_id),
                "index disagrees on ({proxy}, {sat}, {id:?}, {rid:?})"
            );
        }
    }

    /// `order` is what `build` zips the dense pools back against, so it must
    /// be exactly the key order of the map it came from.
    #[test]
    fn country_index_order_is_btreemap_key_order() {
        let (_, index) = index_fixture();
        let mut sorted = index.order.clone();
        sorted.sort();
        assert_eq!(index.order, sorted, "order must be sorted by code");
        assert_eq!(index.order[index.a1 as usize], CountryCode::A1);
        assert_eq!(index.order[index.a2 as usize], CountryCode::A2);
        assert_eq!(index.order[index.o1 as usize], CountryCode::O1);
    }

    /// Chunking is only correct if the ranges tile the file exactly once:
    /// a gap silently drops rows, an overlap silently doubles them, and a
    /// split anywhere but after a newline corrupts two rows at the seam.
    #[test]
    fn chunk_bounds_tile_the_body_exactly_once() {
        let mut csv = String::from("network,geoname_id\n");
        for i in 0..500 {
            csv.push_str(&format!("10.0.{}.0/24,{i}\n", i % 256));
        }
        let bytes = csv.as_bytes();
        let header_end = csv.find('\n').unwrap() + 1;

        for n in [1usize, 2, 3, 7, 8, 64, 4096] {
            let bounds = chunk_bounds(bytes, n);
            assert_eq!(bounds[0].0, header_end, "n={n}: must skip the header");
            assert_eq!(
                bounds.last().unwrap().1,
                bytes.len(),
                "n={n}: must reach the end"
            );
            for w in bounds.windows(2) {
                assert_eq!(w[0].1, w[1].0, "n={n}: gap or overlap");
            }
            for &(_, e) in &bounds {
                assert!(
                    e == bytes.len() || bytes[e - 1] == b'\n',
                    "n={n}: chunk ends mid-row"
                );
            }
        }

        assert!(chunk_bounds(b"header only, no newline", 4).is_empty());
        assert!(chunk_bounds(b"header\n", 4).is_empty());
    }

    fn blocks_fixture(dir: &Path, name: &str, body: &str) {
        let mut csv = String::from(
            "network,geoname_id,registered_country_geoname_id,\
             represented_country_geoname_id,is_anonymous_proxy,\
             is_satellite_provider,is_anycast\n",
        );
        csv.push_str(body);
        fs::write(dir.join(name), csv).unwrap();
    }

    /// End to end over the chunked path: rows land in the right country, get
    /// merged, and malformed ones are dropped rather than aborting.
    #[test]
    fn load_blocks_groups_merges_and_skips() {
        let dir = TempDir::new().unwrap();
        let (_, index) = index_fixture();
        blocks_fixture(
            dir.path(),
            BLOCKS_V4,
            "10.0.0.0/24,42,,,0,0,0\n10.0.1.0/24,42,,,0,0,0\n192.168.0.0/24,\
             99,,,0,0,0\n172.16.0.0/24,,,,1,0,0\nnot-a-cidr,42,,,0,0,0\n,42,,,\
             0,0,0\n10.9.9.9/24,42,,,0,0,0,extra-field\n",
        );

        let pools =
            load_blocks(dir.path(), BLOCKS_V4, &index, cidr_v4_bytes).unwrap();

        let slot = |code: &str| {
            let cc = CountryCode::parse(code).unwrap();
            index.order.iter().position(|&c| c == cc).unwrap()
        };
        // The two adjacent /24s coalesce into one range.
        assert_eq!(
            pools[slot("DE")],
            vec![(
                u32::from_be_bytes([10, 0, 0, 0]),
                u32::from_be_bytes([10, 0, 1, 255])
            )]
        );
        assert_eq!(pools[slot("FR")].len(), 1);
        assert_eq!(pools[index.a1 as usize].len(), 1, "proxy row → A1");
        // Unparseable CIDR, empty network, and the wrong-width row are gone;
        // nothing else is.
        let total: usize = pools.iter().map(|p| p.len()).sum();
        assert_eq!(total, 3);
    }

    /// The quote guard. A quoted comma shifts every field boundary after it,
    /// which chunk splitting cannot see — so a quoted file must take the
    /// single-range path and still parse correctly.
    #[test]
    fn load_blocks_handles_a_quoted_field() {
        let dir = TempDir::new().unwrap();
        let (_, index) = index_fixture();
        blocks_fixture(
            dir.path(),
            BLOCKS_V4,
            "10.0.0.0/24,42,\"a,b\",,0,0,0\n192.168.0.0/24,99,\"c,d\",,0,0,0\n",
        );

        let pools =
            load_blocks(dir.path(), BLOCKS_V4, &index, cidr_v4_bytes).unwrap();
        let de = index
            .order
            .iter()
            .position(|&c| c == CountryCode::parse("DE").unwrap())
            .unwrap();
        assert_eq!(pools[de].len(), 1, "quoted comma must not shift fields");
        assert_eq!(pools.iter().map(|p| p.len()).sum::<usize>(), 2);
    }
}
