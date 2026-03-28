# Usage
- xtgeoip # print usage
- xtgeoip -h # print usage
- xtgeoip conf # print conf usage
- xtgeoip conf -h # print conf usage
- xtgeoip conf -s # show current config
- xtgeoip conf -d # show default config
- xtgeoip conf -s # show current config
- xtgeoip conf -e # edit current config using $EDITOR
- xtgeoip -b # backup data in /usr/share/xt_geoip/, as per manifest, store backup in /var/lib/xt_geoip/
- xtgeoip -b -f # force backup data in /usr/share/xt_geoip/, even without a manifest
- xtgeoip -b -p # backup data in /usr/share/xt_geoip/, and prune old backups in /var/lib/xt_geoip/
- xtgeoip -b -p -f # unsupported option, ambiguous, cannot force prune (does force apply to backup or prune?)
- xtgeoip fetch # download latest MaxMind database
- xtgeoip fetch -p # download latest MaxMind database, then prune old archived copies
- xtgeoip run -p # fetch, build, then prune old archived copies
- xtgeoip fetch -b # unsupported option, fetch has no backup function
- xtgeoip build # build binary data in /usr/share/xt_geoip/ using local copy of database
- xtgeoip run # fetch (if needed), and build binary data in /usr/share/xt_geoip/
- xtgeoip -c # clean (delete) data in /usr/share/xt_geoip/, as per manifest
- xtgeoip -c -f # force clean (delete) data in /usr/share/xt_geoip/, even without a manifest
- xtgeoip -b -c # backup and clean, as per manifest
- xtgeoip -b -c -f # force backup and clean, even without manifest
- xtgeoip run -b -c # backup and clean using manifest, then fetch, and build
- xtgeoip run -b -c -f # force backup and clean even without manifest, then fetch, and build
- xtgeoip build -b -c # backup and clean using manifest, then  build using local copy of database
- xtgeoip build -b -c -f # force backup and clean even without manifest, then and build using local copy of database


