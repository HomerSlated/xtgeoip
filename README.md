# xtgeoip

`xtgeoip` is a Rust-based tool for managing GeoIP-based filtering on Linux systems. It automates the creation and maintenance of binary GeoIP databases compatible with the `xt_geoip` kernel module, enabling network administrators to block or allow traffic by country with high performance and precision.

## Background

This project was inspired by [`xt_geoip_build_maxmind`](https://manpages.debian.org/unstable/xtables-addons-common/xt_geoip_build_maxmind.1.en.html) (Jan Engelhardt, 2008–2011 & Philip Prindeville, 2018), which is now part of Debian's `xtables-addons-common`. While that tool is written in Perl, `xtgeoip` reimplements the functionality in Rust, offering modern safety, concurrency, and performance improvements.

## GeoIP Filtering and Linux xtables

GeoIP filtering allows network administrators to restrict access based on the geographical origin of IP addresses. On Linux, this is typically implemented using the `xt_geoip` kernel module in conjunction with `iptables` or `nftables`. For example, traffic from specific countries can be blocked or allowed by referencing the precompiled GeoIP database.

For users of `ufw` (Uncomplicated Firewall), `xtgeoip` can be integrated to provide country-based rules by generating the necessary `iptables` or `nftables` commands.

### Note on Ethics and Usage

Blocking entire countries is a contentious practice and should be considered carefully. While unfortunate, in some cases it has become necessary for public-facing servers to reduce large-scale attacks or abuse. Administrators should use these tools responsibly and remain mindful of the broader implications of country-based filtering.

## Features

- **Legacy mode support**: Compatible with older workflows.
- **Accurate ISO-based classification**: Correctly separates American Samoa (`AS`) from Asia and places continent-only ranges into `O1`.
- **Flexible backup and delete commands**: Ensures safe management of GeoIP binary data.
- **Context-aware error handling**: Detects missing version or manifest files and provides actionable messages.
- **Multiple operation modes**: Including `backup`, `delete`, `run`, and planned future modes like `fetch only` and `build only`.

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

## Optimisations and Coding Decisions

- Efficient glob-based file collection for `.iv4` and `.iv6` data files.
- Verification of manifest checksums to prevent database corruption.
- Force mode for safe operations when version or manifest files are missing.
- Minimal filesystem overhead during tarball creation.

## Roadmap

Future enhancements may include:

- Dedicated `fetch only` mode to download GeoIP data without processing. (DONE)
- `build only` mode for rapid binary database construction. (DONE)

## Status

This software is considered **beta quality**, though it is fully functional. Feedback, bug reports, contributions, and packaging efforts are highly welcome.

## License

This project is licensed under the [MIT License](LICENSE).

