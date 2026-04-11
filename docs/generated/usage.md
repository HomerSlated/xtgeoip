# xtgeoip

Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

## top level
Top level xtgeoip command

- `xtgeoip` → no command or top-level action specified (exit 1)
- `xtgeoip -h` → 
- `xtgeoip -b` → 
- `xtgeoip -b -c` → 
- `xtgeoip -b -c -f` → 
- `xtgeoip -b -f` → 
- `xtgeoip -b -p` → 
- `xtgeoip -b -p -f` → unsupported option, ambiguous and prune does not support force
- `xtgeoip -c` → 
- `xtgeoip -c -f` → 
- `xtgeoip -c -p` → unsupported option, {command} has no prune function
- `xtgeoip -c -p -f` → unsupported option combination
- `xtgeoip -p` → unsupported option combination
- `xtgeoip -f` → unsupported option, {target} does not support force
- `xtgeoip -l` → unsupported option, -{flag} is not valid for {command}

## build
Build xt_geoip database

- `xtgeoip build` → 
- `xtgeoip build -l` → 
- `xtgeoip build -b -p` → 
- `xtgeoip build -p` → unsupported option, {command} has no prune function
- `xtgeoip build -c -f` → 
- `xtgeoip build -b -c -p` → 
- `xtgeoip build -b -c -p -f` → unsupported option, ambiguous and prune does not support force

## conf
Manage configuration
Usage: xtgeoip conf <-s|-d|-e>

- `xtgeoip conf` → missing required argument, {command} requires {argument}
- `xtgeoip conf -s` → 
- `xtgeoip conf -d` → 
- `xtgeoip conf -e` → 

## fetch
Fetch GeoLite2 data files

- `xtgeoip fetch` → 
- `xtgeoip fetch -p` → 
- `xtgeoip fetch -l` → unsupported option, -{flag} is not valid for {command}
- `xtgeoip fetch -b` → unsupported option, -{flag} is not valid for {command}
- `xtgeoip fetch -c` → unsupported option, -{flag} is not valid for {command}
- `xtgeoip fetch -f` → unsupported option, -{flag} is not valid for {command}

## run
Run full pipeline

- `xtgeoip run` → 
- `xtgeoip run -l` → 
- `xtgeoip run -p` → 
- `xtgeoip run -c -p` → 
- `xtgeoip run -c -f` → 
- `xtgeoip run -c -p -f` → unsupported option, ambiguous and prune does not support force
- `xtgeoip run -b -c -p` → unsupported option, ambiguous (does prune apply to {left} or {right}?)

