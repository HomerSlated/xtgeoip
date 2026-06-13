/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// Typed version token extracted from GeoLite2 archive filenames.

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version(String);

impl Version {
    /// Extract the version token from a GeoLite2 archive filename.
    ///
    /// Takes the segment after the last `_` and before the first `.` as an
    /// opaque token; rejects empty tokens and tokens containing path
    /// separators.
    ///
    /// Examples:
    /// - `GeoLite2-Country-CSV_20260324.zip`               → `"20260324"`
    /// - `GeoLite2-Country-CSV_20260324.zip.sha256`        → `"20260324"`
    /// - `GeoLite2-Country-bin_20260324.tar.gz`            → `"20260324"`
    /// - `GeoLite2-Country-bin_unverified_20260324.tar.gz` → `"20260324"`
    pub fn parse(name: &str) -> Option<Self> {
        let idx = name.rfind('_')?;
        let after = &name[idx + 1..];
        let end = after.find('.').unwrap_or(after.len());
        let token = &after[..end];
        if token.is_empty() || token.contains('/') || token.contains('\\') {
            None
        } else {
            Some(Self(token.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn bin_manifest_name(&self) -> String {
        format!("GeoLite2-Country-bin_{}.blake3", self.0)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_csv_zip() {
        assert_eq!(
            Version::parse("GeoLite2-Country-CSV_20260324.zip")
                .unwrap()
                .as_str(),
            "20260324"
        );
    }

    #[test]
    fn parse_csv_sha256() {
        assert_eq!(
            Version::parse("GeoLite2-Country-CSV_20260324.zip.sha256")
                .unwrap()
                .as_str(),
            "20260324"
        );
    }

    #[test]
    fn parse_bin_tar() {
        assert_eq!(
            Version::parse("GeoLite2-Country-bin_20260324.tar.gz")
                .unwrap()
                .as_str(),
            "20260324"
        );
    }

    #[test]
    fn parse_bin_unverified() {
        assert_eq!(
            Version::parse("GeoLite2-Country-bin_unverified_20260324.tar.gz")
                .unwrap()
                .as_str(),
            "20260324"
        );
    }

    #[test]
    fn parse_rejects_empty_token() {
        assert!(Version::parse("GeoLite2-Country-CSV_.zip").is_none());
    }

    #[test]
    fn parse_rejects_no_underscore() {
        assert!(Version::parse("nogoodname.zip").is_none());
    }

    #[test]
    fn parse_rejects_path_separator() {
        assert!(Version::parse("foo_bar/baz.zip").is_none());
        assert!(Version::parse("foo_bar\\baz.zip").is_none());
    }

    #[test]
    fn bin_manifest_name_format() {
        let v = Version::parse("GeoLite2-Country-CSV_20260324.zip").unwrap();
        assert_eq!(
            v.bin_manifest_name(),
            "GeoLite2-Country-bin_20260324.blake3"
        );
    }

    #[test]
    fn display_yields_token() {
        let v = Version::parse("GeoLite2-Country-CSV_20260324.zip").unwrap();
        assert_eq!(v.to_string(), "20260324");
    }

    #[test]
    fn ordering_by_token() {
        let older =
            Version::parse("GeoLite2-Country-CSV_20260101.zip").unwrap();
        let newer =
            Version::parse("GeoLite2-Country-CSV_20260606.zip").unwrap();
        assert!(older < newer);
    }
}
