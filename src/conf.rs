/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// xtgeoip `conf` subcommand: configuration management actions (show /
/// edit / default), preconditions, and interactive creation. Depends on
/// `config` only for the shared system-config path.
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::Path,
    process::Command,
};

use anyhow::{Context, Result, bail};
use toml_edit::DocumentMut;
use zeroize::Zeroize;

use crate::{
    config::{Credentials, sanitize_toml_error, system_config_path},
    secrets,
};

const DEFAULT_CONFIG: &str = "/usr/share/xt_geoip/xtgeoip.conf.example";

#[derive(Debug, PartialEq, Eq)]
pub enum ConfAction {
    Show,
    Edit,
    Default,
    SetCredentials,
}

impl ConfAction {
    /// Check that the invariants this action requires hold before running.
    pub fn check_preconditions(&self) -> Result<()> {
        match self {
            ConfAction::Default => ensure_default_config_exists(),
            ConfAction::Show => ensure_system_config_exists(),
            ConfAction::Edit => require_existing_system_config("edit"),
            ConfAction::SetCredentials => {
                require_existing_system_config("set credentials")
            }
        }
    }
}

/// Shared by `Edit` and `SetCredentials`: both need a real, already-existing
/// config file to operate on (offering to create one from the default
/// example first, same as every other action).
fn require_existing_system_config(verb: &str) -> Result<()> {
    ensure_system_config_exists()?;
    if !system_config_path().exists() {
        let cfg_path = system_config_path().display();
        bail!(
            "Cannot {verb}: {cfg_path} does not exist. Run `xtgeoip conf -d` \
             to view the default config, then create {cfg_path} manually."
        );
    }
    Ok(())
}

fn config_exists() -> bool {
    system_config_path().exists()
}

/// Verify the packaged default-config example exists. Both `conf -d` (which
/// reads it) and `create_default_config` (which copies it) require it; a bare
/// IO error on the missing file is unactionable, so point at a reinstall.
fn ensure_default_config_exists() -> Result<()> {
    if !Path::new(DEFAULT_CONFIG).exists() {
        bail!(
            "Default config example not found at {DEFAULT_CONFIG}. You may \
             need to reinstall xtgeoip."
        );
    }
    Ok(())
}

fn create_default_config() -> Result<()> {
    ensure_default_config_exists()?;
    fs::copy(DEFAULT_CONFIG, system_config_path()).with_context(|| {
        format!(
            "Failed to copy {DEFAULT_CONFIG} to {}",
            system_config_path().display()
        )
    })?;
    println!(
        "Created {} from default example.",
        system_config_path().display()
    );
    Ok(())
}

/// Returns `true` if the user confirmed creation, `false` if they declined.
fn prompt_create_config() -> Result<bool> {
    if !io::stdin().is_terminal() {
        let cfg_path = system_config_path().display();
        bail!(
            "{cfg_path} does not exist and stdin is not a terminal. Run \
             `xtgeoip conf -d` to view the default config, then create \
             {cfg_path} manually."
        );
    }

    println!(
        "Configuration file not found at {}.",
        system_config_path().display()
    );
    print!("Do you want to create it from the default example? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_lowercase();

    if answer == "y" || answer == "yes" {
        Ok(true)
    } else {
        println!(
            "Skipping creation of system config. You can edit it manually \
             later."
        );
        Ok(false)
    }
}

fn ensure_system_config_exists() -> Result<()> {
    if config_exists() {
        return Ok(());
    }
    if prompt_create_config()? {
        create_default_config()?;
    }
    Ok(())
}

/// Perform the requested action for `xtgeoip conf`
pub fn run_conf(action: ConfAction) -> Result<()> {
    action.check_preconditions()?;
    match action {
        ConfAction::Default => {
            let contents = fs::read_to_string(DEFAULT_CONFIG)
                .with_context(|| format!("Failed to read {DEFAULT_CONFIG}"))?;
            println!("{contents}");
        }
        ConfAction::Show => {
            if system_config_path().exists() {
                let contents = fs::read_to_string(system_config_path())?;
                println!("{contents}");
            } else {
                println!("No system config exists to show.");
            }
        }
        ConfAction::Edit => {
            let editor = std::env::var("EDITOR")
                .ok()
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| "vi".to_string());
            let status = Command::new(&editor)
                .arg(system_config_path())
                .status()
                .with_context(|| {
                    format!("Failed to launch editor '{editor}'")
                })?;
            if !status.success() {
                bail!("Editor '{editor}' exited with {status}");
            }
        }
        ConfAction::SetCredentials => set_credentials()?,
    }
    Ok(())
}

/// Read a plainly-echoed line of input (used for `account_id`, which is not
/// secret — MaxMind's dashboard is an independent source of truth for it,
/// so a visible-as-typed entry is fine; see docs/design/103…md §9a).
fn read_line_trimmed(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// `true` if the operator confirmed overwriting existing credentials.
fn confirm_overwrite_credentials() -> Result<bool> {
    let cfg_path = system_config_path().display();
    print!(
        "MaxMind credentials are already set in {cfg_path}. Overwrite? [y/N] "
    );
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Confirm `SYSTEM_CONFIG`'s directory is writable by trying to create (and
/// immediately drop) a temp file in it. Used to fail fast, before any
/// interactive prompt, rather than let an unprivileged operator answer every
/// prompt — including typing their real MaxMind license_key — only to hit
/// EACCES at the very last step.
fn check_system_config_writable() -> Result<()> {
    let dir = system_config_path().parent().ok_or_else(|| {
        anyhow::anyhow!(
            "{} has no parent directory",
            system_config_path().display()
        )
    })?;
    tempfile::NamedTempFile::new_in(dir).with_context(|| {
        format!(
            "Cannot write to {}. Re-run as root (e.g. with sudo).",
            dir.display()
        )
    })?;
    Ok(())
}

/// Write `contents` to `SYSTEM_CONFIG` atomically: a temp file in the same
/// directory, then an atomic rename, so a process killed mid-write cannot
/// leave the one file this whole scheme depends on half-written.
fn write_system_config_atomically(contents: &str) -> Result<()> {
    let dir = system_config_path().parent().ok_or_else(|| {
        anyhow::anyhow!(
            "{} has no parent directory",
            system_config_path().display()
        )
    })?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).with_context(|| {
        format!("Failed to create a temp file in {}", dir.display())
    })?;
    tmp.write_all(contents.as_bytes())
        .context("Failed to write new config contents")?;
    tmp.persist(system_config_path()).with_context(|| {
        format!(
            "Failed to atomically replace {}",
            system_config_path().display()
        )
    })?;
    Ok(())
}

/// Parse config text with `toml_edit`, reporting failures through
/// [`sanitize_toml_error`].
///
/// `toml_edit::TomlError` quotes the offending source line from its
/// `Display` exactly as `toml::de::Error` does — the same #104 leak, in the
/// one module that is *only* ever reached while handling credentials. Both
/// call sites here read `/etc/xtgeoip.conf`, so a stray quote on a
/// hand-edited `license_key` line would otherwise print the key through
/// `main`'s `{e:#}`.
fn parse_document(source: &str) -> Result<DocumentMut> {
    source.parse::<DocumentMut>().map_err(|e| {
        anyhow::anyhow!(
            "{}",
            sanitize_toml_error(source, e.message(), e.span())
        )
    })
}

/// Splice `creds` into `source` (the raw text of a config file) at
/// `[maxmind.credentials]`, preserving every comment and every other
/// section exactly as written — a full `serde`/`toml` round-trip of the
/// whole file would silently reformat all of it for a change that only
/// touches one table. Pure and side-effect-free (no filesystem, no
/// prompts) precisely so it can be tested directly: see
/// `secrets::tests::splice_then_parse_then_decrypt_round_trips`, which is
/// the only place in this codebase that proves `toml_edit`'s write side and
/// plain `toml`'s read side (`Config`'s `Deserialize`) actually agree on
/// field names, types, and table nesting.
pub(crate) fn splice_credentials(
    source: &str,
    creds: &Credentials,
) -> Result<String> {
    let mut doc = parse_document(source)?;

    if doc.get("maxmind").is_none() {
        doc["maxmind"] = toml_edit::table();
    }

    // Migration: a config written before #103 stores `account_id`/
    // `license_key` as plaintext siblings of `url` under `[maxmind]`.
    // Leaving them in place after encrypting would defeat the entire point
    // of this feature — the operator's real credentials would still be
    // sitting in cleartext right next to the ciphertext that was supposed
    // to replace them.
    if let Some(maxmind) = doc["maxmind"].as_table_mut() {
        maxmind.remove("account_id");
        maxmind.remove("license_key");
    }

    doc["maxmind"]["credentials"] = toml_edit::table();
    doc["maxmind"]["credentials"]["m_cost"] =
        toml_edit::value(i64::from(creds.m_cost));
    doc["maxmind"]["credentials"]["t_cost"] =
        toml_edit::value(i64::from(creds.t_cost));
    doc["maxmind"]["credentials"]["p_cost"] =
        toml_edit::value(i64::from(creds.p_cost));
    doc["maxmind"]["credentials"]["salt"] =
        toml_edit::value(creds.salt.clone());
    doc["maxmind"]["credentials"]["nonce"] =
        toml_edit::value(creds.nonce.clone());
    doc["maxmind"]["credentials"]["ciphertext"] =
        toml_edit::value(creds.ciphertext.clone());

    Ok(doc.to_string())
}

/// `xtgeoip conf --set-credentials`: prompt for MaxMind `account_id` /
/// `license_key` and an encryption passphrase, encrypt them (#103), and
/// splice the result into `/etc/xtgeoip.conf` via [`splice_credentials`],
/// then write it back atomically.
fn set_credentials() -> Result<()> {
    if !io::stdin().is_terminal() {
        bail!(
            "Setting MaxMind credentials requires interactive prompts \
             (account_id, license_key, and an encryption passphrase), but \
             stdin is not a terminal."
        );
    }
    // SYSTEM_CONFIG itself is world-readable, so the read below succeeds
    // even unprivileged — but writing the result back requires root. Check
    // that now, before prompting for anything, so an unprivileged operator
    // doesn't type their real license_key and wait on the KDF only to hit
    // EACCES at the last step.
    check_system_config_writable()?;

    let raw = fs::read_to_string(system_config_path()).with_context(|| {
        format!("Failed to read {}", system_config_path().display())
    })?;

    let has_existing = parse_document(&raw)?
        .get("maxmind")
        .and_then(|m| m.get("credentials"))
        .is_some();
    if has_existing && !confirm_overwrite_credentials()? {
        println!("Leaving existing MaxMind credentials unchanged.");
        return Ok(());
    }

    let account_id = read_line_trimmed("MaxMind account_id: ")?;
    if account_id.is_empty() {
        bail!("account_id must not be empty");
    }
    let mut license_key = rpassword::prompt_password("MaxMind license_key: ")
        .context("Failed to read license_key")?;
    let mut trimmed = license_key.trim().to_string();
    license_key.zeroize();
    if trimmed.is_empty() {
        bail!("license_key must not be empty");
    }

    let creds_result = secrets::encrypt(&account_id, &trimmed);
    trimmed.zeroize();
    let creds = creds_result?;

    let spliced = splice_credentials(&raw, &creds)?;
    write_system_config_atomically(&spliced)?;
    println!(
        "MaxMind credentials encrypted and saved to {}.",
        system_config_path().display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL: &str = "LIVE_PLAINTEXT_KEY_DO_NOT_LEAK";

    /// #104, second instance. `toml_edit::TomlError` quotes the offending
    /// line just as `toml::de::Error` does, and both `conf.rs` call sites
    /// read `/etc/xtgeoip.conf` — so a hand-edited config with a stray
    /// quote on the `license_key` line printed the key through `main`'s
    /// `{e:#}`. Asserting on `{:#}`, not `{}`, for the same reason as
    /// `config::tests::errors_never_echo_config_source`.
    #[test]
    fn parse_errors_never_echo_config_source() {
        let cases = [
            // Unterminated string: the span lands on the secret's own line.
            format!(
                "[maxmind]\nurl = \"https://x/y\"\nlicense_key = \
                 \"{SENTINEL}\n"
            ),
            // Unterminated string on the line above the secret.
            format!(
                "[maxmind]\nurl = \"https://x/y\nlicense_key = \
                 \"{SENTINEL}\"\n"
            ),
        ];
        for source in cases {
            let err = parse_document(&source).expect_err("must not parse");
            let full = format!("{err:#}");
            assert!(
                !full.contains(SENTINEL),
                "config content leaked into the error chain:\n{full}"
            );
            assert!(full.contains("line"), "lost the position: {full}");
        }
    }

    /// `splice_credentials` shares the parse, so it is covered too.
    #[test]
    fn splice_reports_bad_toml_without_quoting_it() {
        let creds = Credentials {
            m_cost: 19456,
            t_cost: 2,
            p_cost: 1,
            salt: "aa".into(),
            nonce: "bb".into(),
            ciphertext: "cc".into(),
        };
        let source = format!("[maxmind]\nlicense_key = \"{SENTINEL}\n");
        let err =
            splice_credentials(&source, &creds).expect_err("must not parse");
        assert!(!format!("{err:#}").contains(SENTINEL));
    }
}
