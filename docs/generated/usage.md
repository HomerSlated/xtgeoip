# xtgeoip

Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

## build
Build xt_geoip data from the local CSV copy.

- `xtgeoip build` → build using local copy of database
- `xtgeoip build -l` → build using local copy of database in legacy mode
- `xtgeoip build -b -p` → backup, prune backups, then build
- `xtgeoip build -p` → (no_prune_here)
- `xtgeoip build -c -f` → clean using manifest, then build using local copy of database
- `xtgeoip build -b -c -p` → backup, prune backups, clean, then build
- `xtgeoip build -b -c -p -f` → (ambiguous_force_prune)

## conf
Configuration operations.
Usage: xtgeoip conf <FLAG>

- `xtgeoip conf` → (missing_required_argument)
- `xtgeoip conf -s` → show current config
- `xtgeoip conf -d` → show default config
- `xtgeoip conf -e` → edit config

## fetch
Download or refresh the local MaxMind CSV archive set.

- `xtgeoip fetch` → fetch CSVs
- `xtgeoip fetch -p` → fetch CSVs, then prune CSVs
- `xtgeoip fetch -l` → (invalid_flag)
- `xtgeoip fetch -b` → (invalid_flag)

## run
Fetch, then build, optionally wrapping with backup/clean/prune.

- `xtgeoip run` → fetch, then build
- `xtgeoip run -l` → fetch, then build in legacy mode
- `xtgeoip run -p` → fetch, then prune CSVs, then build
- `xtgeoip run -c -p` → clean using manifest, then fetch, then prune CSVs, then build
- `xtgeoip run -c -f` → force clean without manifest, then fetch, then build
- `xtgeoip run -c -p -f` → (ambiguous_force_prune)
- `xtgeoip run -b -c -p` → (ambiguous_prune_target)

