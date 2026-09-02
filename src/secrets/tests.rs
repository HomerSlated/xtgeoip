//! Unit tests for [`crate::secrets`].
//!
//! Kept in a child module file rather than inside the parent so that
//! `src/secrets.rs` — which carries a guardian signature — does not change
//! when a test does. A child module sees its parent's private items, so
//! this needs no visibility widening: the parent's public API is
//! unchanged by the move.

use super::*;

#[test]
fn round_trip_recovers_original_credentials() {
    let creds = encrypt_with_passphrase(
        "correct horse battery staple",
        "123456",
        "abcdef0123456789",
    )
    .unwrap();
    let decrypted =
        decrypt_with_passphrase(&creds, "correct horse battery staple")
            .unwrap();
    assert_eq!(decrypted.account_id(), "123456");
    assert_eq!(decrypted.license_key(), "abcdef0123456789");
}

#[test]
fn wrong_passphrase_fails_to_decrypt() {
    // `.err()` rather than `.unwrap_err()`: `DecryptedCredentials`
    // deliberately does not derive `Debug` (§5), which `unwrap_err`
    // would require.
    let creds =
        encrypt_with_passphrase("right passphrase", "123456", "somekey")
            .unwrap();
    let err = decrypt_with_passphrase(&creds, "wrong passphrase")
        .err()
        .expect("wrong passphrase must fail to decrypt");
    assert!(err.to_string().contains("wrong passphrase"));
}

#[test]
fn tampered_ciphertext_fails_to_decrypt() {
    let mut creds =
        encrypt_with_passphrase("a passphrase", "123456", "somekey").unwrap();
    // Flip one hex nibble in the ciphertext — must be caught by the
    // AEAD tag, not silently accepted as different plaintext.
    let mut bytes = creds.ciphertext.into_bytes();
    let last = bytes.len() - 1;
    bytes[last] = if bytes[last] == b'0' { b'1' } else { b'0' };
    creds.ciphertext = String::from_utf8(bytes).unwrap();

    assert!(decrypt_with_passphrase(&creds, "a passphrase").is_err());
}

#[test]
fn two_encryptions_use_different_salt_and_nonce() {
    // Each rotation must derive a fresh key under a fresh random salt —
    // this is the invariant §3a's nonce-reuse argument depends on.
    let a = encrypt_with_passphrase("pw", "1", "a").unwrap();
    let b = encrypt_with_passphrase("pw", "1", "a").unwrap();
    assert_ne!(a.salt, b.salt);
    assert_ne!(a.nonce, b.nonce);
}

#[test]
fn hex_round_trips() {
    let bytes = [0u8, 1, 15, 16, 255, 128];
    assert_eq!(hex_decode(&hex_encode(&bytes)).unwrap(), bytes);
}

#[test]
fn hex_decode_rejects_odd_length() {
    assert!(hex_decode("abc").is_err());
}

#[test]
fn hex_decode_rejects_non_hex() {
    assert!(hex_decode("zz").is_err());
}

/// The seam advisor flagged: `conf --set-credentials` writes ciphertext
/// into the TOML document via `toml_edit`'s field assignment; `fetch`
/// reads it back via plain `toml::from_str::<Config>` (a different
/// parser, though the same underlying project). Nothing before this
/// test actually exercised both directions together — every other test
/// either builds a `Credentials` struct directly in memory, or checks
/// crypto in isolation. This proves the two halves actually agree on
/// field names/types/table nesting.
#[test]
fn splice_then_parse_then_decrypt_round_trips() {
    let creds =
        encrypt_with_passphrase("pw", "myaccount", "mylicensekey").unwrap();

    // Includes a hand-written comment and an unrelated `[logging]`
    // table — the entire reason §9a chose `toml_edit` over a
    // `serde`/`toml` round-trip was to leave both untouched. Asserted
    // below, not just assumed.
    let source = "# a hand-written comment the operator added\n\
                   [maxmind]\n\
                   url = \"https://example.com/download\"\n\
                   \n\
                   [paths]\n\
                   archive_dir = \"/var/lib/xt_geoip\"\n\
                   archive_prune = 3\n\
                   output_dir = \"/usr/share/xt_geoip\"\n\
                   \n\
                   [logging]\n\
                   log_file = \"/var/log/xtgeoip.log\"\n";
    let spliced = crate::conf::splice_credentials(source, &creds)
        .expect("splice must succeed");
    assert!(
        spliced.contains("# a hand-written comment the operator added"),
        "toml_edit must preserve comments, not just data: {spliced}"
    );
    assert!(
        spliced.contains("[logging]") && spliced.contains("log_file"),
        "toml_edit must preserve unrelated sections untouched: {spliced}"
    );

    let cfg: crate::config::Config = toml::from_str(&spliced)
        .expect("toml_edit's output must parse back via plain toml");
    let parsed_creds = cfg
        .maxmind
        .credentials
        .expect("spliced credentials must round-trip through parsing");

    let decrypted = decrypt_with_passphrase(&parsed_creds, "pw")
        .expect("must decrypt with the original passphrase");
    assert_eq!(decrypted.account_id(), "myaccount");
    assert_eq!(decrypted.license_key(), "mylicensekey");
}

/// A real bug caught by the user running `conf -c` against a pre-#103
/// config: `splice_credentials` only *added* `[maxmind.credentials]`
/// and left the old plaintext `account_id`/`license_key` sitting right
/// next to it — exactly the leak this feature exists to close. This
/// proves the migration path actually removes them, not just that a
/// fresh config splices cleanly.
#[test]
fn splice_removes_legacy_plaintext_fields() {
    let creds = encrypt_with_passphrase("pw", "newaccount", "newkey").unwrap();

    let legacy_source = "[maxmind]\n\
                          account_id = \"old-plaintext-account\"\n\
                          license_key = \"old-plaintext-key\"\n\
                          url = \"https://example.com/download\"\n\
                          \n\
                          [paths]\n\
                          archive_dir = \"/var/lib/xt_geoip\"\n\
                          archive_prune = 3\n\
                          output_dir = \"/usr/share/xt_geoip\"\n";

    let spliced =
        crate::conf::splice_credentials(legacy_source, &creds).unwrap();

    assert!(
        !spliced.contains("old-plaintext-account")
            && !spliced.contains("old-plaintext-key"),
        "legacy plaintext credentials must be removed, not left alongside the \
         new ciphertext: {spliced}"
    );

    // Must also still parse cleanly — `MaxMind` now denies unknown
    // fields, so a leftover `account_id`/`license_key` would fail to
    // load at all, not just look untidy.
    let cfg: crate::config::Config = toml::from_str(&spliced)
        .expect("must parse with no leftover plaintext fields");
    let decrypted = decrypt_with_passphrase(
        &cfg.maxmind
            .credentials
            .expect("credentials must be present"),
        "pw",
    )
    .unwrap();
    assert_eq!(decrypted.account_id(), "newaccount");
    assert_eq!(decrypted.license_key(), "newkey");
}
