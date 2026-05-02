# xtgeoip

Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

## top level
Top level xtgeoip command
- `xtgeoip` → you must specify at least one argument (exit 1)
- `xtgeoip -h` → display usage
- `xtgeoip -l` → you must specify build or run, for the --legacy option
- `xtgeoip -f` → you must specify --backup or --clean, for the --force option
- `xtgeoip -p` → you must specify fetch or --backup, for the --prune option
- `xtgeoip -b` → backup database
- `xtgeoip -c` → clean database
- `xtgeoip -b -c` → backup then clean
- `xtgeoip -b -p` → backup then prune
- `xtgeoip -c -p` → you must specify fetch or --backup, for the prune option, and --clean does not support the --prune option
- `xtgeoip -b -p -f` → --prune does not support the --force option
- `xtgeoip -c -p -f` → you must specify fetch or --backup, for the prune option, --clean does not support the --prune option, and --prune does not support the --force option
- `xtgeoip -b -f` → force backup database
- `xtgeoip -b -c -f` → force backup then clean

## build
Build xt_geoip database
- `xtgeoip build` → build database
- `xtgeoip build -l` → build (legacy mode)
- `xtgeoip build -b -p` → backup then prune then build
- `xtgeoip build -p` → you must specify --backup, for the --prune option
- `xtgeoip build -c -f` → clean then build
- `xtgeoip build -b -c -p` → backup, prune, clean, build
- `xtgeoip build -b -c -p -f` → --prune does not support the --force option
- `xtgeoip build -f` → build does not support the --force option
- `xtgeoip build -b` → backup then build
- `xtgeoip build -c` → clean then build
- `xtgeoip build -b -c` → backup then clean then build
- `xtgeoip build -b -f` → force backup then build
- `xtgeoip build -b -c -f` → force backup then clean then build

## conf
xtgeoip conf <-s|-d|-e>
- `xtgeoip conf` → {command} requires {argument}
- `xtgeoip conf -s` → show configuration
- `xtgeoip conf -d` → show default configuration
- `xtgeoip conf -e` → edit configuration

## fetch
Fetch GeoLite2 data files
- `xtgeoip fetch` → fetch CSVs
- `xtgeoip fetch -p` → fetch then prune archives
- `xtgeoip fetch -l` → fetch does not support the --legacy option
- `xtgeoip fetch -b` → fetch does not support the --backup option
- `xtgeoip fetch -c` → fetch does not support the --clean option
- `xtgeoip fetch -f` → fetch does not support the --force option

## run
Run full pipeline
- `xtgeoip run` → fetch then build
- `xtgeoip run -l` → fetch then build (legacy)
- `xtgeoip run -p` → fetch then prune then build
- `xtgeoip run -c -p` → clean then fetch then prune then build
- `xtgeoip run -c -f` → force clean then fetch then build
- `xtgeoip run -c -p -f` → --prune does not support the --force option
- `xtgeoip run -b -c -p` → you must specify fetch or build separately, for the --prune option
- `xtgeoip run -f` → run does not support the --force option
- `xtgeoip run -b` → backup then fetch then build
- `xtgeoip run -b -c` → backup then clean then fetch then build
- `xtgeoip run -b -f` → force backup then fetch then build
- `xtgeoip run -b -p` → backup then fetch then prune then build
- `xtgeoip run -b -c -f` → force backup then clean then fetch then build

