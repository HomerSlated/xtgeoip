# xtgeoip

> Build and manage xt_geoip data from MaxMind GeoLite2 CSVs.

- display usage:

`xtgeoip -h`

- backup database:

`xtgeoip -b`

- clean database:

`xtgeoip -c`

- backup then clean:

`xtgeoip -b -c`

- backup then prune:

`xtgeoip -b -p`

- force backup database:

`xtgeoip -b -f`

- force backup then clean:

`xtgeoip -b -c -f`

- build database:

`xtgeoip build`

- build (legacy mode):

`xtgeoip build -l`

- backup then prune then build:

`xtgeoip build -b -p`

- clean then build:

`xtgeoip build -c -f`

- backup, prune, clean, build:

`xtgeoip build -b -c -p`

- backup then build:

`xtgeoip build -b`

- clean then build:

`xtgeoip build -c`

- backup then clean then build:

`xtgeoip build -b -c`

- force backup then build:

`xtgeoip build -b -f`

- force backup then clean then build:

`xtgeoip build -b -c -f`

- show configuration:

`xtgeoip conf -s`

- show default configuration:

`xtgeoip conf -d`

- edit configuration:

`xtgeoip conf -e`

- fetch CSVs:

`xtgeoip fetch`

- fetch then prune archives:

`xtgeoip fetch -p`

- fetch then build:

`xtgeoip run`

- fetch then build (legacy):

`xtgeoip run -l`

- fetch then prune then build:

`xtgeoip run -p`

- clean then fetch then prune then build:

`xtgeoip run -c -p`

- force clean then fetch then build:

`xtgeoip run -c -f`

- backup then fetch then build:

`xtgeoip run -b`

- backup then clean then fetch then build:

`xtgeoip run -b -c`

- force backup then fetch then build:

`xtgeoip run -b -f`

- backup then fetch then prune then build:

`xtgeoip run -b -p`

- force backup then clean then fetch then build:

`xtgeoip run -b -c -f`

