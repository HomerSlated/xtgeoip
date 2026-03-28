# xtgeoip

> Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

- build using local copy of database:

`xtgeoip build`

- backup, prune backups, then build:

`xtgeoip build -b -p`

- clean using manifest, then build using local copy of database:

`xtgeoip build -c -f`

- backup, prune backups, clean, then build:

`xtgeoip build -b -c -p`

- fetch CSVs:

`xtgeoip fetch`

- fetch CSVs, then prune CSVs:

`xtgeoip fetch -p`

- fetch, then build:

`xtgeoip run`

- fetch, then prune CSVs, then build:

`xtgeoip run -p`

- clean using manifest, then fetch, then prune CSVs, then build:

`xtgeoip run -c -p`

- force clean without manifest, then fetch, then build:

`xtgeoip run -c -f`

- show configured paths and status:

`xtgeoip show`

