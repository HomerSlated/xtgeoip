# xtgeoip

Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

## build
Build xt_geoip data from the local CSV copy.

- `xtgeoip build` → build using local copy of database
- `xtgeoip build -b -p` → backup, prune backups, then build
- `xtgeoip build -p` → unsupported option, build has no prune function
- `xtgeoip build -c -f` → clean using manifest, then build using local copy of database
- `xtgeoip build -b -c -p` → backup, prune backups, clean, then build
- `xtgeoip build -b -c -p -f` → unsupported option, ambiguous and prune does not support force

## fetch
Download or refresh the local MaxMind CSV archive set.

- `xtgeoip fetch` → fetch CSVs
- `xtgeoip fetch -p` → fetch CSVs, then prune CSVs
- `xtgeoip fetch -b` → unsupported option, -b is not valid for fetch

## run
Fetch, then build, optionally wrapping with backup/clean/prune.

- `xtgeoip run` → fetch, then build
- `xtgeoip run -p` → fetch, then prune CSVs, then build
- `xtgeoip run -c -p` → clean using manifest, then fetch, then prune CSVs, then build
- `xtgeoip run -c -f` → force clean without manifest, then fetch, then build
- `xtgeoip run -c -p -f` → unsupported option, ambiguous and prune does not support force
- `xtgeoip run -b -c -p` → unsupported option, ambiguous (does prune apply to fetch or backup?)

## show
Display current configuration and relevant paths.

- `xtgeoip show` → show configured paths and status

