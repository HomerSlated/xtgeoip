# xtgeoip

Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

## top level
Top level xtgeoip command
- `xtgeoip` → no command specified (exit 1)
- `xtgeoip -h` → display usage
- `xtgeoip -l` → unsupported option -{flag} for {command}
- `xtgeoip -f` → {command} does not support force without a target
- `xtgeoip -p` → unsupported option combination
- `xtgeoip -b` → backup database
- `xtgeoip -c` → clean database
- `xtgeoip -b -c` → backup then clean
- `xtgeoip -b -p` → backup then prune
- `xtgeoip -c -p` → {command} does not support prune
- `xtgeoip -b -p -f` → ambiguous flags: prune and force cannot be combined in this context
- `xtgeoip -c -p -f` → unsupported option combination

## build
Build xt_geoip database
- `xtgeoip build` → build database
- `xtgeoip build -l` → build (legacy mode)
- `xtgeoip build -b -p` → backup then prune then build
- `xtgeoip build -p` → {command} does not support prune
- `xtgeoip build -c -f` → clean then build
- `xtgeoip build -b -c -p` → backup, prune, clean, build
- `xtgeoip build -b -c -p -f` → ambiguous flags: prune and force cannot be combined in this context

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
- `xtgeoip fetch -l` → unsupported option -{flag} for {command}
- `xtgeoip fetch -b` → unsupported option -{flag} for {command}
- `xtgeoip fetch -c` → unsupported option -{flag} for {command}
- `xtgeoip fetch -f` → unsupported option -{flag} for {command}

## run
Run full pipeline
- `xtgeoip run` → fetch then build
- `xtgeoip run -l` → fetch then build (legacy)
- `xtgeoip run -p` → fetch then prune then build
- `xtgeoip run -c -p` → clean then fetch then prune then build
- `xtgeoip run -c -f` → force clean then fetch then build
- `xtgeoip run -c -p -f` → ambiguous flags: prune and force cannot be combined in this context
- `xtgeoip run -b -c -p` → ambiguous prune target between {left} and {right}

