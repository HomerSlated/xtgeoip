# xtgeoip

Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

## top level
Top level xtgeoip command

- `xtgeoip` → no command or top-level action specified (exit 1)
- `xtgeoip -h` → OK
- `xtgeoip -b` → OK
- `xtgeoip -b -c` → OK
- `xtgeoip -b -c -f` → OK
- `xtgeoip -b -f` → OK
- `xtgeoip -b -p` → OK
- `xtgeoip -b -p -f` → unsupported option, ambiguous and prune does not support force
- `xtgeoip -c` → OK
- `xtgeoip -c -f` → OK
- `xtgeoip -c -p` → unsupported option, {command} has no prune function
- `xtgeoip -c -p -f` → unsupported option combination
- `xtgeoip -p` → unsupported option combination
- `xtgeoip -f` → unsupported option, {target} does not support force
- `xtgeoip -l` → unsupported option, -{flag} is not valid for {command}

## build
Build xt_geoip database

- `xtgeoip build` → OK
- `xtgeoip build -l` → OK
- `xtgeoip build -b -p` → OK
- `xtgeoip build -p` → unsupported option, {command} has no prune function
- `xtgeoip build -c -f` → OK
- `xtgeoip build -b -c -p` → OK
- `xtgeoip build -b -c -p -f` → unsupported option, ambiguous and prune does not support force

## conf
Manage configuration
Usage: xtgeoip conf <-s|-d|-e>

- `xtgeoip conf` → missing required argument, {command} requires {argument}
- `xtgeoip conf -s` → OK
- `xtgeoip conf -d` → OK
- `xtgeoip conf -e` → OK

## fetch
Fetch GeoLite2 data files

- `xtgeoip fetch` → OK
- `xtgeoip fetch -p` → OK
- `xtgeoip fetch -l` → unsupported option, -{flag} is not valid for {command}
- `xtgeoip fetch -b` → unsupported option, -{flag} is not valid for {command}
- `xtgeoip fetch -c` → unsupported option, -{flag} is not valid for {command}
- `xtgeoip fetch -f` → unsupported option, -{flag} is not valid for {command}

## run
Run full pipeline

- `xtgeoip run` → OK
- `xtgeoip run -l` → OK
- `xtgeoip run -p` → OK
- `xtgeoip run -c -p` → OK
- `xtgeoip run -c -f` → OK
- `xtgeoip run -c -p -f` → unsupported option, ambiguous and prune does not support force
- `xtgeoip run -b -c -p` → unsupported option, ambiguous (does prune apply to {left} or {right}?)

