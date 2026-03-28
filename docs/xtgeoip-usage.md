# Usage

## Top level

- `xtgeoip` — print usage
- `xtgeoip -h` — print usage

- `xtgeoip -b` — back up binary data in `/usr/share/xt_geoip/` using the manifest; store backups in `/var/lib/xt_geoip/`
- `xtgeoip -b -c` — back up and clean binary data using the manifest
- `xtgeoip -b -c -f` — force backup and clean even without a manifest
- `xtgeoip -b -f` — force backup of binary data in `/usr/share/xt_geoip/` even without a manifest
- `xtgeoip -b -p` — back up binary data in `/usr/share/xt_geoip/`, then prune old backups in `/var/lib/xt_geoip/`
- `xtgeoip -b -p -f` — unsupported option: ambiguous, and prune does not support force (does force apply to backup or prune?)

- `xtgeoip -c` — clean (delete) binary data in `/usr/share/xt_geoip/` using the manifest
- `xtgeoip -c -f` — force clean (delete) binary data in `/usr/share/xt_geoip/` even without a manifest
- `xtgeoip -c -p` — unsupported option: clean does not have a prune function
- `xtgeoip -c -p -f` — unsupported option: clean does not have a prune function; prune does not support force

## build

- `xtgeoip build` — build binary data in `/usr/share/xt_geoip/` using the local copy of the database
- `xtgeoip build -b` — back up using the manifest, then build using the local copy of the database
- `xtgeoip build -b -c` — back up and clean using the manifest, then build using the local copy of the database
- `xtgeoip build -b -c -f` — force backup and clean even without a manifest, then build using the local copy of the database
- `xtgeoip build -b -c -p` — back up, prune old backups, clean, then build using the local copy of the database
- `xtgeoip build -b -c -p -f` — unsupported option: ambiguous, and prune does not support force
- `xtgeoip build -b -f` — force backup even without a manifest, then build using the local copy of the database
- `xtgeoip build -b -p` — back up, prune old backups, then build using the local copy of the database

- `xtgeoip build -c` — clean using the manifest, then build using the local copy of the database
- `xtgeoip build -c -f` — force clean even without a manifest, then build using the local copy of the database
- `xtgeoip build -c -p` — unsupported option: neither build nor clean has a prune function

- `xtgeoip build -p` — unsupported option: build has no prune function

## fetch

- `xtgeoip fetch` — download the latest MaxMind database
- `xtgeoip fetch -b` — unsupported option: fetch has no backup function
- `xtgeoip fetch -c` — unsupported option: fetch has no clean function
- `xtgeoip fetch -f` — unsupported option: fetch has no force option
- `xtgeoip fetch -p` — download the latest MaxMind database, then prune old CSV archives

## run

- `xtgeoip run` — fetch (if needed), then build binary data in `/usr/share/xt_geoip/`
- `xtgeoip run -b` — back up using the manifest, then fetch, then build
- `xtgeoip run -b -c` — back up and clean using the manifest, then fetch, then build
- `xtgeoip run -b -c -f` — force backup and clean even without a manifest, then fetch, then build
- `xtgeoip run -b -c -p` — unsupported option: ambiguous (does prune apply to fetch or backup?)
- `xtgeoip run -b -f` — force backup even without a manifest, then fetch, then build
- `xtgeoip run -b -p` — unsupported option: ambiguous (are we pruning backups or CSV archives?)
- `xtgeoip run -b -p -f` — unsupported option: ambiguous, and prune does not support force (does force apply to backup or prune?)

- `xtgeoip run -c` — clean using the manifest, then fetch, then build
- `xtgeoip run -c -f` — force clean even without a manifest, then fetch, then build
- `xtgeoip run -c -p` — clean using the manifest, then fetch, then prune CSV archives, then build
- `xtgeoip run -c -p -f` — unsupported option: prune does not support force

- `xtgeoip run -p` — fetch, then prune CSV archives, then build

## conf

- `xtgeoip conf` — print conf usage
- `xtgeoip conf -h` — print conf usage

- `xtgeoip conf -b` — unsupported option: conf has no backup function
- `xtgeoip conf -c` — unsupported option: conf has no clean function
- `xtgeoip conf -f` — unsupported option: conf has no force option
- `xtgeoip conf -p` — unsupported option: conf has no prune function

- `xtgeoip conf -d` — show default config
- `xtgeoip conf -e` — edit current config using `$EDITOR`
- `xtgeoip conf -s` — show current config
