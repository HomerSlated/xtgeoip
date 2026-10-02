# xtgeoip

xtgeoip is a Rust reimplementation of the Perl program xt_geoip_build_maxmind (Jan Engelhardt, Philip Prindeville), but with many enhancements.

## GeoIP Filtering Preamble

Public facing servers are exposed to a massive volume of constant scanning and hacking attempts. The sheer scale of these attacks makes it impractical to manage without some form of automated filtering. Analysis of this traffic typically reveals that a disproportionately large amount of it originates from certain countries.

Although it might seem unfair to block an entire country, given the scale of the problem posed by that country, the alternative would require an ongoing expenditure of resources that is untenable and simply not justifiable. This sledgehammer approach may not completely eliminate the problem, but experience shows that it reduces it to negligible levels.

GeoIP filtering is achieved using the firewall, which on Linux is iptables (or its modern replacement nftables). Once the appropriate data files have been created, and the xt_geoip kernel module has been installed and loaded, it's then simply a case of creating a firewall rule that targets "--src-cc XX" for logging or filtering, where "XX" is a valid ISO 3166-1 country code (e.g. US), or comma separated list of them, corresponding to the data files. This will then log and/or filter all traffic from that country (or countries), at least as defined by MaxMind.

## Speed Comparison
**High-performance Rust implementation**: Significantly faster than the Perl version.

```bash
$ time sudo /usr/libexec/xtables-addons/xt_geoip_build_maxmind -s
...
Executed in   45.48 secs

$ time sudo xtgeoip run
...
Executed in    1.84 secs
```

## Features

xtgeoip can:

- Download the latest GeoLite2 databases from MaxMind (e.g. GeoLite2-Country-CSV_${date}.zip), and its associated sha256 checksum file (e.g. GeoLite2-Country-CSV_${date}.zip.sha256), and store in /var/lib/xt_geoip/.
- Prune older GeoLite2 databases from /var/lib/xt_geoip/ to save disk space, keeping only the latest "n" versions (configurable, default: 3).
- Verify the integrity of the downloaded zip file using the corresponding sha256 checksum.
- Unzip the downloaded zip file and extract the relevant CSV files to a temporary directory.
- Convert the CSV data into the binary format supported by the xt_geoip Linux kernel module, one data file per country, then store them in /usr/share/xt_geoip/. The files are named according to the country code (e.g. US, CN, etc.) and the IP version (e.g. iv4, iv6), e.g. US.iv4, US.iv6, CN.iv4, CN.iv6, etc.
- Create a Blake3 checksum of the generated binary files, which will be used as a manifest for backup and housekeeping purposes.
- Back up the binary files to a tarball in /var/lib/xt_geoip/, for future reference and potential rollback.
- Prune older binary tarballs from /var/lib/xt_geoip/ to save disk space, keeping only the latest n versions (configurable, default: 3).
- Delete the current binary files in /usr/share/xt_geoip/, to ensure no orphaned files are left behind. This also removes any metadata files created by xtgeoip (typically a file called "version" and the Blake3 manifest file).

## Force Flag

Two operations support the "force" flag: backup and clean.

Normally, backup requires a minimum of 3 files in /usr/share/xt_geoip/: the "version" file, the Blake3 manifest file, and at least one iv4/iv6 binary file, the latter of which must also be named in the manifest, and pass checksum verification. However, typically you would expect to see hundreds of iv4/iv6 files. When backup runs, all files named in the manifest must exist in /usr/share/xt_geoip/, and pass checksum verification, otherwise the backup will not run, and the program will exit with an error. However, the force flag allows you to bypass these checks, and run the backup even if the version file or manifest is missing, or if some of the iv4/iv6 files are missing or fail checksum verification. This can be useful in certain scenarios, such as when you want to create a backup of the current state of /usr/share/xt_geoip/, even if it's in a broken state.

Similarly, the clean operation normally requires that the version file and manifest file be present in /usr/share/xt_geoip/, which it then uses to delete only those files that were originally created by xtgeoip, as named in the manifest. However, as with backup, the force flag allows you to bypass these checks, and any file matching the pattern *.iv4 or *.iv6 in /usr/share/xt_geoip/ will be deleted, along with any metadata files created by xtgeoip (e.g. the version file and manifest).

## Order of Operations vs Order of Flags

Certain flags can be combined, such as -b (backup) and -c (clean). In this case, the order of operations is always to back up first, then clean, as the reverse would fail (you've just deleted the files you wanted to back up), and the order of the flags given is ignored (i.e. xtgeoip -b -c == xtgeoip -c -b). Generally, the order of subcommands and flags is always ignored, and the order of execution is fixed, based on the most logical order of operations requested.

## Context of Flags

Some flags are only relevant in certain contexts. For example, the -f (force) flag is only relevant in the context of backup and clean, and will raise an error if used in the context of fetch (downloading cannot be forced, as it either succeeds or fails). If force is used in combination with both backup and clean, it will raise an error and exit, as the request is ambiguous (do you want to force backup, or force clean, or both?). In order to avoid surprising the user with unexpected results, the intent must be explicit. Note that if you need to force both, you will therefore need to run the program twice, once with "-b -f", then again with "-c -f".

Similarly, the -p (prune) flag is only relevant in the contexts of fetch and backup. In the case of backup, it will prune older bin tarballs from /var/lib/xt_geoip/, while in the case of fetch, it will prune older CSV zip files from /var/lib/xt_geoip/. The prune target is therefore determined by context: backup implies bin archives; fetch implies CSV archives. This extends to compound operations: `run -p` prunes CSV archives (after the implicit fetch, before build); `build -b -p` prunes bin archives (after backup, before build). Using -p without a fetch or backup context will raise an error and exit (e.g. `build -p` without -b). As with the force flag, if prune is used in a context where both fetch and backup are active, it will raise an error and exit for the same reason of ambiguity (ambiguous prune target).

## Legacy Mode

A "legacy" mode is provided, which produces output files identical to the original Perl implementation.

*WARNING*: Be aware that the original implementation incorrectly assigns some IP ranges to the wrong country.

For example, it assigns IP ranges with the AS (Asia) continent code, which is not a valid ISO 3166 country, to the country code "AS", and worse, it's an actual collision with the real country code for American Samoa, meaning that a large number of Asian IP ranges are incorrectly assigned to American Samoa.

Additionally, the original implementation bundles all EU (Europe) continent IP ranges into the "EU" pseudo country code, which is not a valid ISO 3166 country code. These are ranges without a designated country code, but which are nonetheless within the EU. The correct, ISO compliant way to handle these ranges is to assign them to the country code "O1" ("O" as in "Other"), which is the reserved country code for "other countries".

## Configuration

xtgeoip reads its configuration from /etc/xtgeoip.conf (TOML). A documented example is installed at /usr/share/xt_geoip/xtgeoip.conf.example, and `xtgeoip conf -d` prints it.

To download the GeoLite2 databases you need a MaxMind account (https://www.maxmind.com/en/geolite2/signup) and a license key, which you create from the account dashboard. Then run:

```
sudo xtgeoip conf -c
```

If /etc/xtgeoip.conf does not exist yet, this offers to create it from the example. It then asks for your account ID, your license key and a passphrase, encrypts the account ID and license key under that passphrase, and writes them into /etc/xtgeoip.conf as `[maxmind.credentials]`. The credentials are never stored in plaintext, so do not add `account_id` or `license_key` to the file by hand.

The passphrase is stored nowhere, so `fetch` and `run` ask for it each time they contact MaxMind, and cannot run unattended (for example from cron). `build` works from the archived copy and needs no passphrase.

`xtgeoip conf -s` shows the active configuration, and `xtgeoip conf -e` opens it in `$EDITOR` (vi if unset). The remaining settings (the archive and output directories, how many archives to keep, the log file, and the number of worker threads) are documented in the example file and in xtgeoip(1).

Note that there are both free and paid tiers of MaxMind accounts, and the free tier allows you to download the GeoLite2 databases, which are sufficient for use with xtgeoip. The paid tier allows you to download the larger and more accurate GeoIP2 databases, but these have not been tested with xtgeoip.

## Running xtgeoip

This is the program's own help, from `xtgeoip --help`:

```
$ xtgeoip --help
Build and manage xt_geoip data from MaxMind GeoLite2 CSVs

Usage: xtgeoip [OPTIONS]
       xtgeoip <COMMAND>

Commands:
  run    Fetch then build the full pipeline
  build  Build binary database from local CSV archive
  fetch  Download GeoLite2 CSV archive from MaxMind
  conf   Manage system configuration

Options:
  -b, --backup
          Back up current database before replacing it

  -c, --clean
          Delete current binary database files

  -f, --force
          Force the operation (overrides safety checks)
          
          With `build -c`, extends the clean to stale-owned files — those left by a previous manifest, such as EU.iv4/EU.iv6 after leaving legacy mode. It does not widen the clean to files xtgeoip did not create: eligibility is structural (extension iv4/iv6 and a two-character [A-Z0-9] stem), so --force cannot reach an unowned file.

  -l, --legacy
          Enable legacy mode (historical compatibility only)
          
          Switching back to default mode leaves EU.iv4/EU.iv6 behind; they are listed in the `orphaned` file. Which clean form removes them depends on when you act, because --clean runs before build regenerates the manifest: `build -c` in the same invocation that leaves legacy mode (still owned), or `build -c -f` afterwards (stale-owned, needs the glob). Files xtgeoip did not create are never touched by either. See xtgeoip(1), FILE OWNERSHIP and LEGACY MODE.

  -p, --prune
          Prune old bin archives (requires --backup)

      --log-file <PATH>
          Write the log to PATH, overriding [logging] in the config
          
          Takes precedence over `log_file` in `/etc/xtgeoip.conf`. Because the override is known before the config is read, it also captures a config-load failure — which the configured path cannot, since that path is only known once the load has succeeded.

      --no-log
          Disable file logging, overriding [logging] in the config
          
          Terminal output is unaffected: `init_logger` always installs the stdout/stderr dispatches, and only the file sink is conditional (#1).

      --config <PATH>
          Read configuration from PATH instead of /etc/xtgeoip.conf
          
          Applies to every command, `conf` included: `conf --show --config P` shows `P`, and `conf --set-credentials --config P` encrypts into `P`. A flag that redirected reads but not writes would be a trap.
          
          This is what makes the integration suite able to run against a temporary tree instead of the live `/usr/share/xt_geoip` and `/var/lib/xt_geoip` — `output_dir` and `archive_dir` are ordinary `[paths]` keys, so redirecting the config redirects everything the program touches. It is a general capability rather than a test hook, which is why it is documented rather than hidden: an operator running a non-default install has the same need.

      --ca-file <PATH>
          Verify the MaxMind server against the CA bundle in PATH
          
          Replaces the system trust roots for this run rather than adding to them: naming a CA says which CA to trust, and merging would let a wrong or stale bundle succeed against a different anchor than the one named. Verification is otherwise unchanged — chain, hostname and validity are still checked, against these roots instead of the system's. It narrows what is trusted; it never disables a check.
          
          For an installation behind a TLS-intercepting proxy, or against a private mirror with a self-signed certificate. It is also what lets the integration suite verify a local https stub without touching the host's trust configuration, which `SSL_CERT_FILE` cannot do: the suite spawns cases via `sudo`, and `sudo` resets the environment while passing arguments through unchanged.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

The `conf` command's flags are listed by its own help:

```
$ xtgeoip conf -h
Manage system configuration

Usage: xtgeoip conf [OPTIONS]

Options:
  -d, --default          Show default (example) configuration
  -s, --show             Show system configuration
  -e, --edit             Open system configuration in $EDITOR
  -c, --set-credentials  Encrypt and store MaxMind account_id/license_key (#103)
      --log-file <PATH>  Write the log to PATH, overriding [logging] in the config
      --no-log           Disable file logging, overriding [logging] in the config
      --config <PATH>    Read configuration from PATH instead of /etc/xtgeoip.conf
      --ca-file <PATH>   Verify the MaxMind server against the CA bundle in PATH
  -h, --help             Print help (see more with '--help')
```

Each command has its own `-h` and `--help`.

## The xt_geoip Kernel Module

To use the xt_geoip firewall kernel module, you need to load the kernel module and ensure it is loaded at boot time. You can do this by running the following commands:

sudo modprobe xt_geoip
echo "xt_geoip" | sudo tee /etc/modules-load.d/xt_geoip.conf

The module is available as part of the xtables-addons package. If this package is not available for your distro, a simple dkms source package is included in the extra/dkms directory of the git repository (https://github.com/HomerSlated/xtgeoip/tree/main/extra/dkms). It is not part of the release tarball. Please read the included install instructions for further information.

## Firewall Utilization

An example firewall config has been included in the extra/ufw directory of the git repository (https://github.com/HomerSlated/xtgeoip/tree/main/extra/ufw). It is not part of the release tarball. Please read the instructions in that directory for further information.

