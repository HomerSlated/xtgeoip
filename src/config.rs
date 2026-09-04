/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip configuration data and loading. Pure: no output, no
/// subprocesses, no prompts — see `conf.rs` for the `conf` subcommand
/// handler.
use std::{fs, ops::Range, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub(crate) const SYSTEM_CONFIG: &str = "/etc/xtgeoip.conf";

pub(crate) fn system_config_path() -> &'static Path {
    Path::new(SYSTEM_CONFIG)
}

#[derive(Debug, Deserialize)]
pub struct Paths {
    pub archive_dir: String,
    pub archive_prune: usize,
    pub output_dir: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaxMind {
    pub url: String,
    /// `None` until `xtgeoip conf --set-credentials` has been run. Not
    /// required at parse time so a freshly-installed config (which has no
    /// `[maxmind.credentials]` table yet) still loads — `fetch` reports the
    /// friendlier "run --set-credentials" message itself rather than a raw
    /// TOML "missing field" error.
    pub credentials: Option<Credentials>,
}

/// MaxMind `account_id`/`license_key`, encrypted at rest (#103). Everything
/// here is either a cost parameter or hex-encoded ciphertext — nothing in
/// this struct is secret, so deriving `Debug` is safe (contrast the
/// plaintext fields this replaced, which were not).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Debug, Deserialize)]
pub struct Logging {
    pub log_file: String,
}

#[derive(Debug, Deserialize)]
pub struct Processing {
    /// Number of Rayon worker threads. 0 or absent = use all available cores.
    pub threads: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub paths: Paths,
    pub maxmind: MaxMind,
    pub logging: Option<Logging>,
    pub processing: Option<Processing>,
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.paths.archive_dir.trim().is_empty() {
            bail!("paths.archive_dir must not be empty");
        }
        if self.paths.output_dir.trim().is_empty() {
            bail!("paths.output_dir must not be empty");
        }
        if self.maxmind.url.trim().is_empty() {
            bail!("maxmind.url must not be empty");
        }
        // The license key is sent as HTTP basic auth on the first request, so
        // a cleartext origin exposes it before any redirect is involved
        // (guardian F-3, #102). `fetch::redirect_policy` refuses an https→http
        // *downgrade*, but it cannot help when the origin is already http:
        // there is no https predecessor to downgrade from. The two checks are
        // complementary halves of one property.
        //
        // Enforced here rather than with reqwest's `.https_only(true)` so that
        // `fetch()` stays scheme-agnostic — its mock-server tests drive
        // `http://127.0.0.1` — and so the change stays out of guardian-signed
        // `fetch.rs`.
        //
        // Scheme comparison is case-insensitive: RFC 3986 defines schemes as
        // case-insensitive, so `HTTPS://` is valid and must pass.
        if !self
            .maxmind
            .url
            .trim()
            .to_ascii_lowercase()
            .starts_with("https://")
        {
            bail!(
                "maxmind.url must use https — the MaxMind license key is sent \
                 as HTTP basic auth and would otherwise cross the network in \
                 cleartext (got {:?})",
                self.maxmind.url.trim()
            );
        }
        if let Some(creds) = &self.maxmind.credentials {
            if creds.salt.trim().is_empty() {
                bail!("maxmind.credentials.salt must not be empty");
            }
            if creds.nonce.trim().is_empty() {
                bail!("maxmind.credentials.nonce must not be empty");
            }
            if creds.ciphertext.trim().is_empty() {
                bail!("maxmind.credentials.ciphertext must not be empty");
            }
        }
        Ok(())
    }
}

/// The plaintext credential fields `#103` replaced with
/// `[maxmind.credentials]`. Their presence means the host was upgraded to a
/// post-`#103` binary but never migrated.
const LEGACY_CREDENTIAL_FIELDS: [&str; 2] = ["account_id", "license_key"];

/// 1-based (line, column) of byte `offset` within `source`.
///
/// `toml::de::Error` knows this too, but only computes it inside its
/// `Display`, in the same breath as quoting the offending source line — the
/// thing [`sanitize_toml_error`] exists to prevent. Iterating `char_indices`
/// rather than slicing keeps a span landing mid-codepoint from panicking.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let (mut line, mut column) = (1, 1);
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// Render a TOML parser failure as a message that cannot contain config
/// file content (#104).
///
/// `toml::de::Error` carries an `Arc<str>` of the *whole* input, and its
/// `Display` quotes the offending line verbatim — value included. On an
/// unmigrated host that line can be `license_key = "<the real key>"`. Since
/// the error owns the file, no formatting choice at the printing end can
/// help; the fix is to never let it into the chain. So take the two pieces
/// that are safe — the message and the span — and build a fresh string.
///
/// Takes them loose rather than taking an error, because `toml::de::Error`
/// and `toml_edit::TomlError` are distinct types with identical accessors
/// that leak identically: `conf.rs` parses the same file with the other one.
///
/// The message is nearly structural — with `deny_unknown_fields` a plaintext
/// credential is an *unknown field*, reported by name and never by value —
/// but serde's type-mismatch text does embed the offending value, so it goes
/// through [`redact_quoted_values`] first.
pub(crate) fn sanitize_toml_error(
    source: &str,
    message: &str,
    span: Option<Range<usize>>,
) -> String {
    let message = redact_quoted_values(message);
    match span {
        Some(span) => {
            let (line, column) = line_col(source, span.start);
            format!(
                "Failed to parse {SYSTEM_CONFIG} at line {line}, column \
                 {column}: {message}"
            )
        }
        None => format!("Failed to parse {SYSTEM_CONFIG}: {message}"),
    }
}

/// Blank out the contents of every double-quoted run in a parser message.
///
/// `toml::de::Error::message()` is *mostly* structural — field and key names
/// arrive in backticks — but serde's own type-mismatch text is built with
/// `Debug`, so it embeds the offending value: `invalid type: string "<the
/// value>", expected u32`. Every known field today holds something
/// non-secret, so nothing reachable leaks; relying on that would make this
/// module's safety depend on a promise about fields not yet written.
/// Redacting instead makes it a property of the code.
///
/// The split is by quoting style, which is why it holds: values are
/// double-quoted, names and expected literals are backticked. Over-redaction
/// is the failure direction, and it costs only detail in a message that
/// still carries the error kind and an exact position.
fn redact_quoted_values(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            out.push(c);
            continue;
        }
        // A `"` *inside* backticks is one of those expected literals — the
        // parser rendering `expected `"`` for an unterminated string — not
        // the opening of a value. Redacting from there would swallow the
        // rest of an otherwise clean message.
        if out.ends_with('`') && chars.peek() == Some(&'`') {
            out.push(c);
            continue;
        }
        out.push_str("\"<redacted>\"");
        // Consume through the closing quote, honouring `\"` escapes; an
        // unterminated run simply redacts to end of message.
        let mut escaped = false;
        for c in chars.by_ref() {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                break;
            }
        }
    }
    out
}

/// Which `#103` legacy plaintext credential fields are still under
/// `[maxmind]`, in `LEGACY_CREDENTIAL_FIELDS` order (not the file's — the
/// untyped parse goes through a sorted map). Empty if the file will not parse
/// as TOML at all, in which case the caller falls back to the sanitized parse
/// error — a syntax error is not evidence of a migration state.
fn legacy_plaintext_credentials(source: &str) -> Vec<&'static str> {
    let Ok(doc) = source.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(maxmind) = doc.get("maxmind").and_then(|m| m.as_table()) else {
        return Vec::new();
    };
    LEGACY_CREDENTIAL_FIELDS
        .into_iter()
        .filter(|field| maxmind.contains_key(*field))
        .collect()
}

/// Parse and validate config text. Split out of [`load_config`] — which can
/// only ever read the hardcoded `/etc/xtgeoip.conf` — so the invariant that
/// matters here is directly testable: no error this returns contains any
/// content from `source`. See `errors_never_echo_config_source`.
fn parse_config(source: &str) -> Result<Config> {
    let cfg: Config = toml::from_str(source).map_err(|e| {
        // A parse failure on an unmigrated host is not a typo, it is a
        // missed migration step — nothing else detects that state, and the
        // raw parser error ("unknown field `account_id`") describes the
        // symptom rather than the action. Say what to run instead.
        let legacy = legacy_plaintext_credentials(source);
        if legacy.is_empty() {
            anyhow::anyhow!(
                "{}",
                sanitize_toml_error(source, e.message(), e.span())
            )
        } else {
            anyhow::anyhow!(
                "{SYSTEM_CONFIG} still holds MaxMind credentials in plaintext \
                 ({}). Run `sudo xtgeoip conf --set-credentials` to encrypt \
                 them; it removes the plaintext as it writes. Treat the old \
                 key as exposed and consider rotating it at MaxMind.",
                legacy.join(" and ")
            )
        }
    })?;

    cfg.validate()?;

    Ok(cfg)
}

/// Load the TOML configuration into a Config struct
pub fn load_config() -> Result<Config> {
    let path = system_config_path();

    if !path.exists() {
        anyhow::bail!("{} missing", SYSTEM_CONFIG);
    }

    let contents = fs::read_to_string(path)
        .context("Failed to read system configuration file")?;

    if contents.trim().is_empty() {
        anyhow::bail!("{} is empty", SYSTEM_CONFIG);
    }

    parse_config(&contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_url(url: &str) -> Config {
        Config {
            paths: Paths {
                archive_dir: "/var/lib/xt_geoip".into(),
                archive_prune: 3,
                output_dir: "/usr/share/xt_geoip".into(),
            },
            maxmind: MaxMind {
                url: url.into(),
                credentials: None,
            },
            logging: None,
            processing: None,
        }
    }

    #[test]
    fn https_url_is_accepted() {
        assert!(
            config_with_url(
                "https://download.maxmind.com/geoip/databases/\
                 GeoLite2-Country-CSV/download"
            )
            .validate()
            .is_ok()
        );
    }

    /// RFC 3986 schemes are case-insensitive, so this is a valid https URL and
    /// rejecting it would be wrong.
    #[test]
    fn uppercase_scheme_is_accepted() {
        assert!(
            config_with_url("HTTPS://download.maxmind.com/x")
                .validate()
                .is_ok()
        );
    }

    /// The finding this closes (#102): a cleartext origin sends the license
    /// key before any redirect exists to be checked.
    #[test]
    fn http_url_is_rejected() {
        let err = config_with_url("http://download.maxmind.com/x")
            .validate()
            .expect_err("http must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("https"), "unhelpful error: {msg}");
        assert!(
            msg.contains("cleartext"),
            "error should say why, not just what: {msg}"
        );
    }

    /// Loopback gets no exception — the decision was "no exception", so a
    /// local http mirror must be fronted with https rather than special-cased.
    #[test]
    fn http_loopback_is_also_rejected() {
        assert!(
            config_with_url("http://127.0.0.1:8080/download")
                .validate()
                .is_err()
        );
        assert!(
            config_with_url("http://localhost/download")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn other_schemes_are_rejected() {
        for url in ["ftp://example.com/x", "file:///tmp/x", "download"] {
            assert!(
                config_with_url(url).validate().is_err(),
                "accepted non-https url {url:?}"
            );
        }
    }

    /// Leading/trailing whitespace must not smuggle a bad scheme past the
    /// check, and must not fail a good one.
    #[test]
    fn surrounding_whitespace_is_ignored() {
        assert!(
            config_with_url("  https://example.com/x  ")
                .validate()
                .is_ok()
        );
        assert!(
            config_with_url("  http://example.com/x  ")
                .validate()
                .is_err()
        );
    }

    /// The empty check must still fire first, with its own message.
    #[test]
    fn empty_url_still_reports_as_empty() {
        let err = config_with_url("   ").validate().expect_err("must reject");
        assert!(
            err.to_string().contains("must not be empty"),
            "empty url should report emptiness, not scheme: {err}"
        );
    }

    // ---- #104: config-load errors must not echo config content ----

    /// A value no real config would contain, so `contains` is a sound test.
    const SENTINEL: &str = "SuPeRsEcReT_KEY_abcdef0123456789";

    fn legacy_config(maxmind_body: &str) -> String {
        format!(
            "[paths]\narchive_dir = \"/var/lib/xt_geoip\"\narchive_prune = \
             3\noutput_dir = \
             \"/usr/share/xt_geoip\"\n\n[maxmind]\n{maxmind_body}"
        )
    }

    /// The finding this closes (#104). `toml::de::Error` owns the whole
    /// input and quotes the offending line from `Display`, so `main`'s
    /// `{e:#}` catch-all printed the operator's plaintext key to stderr.
    ///
    /// Asserting on the `{:#}` (full-chain) rendering is the whole point:
    /// `{}` showed only the outermost context and was already safe before
    /// this fix, so a test against `{}` would have passed on the bug.
    #[test]
    fn errors_never_echo_config_source() {
        let cases = [
            // Unmigrated host, both legacy fields (#103's own migration path).
            format!(
                "url = \"https://x.example/y\"\naccount_id = \
                 \"123456\"\nlicense_key = \"{SENTINEL}\"\n"
            ),
            // Partially hand-migrated: license_key alone. `account_id` sorts
            // first in toml's map, so while it is present it is the field
            // reported; remove it and the key's own line is what gets quoted.
            format!(
                "url = \"https://x.example/y\"\nlicense_key = \"{SENTINEL}\"\n"
            ),
            // Unknown non-credential field, adjacent to a secret-looking line.
            format!("url = \"https://x.example/y\"\nbogus = \"{SENTINEL}\"\n"),
            // Syntax error whose reported span lands on the secret's line.
            format!(
                "url = \"https://x.example/y\"\nlicense_key = \"{SENTINEL}\n"
            ),
            // Type mismatch: serde's own message embeds the offending value.
            format!("url = {SENTINEL}\n"),
        ];

        for source in cases {
            let text = legacy_config(&source);
            let err = parse_config(&text).expect_err("must not parse");
            let full = format!("{err:#}");
            assert!(
                !full.contains(SENTINEL),
                "config content leaked into the error chain:\n{full}"
            );
        }
    }

    /// An unmigrated host gets an actionable instruction, not the parser's
    /// symptom report. Nothing else in the codebase detects this state.
    #[test]
    fn legacy_plaintext_credentials_prompt_migration() {
        let text = legacy_config(&format!(
            "url = \"https://x.example/y\"\naccount_id = \
             \"123456\"\nlicense_key = \"{SENTINEL}\"\n"
        ));
        let err = parse_config(&text).expect_err("must not parse");
        let full = format!("{err:#}");
        assert!(
            full.contains("--set-credentials"),
            "should say what to run: {full}"
        );
        assert!(
            full.contains("account_id") && full.contains("license_key"),
            "should name both stale fields: {full}"
        );
        assert!(!full.contains(SENTINEL), "leaked the key: {full}");
    }

    /// Sanitizing must not cost diagnosability: an ordinary typo still gets
    /// the field name and a position to look at.
    #[test]
    fn sanitized_error_keeps_field_name_and_position() {
        let text = legacy_config("url = \"https://x.example/y\"\nbogus = 1\n");
        let err = parse_config(&text).expect_err("must not parse");
        let full = format!("{err:#}");
        assert!(full.contains("bogus"), "lost the field name: {full}");
        assert!(full.contains("line 8"), "lost the position: {full}");
    }

    /// A syntax error carries a span but no field name; it must still
    /// report where, and still not quote the line.
    #[test]
    fn syntax_error_reports_position_only() {
        let text = legacy_config("url = \"https://x.example/y\n");
        let err = parse_config(&text).expect_err("must not parse");
        let full = format!("{err:#}");
        assert!(full.contains("line 7"), "lost the position: {full}");
        assert!(!full.contains("x.example"), "quoted the line: {full}");
    }

    #[test]
    fn a_valid_config_still_parses() {
        let text = legacy_config(
            "url = \"https://x.example/y\"\n\n[maxmind.credentials]\nm_cost = \
             19456\nt_cost = 2\np_cost = 1\nsalt = \"aabb\"\nnonce = \
             \"ccdd\"\nciphertext = \"eeff\"\n",
        );
        let cfg = parse_config(&text).expect("valid config must parse");
        assert_eq!(cfg.paths.archive_prune, 3);
        assert!(cfg.maxmind.credentials.is_some());
    }

    /// `validate()` still runs after a successful parse.
    #[test]
    fn parse_config_still_validates() {
        let text = legacy_config("url = \"http://x.example/y\"\n");
        assert!(parse_config(&text).is_err(), "http url must be rejected");
    }

    /// serde builds type-mismatch text with `Debug`, so the offending value
    /// is embedded in `message()` itself — the one value-bearing path the
    /// span-stripping alone does not close.
    #[test]
    fn type_mismatch_value_is_redacted() {
        let text = legacy_config(&format!(
            "url = \"https://x.example/y\"\n\n[maxmind.credentials]\nm_cost = \
             \"{SENTINEL}\"\nt_cost = 2\np_cost = 1\nsalt = \"a\"\nnonce = \
             \"b\"\nciphertext = \"c\"\n"
        ));
        let err = parse_config(&text).expect_err("must not parse");
        let full = format!("{err:#}");
        assert!(!full.contains(SENTINEL), "value leaked: {full}");
        assert!(full.contains("<redacted>"), "should mark the gap: {full}");
        assert!(full.contains("expected u32"), "lost the reason: {full}");
    }

    #[test]
    fn redaction_leaves_backticked_literals_intact() {
        // The parser renders an unterminated string as ``expected `"` `` —
        // a quote character inside backticks, not the start of a value.
        assert_eq!(
            redact_quoted_values("invalid basic string, expected `\"`"),
            "invalid basic string, expected `\"`"
        );
        assert_eq!(
            redact_quoted_values("unknown field `account_id`, expected `url`"),
            "unknown field `account_id`, expected `url`"
        );
        assert_eq!(
            redact_quoted_values(
                r#"invalid type: string "secret", expected u32"#
            ),
            r#"invalid type: string "<redacted>", expected u32"#
        );
        // Escaped quotes inside the value must not end the run early.
        assert_eq!(
            redact_quoted_values(r#"got "a\"b" here"#),
            r#"got "<redacted>" here"#
        );
        // An unterminated run redacts to the end rather than emitting it.
        assert_eq!(redact_quoted_values(r#"got "abc"#), r#"got "<redacted>""#);
    }

    #[test]
    fn line_col_is_one_based_and_utf8_safe() {
        assert_eq!(line_col("abc", 0), (1, 1));
        assert_eq!(line_col("abc\ndef", 4), (2, 1));
        assert_eq!(line_col("abc\ndef", 6), (2, 3));
        // A span landing inside a multi-byte codepoint must not panic.
        assert_eq!(line_col("é\nx", 1), (1, 2));
    }

    // ── man page ↔ configuration agreement ───────────────────────────────

    const EXAMPLE_CONFIG: &str = "conf/usr/share/xt_geoip/xtgeoip.conf.example";

    /// The CONFIGURATION section of the generated man page.
    fn manpage_configuration() -> String {
        let man = std::fs::read_to_string("docs/generated/xtgeoip.1")
            .expect("docs/generated/xtgeoip.1 missing — run docgen");
        man.split(".SH CONFIGURATION")
            .nth(1)
            .and_then(|s| s.split("\n.SH ").next())
            .expect("no CONFIGURATION section in the man page")
            .to_owned()
    }

    /// Section and key names in the shipped example config.
    ///
    /// Commented-out TOML counts as shipped: `[processing]`/`threads` are
    /// commented *because they are optional*, not because they are absent,
    /// and the man page documents them. So lines whose comment body is
    /// itself TOML — `#[section]` or `#key = value` — are uncommented before
    /// parsing, and prose comments are left alone. The parse below is what
    /// guards the heuristic: if it ever swallowed a sentence, the result
    /// would stop being valid TOML and this would fail loudly rather than
    /// quietly checking less.
    fn shipped_config_keys() -> (Vec<String>, toml::Table) {
        let raw = std::fs::read_to_string(EXAMPLE_CONFIG)
            .unwrap_or_else(|e| panic!("{EXAMPLE_CONFIG}: {e}"));

        let uncommented: String = raw
            .lines()
            .map(|line| {
                let body = line.trim_start().trim_start_matches('#').trim();
                let is_toml = body.starts_with('[')
                    || body.split_once('=').is_some_and(|(k, _)| {
                        let k = k.trim();
                        !k.is_empty()
                            && k.chars()
                                .all(|c| c.is_ascii_lowercase() || c == '_')
                    });
                if line.trim_start().starts_with('#') && is_toml {
                    body.to_owned()
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let table: toml::Table = uncommented.parse().unwrap_or_else(|e| {
            panic!(
                "uncommenting {EXAMPLE_CONFIG} produced invalid TOML ({e}) — \
                 the heuristic in shipped_config_keys() has swallowed prose"
            )
        });

        let mut names = Vec::new();
        for (section, value) in &table {
            names.push(section.clone());
            if let Some(t) = value.as_table() {
                names.extend(t.keys().cloned());
            }
        }
        (names, table)
    }

    /// Every section and key that ships must be documented.
    ///
    /// The direct analogue of `cli::contradiction::global_options_are_
    /// documented`, for the configuration file rather than the flags. A key
    /// added to the shipped example and not written up here reaches users as
    /// an undocumented setting.
    #[test]
    fn manpage_documents_every_shipped_config_key() {
        let section = manpage_configuration();
        let (names, _) = shipped_config_keys();

        assert!(!names.is_empty(), "no keys parsed from {EXAMPLE_CONFIG}");

        let missing: Vec<&String> =
            names.iter().filter(|n| !section.contains(*n)).collect();
        assert!(
            missing.is_empty(),
            "{EXAMPLE_CONFIG} ships {missing:?}, which the man page's \
             CONFIGURATION section never mentions. Document them in \
             docs/spec/manpage-template.toml (configuration) and re-run \
             docgen."
        );
    }

    /// The man page must not document a key that does not exist.
    ///
    /// This is the direction that caught the real defect: on 2026-09-02 the
    /// section documented a `[maxmind] timeout` key which had never existed
    /// and which — `[maxmind]` carrying `deny_unknown_fields` — would have
    /// been rejected outright had a reader copied it into their config. The
    /// documentation was not merely stale, it was actively harmful, and
    /// nothing in the pipeline objected.
    ///
    /// Config keys appear as a lone `.I name` in this section; paths and
    /// URLs use `.IR` and carry `/` or `:`, so the snake_case filter passes
    /// over them.
    #[test]
    fn manpage_names_no_unknown_config_key() {
        let section = manpage_configuration();
        let (names, _) = shipped_config_keys();

        let documented: Vec<&str> = section
            .lines()
            .filter_map(|l| l.strip_prefix(".I "))
            .map(str::trim)
            .filter(|t| {
                !t.is_empty()
                    && t.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            })
            .collect();

        assert!(
            !documented.is_empty(),
            "no `.I key` lines found in CONFIGURATION — the roff shape this \
             scan relies on has changed, so it is checking nothing"
        );

        // Written by `xtgeoip conf -c`, never present in the shipped
        // example — the example says in as many words that credentials must
        // not be put there by hand. So it is a real key of `[maxmind]` that
        // legitimately cannot come from the file. One narrow, stated
        // exception; if this list ever grows past a couple of entries the
        // universe is being chosen to fit the test rather than the reverse.
        const WRITTEN_AT_RUNTIME: &[&str] = &["credentials"];
        assert!(WRITTEN_AT_RUNTIME.len() <= 2, "exception list is growing");

        let unknown: Vec<&&str> = documented
            .iter()
            .filter(|d| !names.iter().any(|n| n == *d))
            .filter(|d| !WRITTEN_AT_RUNTIME.contains(*d))
            .collect();
        assert!(
            unknown.is_empty(),
            "the man page documents {unknown:?}, which {EXAMPLE_CONFIG} does \
             not ship. A documented key that the parser does not accept is \
             worse than an undocumented one — a reader who copies it gets a \
             config that fails to load. Fix docs/spec/manpage-template.toml \
             (configuration)."
        );
    }

    /// Every default the man page *claims* must be the value that ships.
    ///
    /// Scoped to keys whose prose actually says "default:". The man page
    /// describes `url` and `threads` rather than defaulting them, and
    /// demanding it repeat a 78-character download URL would be a check
    /// written for the tooling's convenience rather than the reader's.
    ///
    /// This is the check that found the fourth defect in this template
    /// (2026-09-03): `[logging] log_file` named the key but stated no
    /// default, alone among the keys that have one. The first three were
    /// found by reading; this one was not.
    #[test]
    fn manpage_config_defaults_match_the_shipped_example() {
        let section = manpage_configuration();
        let (_, table) = shipped_config_keys();

        // The prose for one key: from its `.I key` line up to the next key
        // or the next tagged paragraph.
        let window_for = |key: &str| -> Option<String> {
            let start = section.find(&format!(".I {key}\n"))?;
            let rest = &section[start + 4 + key.len()..];
            let end = rest
                .find("\n.I ")
                .into_iter()
                .chain(rest.find("\n.TP"))
                .min()
                .unwrap_or(rest.len());
            Some(rest[..end].to_owned())
        };

        let mut checked = 0;
        for (name, value) in
            table.values().filter_map(toml::Value::as_table).flatten()
        {
            let shipped = match value {
                toml::Value::String(v) => v.clone(),
                toml::Value::Integer(v) => v.to_string(),
                _ => continue,
            };
            let Some(window) = window_for(name) else {
                continue;
            };
            if !window.contains("default:") {
                continue;
            }
            assert!(
                window.contains(&shipped),
                "the man page claims a default for {name} but does not give \
                 {shipped:?}, which is what {EXAMPLE_CONFIG} actually ships. \
                 A reader following the man page would configure something \
                 different from the shipped example \
                 (docs/spec/manpage-template.toml, configuration)."
            );
            checked += 1;
        }
        assert!(
            checked >= 4,
            "only {checked} documented defaults were cross-checked against \
             {EXAMPLE_CONFIG}; expected at least 4 (archive_dir, output_dir, \
             archive_prune, log_file). Fewer means the scan has stopped \
             matching the roff shape and is checking less than it appears to."
        );
    }

    /// The man page states that `[maxmind]` accepts no other key. Prove it.
    ///
    /// Asserted by parsing, not by grepping for the sentence: the claim is
    /// about the program's behaviour, so only the program can confirm it.
    /// `[paths]`, `[logging]` and `[processing]` carry no
    /// `deny_unknown_fields` and would silently accept a stray key — that
    /// asymmetry is recorded in TODO.md HOUSEKEEPING as needing a migration
    /// story, and the man page deliberately claims strictness only for the
    /// section that has it.
    #[test]
    fn unknown_maxmind_key_is_rejected_as_documented() {
        let base = "[paths]\narchive_dir = \"/a\"\narchive_prune = 3\n\
                    output_dir = \"/b\"\n\n[maxmind]\n\
                    url = \"https://example.com/x\"\n";

        assert!(
            parse_config(base).is_ok(),
            "the baseline config should parse"
        );
        assert!(
            parse_config(&format!("{base}timeout = 30\n")).is_err(),
            "the man page states that no other key is accepted in [maxmind], \
             but an unknown key parsed successfully — either \
             deny_unknown_fields was removed or the claim is now false"
        );
    }
}
