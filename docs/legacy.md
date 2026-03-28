# Legacy Mode

## Overview

`xtgeoip` supports an optional legacy compatibility mode:

> -l, --legacy

Legacy mode preserves historical behaviour from the original Perl implementation, even where that behaviour is known to be semantically incorrect.

This mode exists solely for compatibility with previously generated xt\_geoip country files and for reproducing legacy checksums.

Default mode is recommended for all normal use.

## Why Legacy Mode Exists

Older versions of the MaxMind-to-xt_geoip conversion logic treated some MaxMind continent-only rows as though their continent_code values were valid country_iso_code values.

This was done to preserve historical output compatibility, but it creates data integrity problems because some MaxMind continent codes collide with real ISO 3166-1 alpha-2 country codes.

In other words:

- MaxMind continent_code values are not country codes
- Some of them happen to look like country codes
- Reusing them as country codes can silently misassign IP ranges

Current Behaviour
Default Mode (recommended)
In default mode, any MaxMind row with:

an empty country_iso_code, and

no valid country mapping

is treated as undefined / non-country data and assigned to:

O1
This includes continent-only rows.

This avoids incorrectly assigning undefined or continent-level data to unrelated real countries.

Legacy Mode (-l, --legacy)

Legacy mode preserves the historical special-case behaviour for the following MaxMind geoname_id values:

geoname_id	MaxMind meaning	Legacy output code
6255148	Europe (continent-only row)	EU
6255147	Asia (continent-only row)	AS

This behaviour is preserved only for compatibility.

It should not be interpreted as semantically correct country mapping.

## Known Collisions

The following legacy mappings are problematic:

6255148 → EU
MaxMind meaning: Europe (continent-level row, no country)

Legacy output: EU

EU is not a normal ISO 3166-1 assigned country code.
It is an exceptionally reserved alpha-2 code used for the European Union in some contexts.

Using it here effectively treats Europe as though it were a country-like output bucket.

This is retained only for historical compatibility.

6255147 → AS
MaxMind meaning: Asia (continent-level row, no country)

Legacy output: AS

AS is the real ISO 3166-1 alpha-2 country code for American Samoa.

This creates a direct collision:

continent-level Asia data

is merged into

the real country bucket for American Samoa

This is a known data integrity issue in legacy mode.

Why Default Mode Uses O1
O1 is the correct destination for undefined or non-country rows because:

- the row does not represent a specific country
- the row has no valid country_iso_code
- assigning a continent code as a country code is not reliable
- several continent codes can collide with real ISO alpha-2 codes

Examples of possible or known collisions include:

- AS (Asia vs American Samoa)
- AF (Africa vs Afghanistan)
- NA (North America vs Namibia)
- SA (South America vs Saudi Arabia)

Even where a collision is not currently used by legacy mode, treating continent codes as country codes is fundamentally unsafe.

## Compatibility Notes

If you need bit-for-bit compatibility with historical xt_geoip output, use:

> xt_geoip_build_maxmind --legacy

This mode is expected to reproduce the same country files as older Perl-based tooling, including legacy checksum matches.

If you want semantically correct handling of undefined / continent-only data, use default mode:

xt_geoip_build_maxmind
In default mode:

- no fake EU country files are created
- continent-only rows are not merged into real countries
- undefined data is assigned to O1

## Recommendation

Use default mode unless you specifically need:

- reproducible historical checksums
- compatibility with previously generated legacy outputs
- exact behaviour matching older Perl tooling

Legacy mode should be considered a compatibility shim, not the preferred interpretation of MaxMind data.
