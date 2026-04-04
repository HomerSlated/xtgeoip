# xtgeoip

> Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

- build using local copy of database:

`xtgeoip build`

- build using local copy of database in legacy mode:

`xtgeoip build -l`

- backup, prune backups, then build:

`xtgeoip build -b -p`

- clean using manifest, then build using local copy of database:

`xtgeoip build -c -f`

- backup, prune backups, clean, then build:

`xtgeoip build -b -c -p`

- show current config:

`xtgeoip conf -s`

- show default config:

`xtgeoip conf -d`

- edit config:

`xtgeoip conf -e`

- fetch CSVs:

`xtgeoip fetch`

- fetch CSVs, then prune CSVs:

`xtgeoip fetch -p`

- fetch, then build:

`xtgeoip run`

- fetch, then build in legacy mode:

`xtgeoip run -l`

- fetch, then prune CSVs, then build:

`xtgeoip run -p`

- clean using manifest, then fetch, then prune CSVs, then build:

`xtgeoip run -c -p`

- force clean without manifest, then fetch, then build:

`xtgeoip run -c -f`

- display version:

`xtgeoip -V`

