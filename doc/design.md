# xtgeoip Design Specification

## 1. Archive Version Verification

**Objective:** Determine whether the MaxMind GeoLite2-Country CSV archive has been updated, without downloading the entire file unnecessarily.

**Method Overview:**

1. **Check for Existing Local Archive**  
   - Local archives are stored in `/var/lib/xt_geoip/`, with filenames derived from the original archive’s date, e.g., `GeoLite2-Country-CSV_20260227.zip`.
   - The presence of a versioned file indicates a previously downloaded copy.

2. **Fetch Remote Metadata**  
   - A HTTP `GET` request is sent to the MaxMind GeoLite2-Country CSV download URL.
   - Only headers are initially inspected (the body is streamed to a temporary location or discarded until needed).

3. **Extract Remote Version from Content-Disposition**  
   - The `Content-Disposition` header from the HTTP response contains the archive filename, e.g.:
     ```
     Content-Disposition: attachment; filename=GeoLite2-Country-CSV_20260227.zip
     ```
   - The date portion of the filename (e.g., `20260227`) is parsed to determine the remote archive version.

4. **Compare Remote Version with Local Copy**  
   - If a local file with the same version exists, the archive is considered up-to-date; no download is performed.
   - If the remote version differs or no local archive exists, the archive is downloaded and saved in `/var/lib/xt_geoip/`, using the date-based versioned filename.

5. **Notes on Implementation**  
   - `HEAD` requests were considered to save bandwidth, but MaxMind’s server behavior requires using `GET` to reliably obtain the `Content-Disposition` header.
   - Parsing is robust to missing headers via fallback (empty string), preventing crashes.
   - This method avoids redundant downloads, reducing network load and speeding up repeated runs.

**Example Flow:**

1. Local check: `/var/lib/xt_geoip/GeoLite2-Country-CSV_20260227.zip` exists → proceed to step 4.  
2. Remote check: GET `https://download.maxmind.com/geoip/databases/GeoLite2-Country-CSV/download` → headers received.  
3. Parse `Content-Disposition` → filename: `GeoLite2-Country-CSV_20260227.zip`.  
4. Compare with local version → match found → skip download.
