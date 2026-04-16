# xtgeoip

xtgeoip is a Rust reimplementation of the Perl program xt_geoip_build_maxmind (Jan Engelhardt, Philip Prindeville), but with many enhancements.

## GeoIP Filtering Preample

Public facing servers are exposed to a massive volume of constant scanning and hacking attempts. The sheer scale of these attacks makes it impractical to manage without some form of automated filtering. Analysis of this traffic typically reveals that a disproportionately large amount of it originates from certain countries.

Although it might seem unfair to block an entire country, given the scale of the problem posed by that country, the alternative would require an ongoing expenditure of resources that is untenable and simply not justifiable. This sledgehammer approach may not completely eliminate the problem, but experience shows that it reduces it to negligible levels.

GeoIP filtering is achieved using the firewall, which on Linux is iptables (or its modern replacement nftables). Once the appropriate data files have been created, and the xt_geoip kernel module has been installed and loaded, it's then simply a case of creating a firewall rule that targets "--src-cc XX" for logging or filtering, where "XX" is a valid  ISO 3166-1 country code (e.g. US), or comma separated list of them, corresponding to the data files. This will then log and/or filter all traffic from that country (or countires), at least as defined by MaxMind.

## Features

xtgeoip can:

- Download the latest GeoLite2 databases from MaxMind (e.g. GeoLite2-Country-CSV_${date}.zip), and its associated sha256 checksum file (e.g. GeoLite2-Country-CSV_${date}.zip.sha256), and store in /var/lib/xt_geoip/.
- Prune older GeoLite2 databases from /var/lib/xt_geoip/ to save disk space, keeping only the latest "n" versions (configurable, default: 3).
- Verify the integrity of the downloaded zip file using the corresponding sha256 checksum.
- Unzip the downloaded zip file and extract the relevant CSV files to a temporary directory.
- Convert the CSV data into the binary format supported by the xt_geoip Linux kernel module, one data file per country, then store them in /usr/share/xt_geoip/. The files are named according to the country code (e.g. US, CN, etc.) and the IP version (e.g. iv4, iv6), e.g. US.iv4, US.iv6, CN.iv4, CN.iv6, etc.
- Create a sha256 checksum of the generated binary files, which will be used as a manifest for backup and housekeeping purposes.
- Back up the binary files to a tarball in /var/lib/xt_geoip/, for future reference and potential rollback.
- Prune older binary tarballs from /usr/share/xt_geoip/ to save disk space, keeping only the latest n versions (configurable, default: 3).
- Delete the current binary files in /usr/share/xt_geoip/, to ensure no orphaned files are left behind. This also removes any metadata files created by xtgeoip (typically a file called "version" and the sha256 manifest file).

## Force Flag

Two operations support the "force" flag: backup and clean.

Normally, backup requires a minimum of 3 files in /usr/share/xt_geoip/: the "version" file, the sha256 manifest file, and at least one iv4/iv6 binary file, the latter of which must also be named in the manifest, and pass checksum verification. However, typically you would expect to see hundreds of iv4/iv6 files. When backup runs, all files named in the manifest must exist in /usr/share/xt_geoip/, and pass checksum verification, otherwise the backup will not run, and the program will exit with an error. However, the force flag allows you to bypass these checks, and run the backup even if the version file or manifest is missing, or if some of the iv4/iv6 files are missing or fail checksum verification. This can be useful in certain scenarios, such as when you want to create a backup of the current state of /usr/share/xt_geoip/, even if it's in a broken state.

Similarly, the clean operation normally requires that the version file and manifest file be present in /usr/share/xt_geoip/, which it then uses to delete only those files that were originally created by xtgeoip, as named in the manifest. However, as with backup, the force flag allows you to bypass these checks, and any file matching the pattern *.iv4 or *.iv6 in /usr/share/xt_geoip/ will be deleted, along with any metadata files created by xtgeoip (e.g. the version file and manifest).

## Order of Operations vs Order of Flags

Certain flags can be combined, such as -b (backup) and -c (clean). In this case, the order of operations is always to back up first, then clean, as the reverse would fail (you've just deleted the files you wanted to back up), and the order of the flags given are ignored (i.e. xtgeip -b -c == xtgeoip -c -b). Generally, the order of subcommands and flags is always ignored, and the order of exexution is fixed, based on the most logical order of operations requested.

## Context of Flags

Some flags are only relevant in certain contexts. For example, the -f (force) flag is only relevant in the context of backup and clean, and will raise an error if used in the context of fetch (downloading cannot be forced, as it either succeeds or fails). If force is used in combination with both backup and clean, it will raise an error and exit, as the request is ambiguous (do you want to force backup, or force clean, or both?). In order to avoid surprising the user with unexpected results, the intent must be explicit. Note that if you need to force both, you will therefore need to run the program twice, once with "-b -f", then again with "-c -f".

Similarly, the -p (prune) flag is only relevant in the contexts of fetch and backup. In the case of backup, it will prune older tarballs from /var/lib/xt_geoip/, while in the case of fetch, it will prune older zip files from /var/lib/xt_geoip/. Using the -p flag in any other context will raise an error and exit, e.g. in combination with build (as build neither downloads zip files nor creates tarballs). As with the force flag, if prune is used in combination with both fetch and backup, it will raise an error and exit, for the same reason of ambiguity.

## Legacy Mode

A "legacy" mode is provided, which produces output files identical to the original Perl implementation.

*WARNING*: Be aware that the original implementation incorrectly assigns some IP ranges to the wrong country.

For example, it assigns IP ranges with the AS (Asia) continent code, which is not a valid ISO 3166 country, to the country code "AS", and worse, it's an actual collision with the real country code for American Samoa, meaning that a large number of Asian IP ranges are incorrectly assigned to American Samoa.

Additionally, the original implementation bundles all EU (Europe) continent IP ranges into the "EU" pseudo country code, which is not a valid ISO 3166 country code. These are ranges without a designated country code, but which are nonetheless within the EU. The correct, ISO compliant way to handle these ranges is to assign them to the country code "O1" ("O" as in "Other"), which is the reserved country code for "other countries".

## Configuration

An example config file should be provided with this distribution, and may be located at /usr/share/xt_geoip/xtgeoip.conf.example. The config file should be copied to /etc/xtgeoip.conf. At a bare minimum, you will need to modify the following 2 parameters in the config file to get either the "run" or "fetch" modes of xtgeoip working:

- `account_id`: You will need to obtain an account ID from MaxMind (https://www.maxmind.com/en/geolite2/signup). This is required to download the GeoLite2 databases.

- `license_key`: You will be able to create a license key from your MaxMind account dashboard, once you have an account.

Note that there are both free and paid tiers of MaxMind accounts, and the free tier allows you to download the GeoLite2 databases, which are sufficient for use with xtgeoip. The paid tier allows you to download the larger and more accurate GeoIP2 databases, but these have not been tested with xtgeoip.

## Running xtgeoip

```
Commands (only one of): build
                        Optional flags: -b, -c, -f, -p
                        Create ip4/ip6 data files from a stored copy of the database

                        fetch
                        Optional flags: -p
                        Download the latest database

                        run
                        Optional flags: -b, -c, -f
                        Fetch then build

                        conf
                        Mandatory flags (only one of): -d, -e, -s, -h
                        
flags:                  -b|--backup
                        Backup ip4/ip6 data files listed in manifest
                        Optional commands: build, run

                        -c|--clean
                        Delete ip4/ip6 data files listed in manifest

                        -d|--default
                        Show the default config
                        Requires the conf command

                        -e|--edit
                        Edit the current config
                        Requires the conf command

                        -f|--force
                        Force backup or clean
                        Requires the -b or -c flag, but not both in a single invocation (ambiguous)

                        -h|--help
                        Show help

                        -l|--legacy
                        Produce legacy (incorrect) data files, for comparison

                        -p|--prune
                        Delete older binary backups or CSV databases, but not both in a single invocation (ambiguous)
                        Requires the -b flag or fetch command, but not both in a single invocation (ambiguous)

                        -s|--show
                        Show the current config
                        Requires the conf command

                        -V|--version
```


## The xt_geoip Kernel Module

To use the xt_geoip firewall kernel module, you need to load the kernel module and ensure it is loaded at boot time. You can do this by running the following commands:

sudo modprobe xt_geoip
echo "xt_geoip" | sudo tee /etc/modules-load.d/xt_geoip.conf

The module is available as part of the xtables-addons package. If this package is not available for your distro, a simple dkms source package is included with xtgeoip, in the extras/dkms directory. Please read the included install instructions for further information.

## Firewall Utilization

An example firewall config has been included in the extra/ufw directory. Please read the instructions in that directory for further information.

