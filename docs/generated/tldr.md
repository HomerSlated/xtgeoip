# xtgeoip

> Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

- display usage:

`xtgeoip`

- display usage:

`xtgeoip -h`

- back up binary data in /usr/share/xt_geoip/ using the manifest; store backups in /var/lib/xt_geoip/:

`xtgeoip -b`

- back up and clean binary data using the manifest:

`xtgeoip -b -c`

- force backup and clean even without a manifest:

`xtgeoip -b -c -f`

- force backup of binary data in /usr/share/xt_geoip/ even without a manifest:

`xtgeoip -b -f`

- back up binary data in /usr/share/xt_geoip/, then prune old backups in /var/lib/xt_geoip/:

`xtgeoip -b -p`

- clean (delete) binary data in /usr/share/xt_geoip/ using the manifest:

`xtgeoip -c`

- force clean (delete) binary data in /usr/share/xt_geoip/ even without a manifest:

`xtgeoip -c -f`

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

- fetch CSVs, then prune CSV archives:

`xtgeoip fetch -p`

- fetch, then build:

`xtgeoip run`

- fetch, then build in legacy mode:

`xtgeoip run -l`

- fetch, then prune CSV archives, then build:

`xtgeoip run -p`

- clean using manifest, then fetch, then prune CSV archives, then build:

`xtgeoip run -c -p`

- force clean without manifest, then fetch, then build:

`xtgeoip run -c -f`

- display version:

`xtgeoip -V`

