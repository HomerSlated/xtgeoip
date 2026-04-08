# xtgeoip

Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

## top level
Legacy top-level flag mode without a subcommand.

- `xtgeoip` → display usage (exit 1) — No arguments prints usage and exits non-zero.
- `xtgeoip -h` → display usage (exit 0) — Explicit help prints usage and exits zero.
- `xtgeoip -b` → back up binary data in /usr/share/xt_geoip/ using the manifest; store backups in /var/lib/xt_geoip/
- `xtgeoip -b -c` → back up and clean binary data using the manifest
- `xtgeoip -b -c -f` → force backup and clean even without a manifest
- `xtgeoip -b -f` → force backup of binary data in /usr/share/xt_geoip/ even without a manifest
- `xtgeoip -b -p` → back up binary data in /usr/share/xt_geoip/, then prune old backups in /var/lib/xt_geoip/
- `xtgeoip -b -p -f` → unsupported option, ambiguous and prune does not support force
- `xtgeoip -c` → clean (delete) binary data in /usr/share/xt_geoip/ using the manifest
- `xtgeoip -c -f` → force clean (delete) binary data in /usr/share/xt_geoip/ even without a manifest
- `xtgeoip -c -p` → unsupported option, clean has no prune function
- `xtgeoip -c -p -f` → unsupported option combination
- `xtgeoip -p` → unsupported option combination
- `xtgeoip -f` → unsupported option, top-level mode without backup or clean does not support force
- `xtgeoip -l` → unsupported option, -l is not valid for top-level mode

## build
Build xt_geoip data from the local CSV copy.

- `xtgeoip build` → build using local copy of database
- `xtgeoip build -l` → build using local copy of database in legacy mode
- `xtgeoip build -b -p` → backup, prune backups, then build
- `xtgeoip build -p` → unsupported option, build has no prune function
- `xtgeoip build -c -f` → clean using manifest, then build using local copy of database
- `xtgeoip build -b -c -p` → backup, prune backups, clean, then build
- `xtgeoip build -b -c -p -f` → unsupported option, ambiguous and prune does not support force

## conf
Configuration operations.
Usage: xtgeoip conf <FLAG>

- `xtgeoip conf` → missing required argument, conf requires FLAG
- `xtgeoip conf -s` → show current config
- `xtgeoip conf -d` → show default config
- `xtgeoip conf -e` → edit config

## fetch
Download or refresh the local MaxMind CSV archive set.

- `xtgeoip fetch` → fetch CSVs
- `xtgeoip fetch -p` → fetch CSVs, then prune CSV archives
- `xtgeoip fetch -l` → unsupported option, -l is not valid for fetch
- `xtgeoip fetch -b` → unsupported option, -b is not valid for fetch
- `xtgeoip fetch -c` → unsupported option, -c is not valid for fetch
- `xtgeoip fetch -f` → unsupported option, -f is not valid for fetch

## run
Fetch, then build, optionally wrapping with backup/clean/prune.

- `xtgeoip run` → fetch, then build
- `xtgeoip run -l` → fetch, then build in legacy mode
- `xtgeoip run -p` → fetch, then prune CSV archives, then build
- `xtgeoip run -c -p` → clean using manifest, then fetch, then prune CSV archives, then build
- `xtgeoip run -c -f` → force clean without manifest, then fetch, then build
- `xtgeoip run -c -p -f` → unsupported option, ambiguous and prune does not support force
- `xtgeoip run -b -c -p` → unsupported option, ambiguous (does prune apply to fetch or backup?)

## version
Show the program version.

- `xtgeoip -V` → display version

