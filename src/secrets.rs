/// xtgeoip © Haze N Sparkle 2026 (MIT)
/// MaxMind credential encryption/decryption (#103): Argon2id key
/// derivation, XChaCha20-Poly1305 AEAD, `rpassword` prompts,
/// `secrecy`/`zeroize`/`mlock` memory hygiene. See
/// docs/design/103-encrypted-credentials.md for the full rationale behind
/// every choice below.
///
/// This is the only module that ever holds MaxMind credentials in
/// plaintext form. Everywhere else sees either ciphertext
/// (`config::Credentials`) or a `DecryptedCredentials` whose contents are
/// reachable only through `.account_id()`/`.license_key()`, each a thin
/// wrapper over `secrecy::ExposeSecret`. Deliberately not called from
/// `fetch.rs` itself — see `action.rs`'s dispatch-time call site, which
/// keeps `fetch.rs`'s mock-HTTP unit test suite running under plain `cargo
/// test`, with no terminal and no passphrase involved.
use std::io::{self, IsTerminal};

use anyhow::{Context, Result, bail};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, KeyInit},
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::config::Credentials;

const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;

/// Decrypted MaxMind credentials, held for the duration of one `fetch`
/// invocation. `secrecy::SecretString` refuses `Debug`/`Display`/`Clone`/
/// `Serialize` on the wrapped value — a guardian audit of a change to this
/// file need only check that every `.expose_secret()` call (reached via the
/// two methods below) feeds directly into `fetch.rs`'s MaxMind `basic_auth`
/// call and nowhere else (no `log::debug!`, no `{:?}`, no re-serialization).
pub struct DecryptedCredentials {
    account_id: SecretString,
    license_key: SecretString,
}

impl DecryptedCredentials {
    pub fn account_id(&self) -> &str {
        self.account_id.expose_secret()
    }

    pub fn license_key(&self) -> &str {
        self.license_key.expose_secret()
    }
}

/// The plaintext this design encrypts as one JSON blob under one nonce
/// (docs/design/103-encrypted-credentials.md §3b) — both fields live and
/// die together, keeping "one nonce, used once, per key" literally true
/// rather than merely typical.
#[derive(Serialize, Deserialize)]
struct Blob {
    account_id: String,
    license_key: String,
}

/// Best-effort: ask the kernel not to swap out `len` bytes starting at
/// `ptr`. Failure (e.g. a container's `RLIMIT_MEMLOCK` too low to cover even
/// this) is logged, not fatal — this narrows the swap-exposure window (§5);
/// it is not the primary defence (§4's actual boundary is the AEAD
/// ciphertext at rest, which `mlock` has no bearing on).
fn mlock(ptr: *const u8, len: usize) {
    if len == 0 {
        return;
    }
    // SAFETY: `ptr` is valid for `len` bytes for the duration of this call —
    // every caller passes a pointer/length pair taken directly from a live,
    // owned buffer it still holds.
    let rc = unsafe { libc::mlock(ptr.cast(), len) };
    if rc != 0 {
        crate::messages::warn(&format!(
            "mlock failed ({}) — a secret buffer may be swappable",
            io::Error::last_os_error()
        ));
    }
}

/// Pairs with [`mlock`]. Not called for `DecryptedCredentials`'s fields —
/// see the comment at its construction site in [`lock_and_wrap`]. Callers
/// must capture `len` *before* zeroizing a `String`/`Vec` — `Zeroize`
/// clears them to length 0, so `munlock(p.as_ptr(), p.len())` called after
/// `p.zeroize()` would always be a no-op.
fn munlock(ptr: *const u8, len: usize) {
    if len == 0 {
        return;
    }
    // SAFETY: see `mlock` above; the region being unlocked here was locked
    // by a prior, matching `mlock` call on the same still-live buffer.
    unsafe {
        libc::munlock(ptr.cast(), len);
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        bail!("hex string has odd length ({})", s.len());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .with_context(|| format!("invalid hex byte {:?}", &s[i..i + 2]))
        })
        .collect()
}

/// Build the Argon2id instance this design has committed to (§3), from cost
/// parameters supplied by the caller — stored alongside the ciphertext
/// (§3b) on decrypt, current OWASP-recommended defaults on encrypt — so a
/// future bump in recommended parameters never breaks a config encrypted
/// under today's numbers.
fn argon2_for(
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<Argon2<'static>> {
    let params = Params::new(m_cost, t_cost, p_cost, Some(KEY_LEN))
        .map_err(|e| anyhow::anyhow!("invalid Argon2 parameters: {e}"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Derive the XChaCha20-Poly1305 key from `passphrase` and `salt`. The
/// derived key is mlocked while it exists and zeroized/munlocked
/// immediately after being copied into the cipher's own internal state —
/// regardless of whether derivation or cipher construction succeeded, so no
/// early-return path leaves it lying around. `key` is a fixed-size array,
/// so unlike the `String`/`Vec` buffers elsewhere in this file, `.len()`
/// after `.zeroize()` is still `KEY_LEN` — the munlock call below is not
/// subject to the zeroize-then-munlock ordering trap those need.
fn derive_cipher(
    argon2: &Argon2<'_>,
    passphrase: &[u8],
    salt: &[u8],
) -> Result<XChaCha20Poly1305> {
    let mut key = [0u8; KEY_LEN];
    mlock(key.as_ptr(), key.len());
    let result = argon2
        .hash_password_into(passphrase, salt, &mut key)
        .map_err(|e| anyhow::anyhow!("key derivation failed: {e}"))
        .and_then(|()| {
            XChaCha20Poly1305::new_from_slice(&key).map_err(|_| {
                anyhow::anyhow!("derived key has the wrong length")
            })
        });
    key.zeroize();
    munlock(key.as_ptr(), key.len());
    result
}

/// mlock the heap allocation backing `s`, then hand it to `SecretBox`.
/// `SecretBox::new` stores the `Box` unchanged (no reallocation), so the
/// pointer locked here is exactly the one `secrecy` holds for the rest of
/// this value's life.
///
/// Not munlocked on drop: `secrecy`'s `Drop` impl gives no hook to run code
/// between zeroizing and deallocating, and `xtgeoip` is a short-lived CLI
/// process (`fetch`, `conf --set-credentials`) — the OS reclaims the lock
/// at process exit regardless. A long-running daemon reusing this code
/// would need an explicit munlock-on-drop wrapper; this tool is not one.
fn lock_and_wrap(s: String) -> SecretString {
    let boxed = s.into_boxed_str();
    mlock(boxed.as_ptr(), boxed.len());
    SecretString::from(boxed)
}

/// Core of [`decrypt`], parameterized on an already-known passphrase rather
/// than prompting for one — this is what makes the write→read round trip
/// (`conf --set-credentials` splices ciphertext in, `fetch` must be able to
/// read it back out) testable under plain `cargo test`, with no terminal
/// involved.
fn decrypt_with_passphrase(
    creds: &Credentials,
    passphrase: &str,
) -> Result<DecryptedCredentials> {
    let salt =
        hex_decode(&creds.salt).context("credentials.salt is not valid hex")?;
    let nonce_bytes = hex_decode(&creds.nonce)
        .context("credentials.nonce is not valid hex")?;
    let ciphertext = hex_decode(&creds.ciphertext)
        .context("credentials.ciphertext is not valid hex")?;

    let argon2 = argon2_for(creds.m_cost, creds.t_cost, creds.p_cost)?;
    let cipher = derive_cipher(&argon2, passphrase.as_bytes(), &salt)?;

    let nonce = XNonce::try_from(nonce_bytes.as_slice()).map_err(|_| {
        anyhow::anyhow!("credentials.nonce has the wrong length")
    })?;

    let mut plaintext =
        cipher.decrypt(&nonce, ciphertext.as_slice()).map_err(|_| {
            anyhow::anyhow!(
                "failed to decrypt MaxMind credentials — wrong passphrase, or \
                 the config is corrupt"
            )
        })?;
    let len = plaintext.len();
    mlock(plaintext.as_ptr(), len);
    let parsed: Result<Blob, _> = serde_json::from_slice(&plaintext);
    plaintext.zeroize();
    munlock(plaintext.as_ptr(), len);
    let mut blob =
        parsed.context("decrypted credentials are not valid JSON")?;

    // `mem::take` moves each plaintext `String` directly into `SecretString`
    // rather than cloning it, so no extra unprotected copy is ever made.
    let account_id = lock_and_wrap(std::mem::take(&mut blob.account_id));
    let license_key = lock_and_wrap(std::mem::take(&mut blob.license_key));

    Ok(DecryptedCredentials {
        account_id,
        license_key,
    })
}

/// Decrypt `creds` into usable MaxMind credentials, prompting interactively
/// for the passphrase.
pub fn decrypt(creds: &Credentials) -> Result<DecryptedCredentials> {
    if !io::stdin().is_terminal() {
        bail!(
            "Decrypting MaxMind credentials requires an interactive \
             passphrase prompt, but stdin is not a terminal. This tool cannot \
             fetch unattended — see docs/design/103-encrypted-credentials.md \
             §6."
        );
    }

    let mut passphrase =
        rpassword::prompt_password("MaxMind credentials passphrase: ")
            .context("failed to read passphrase")?;
    let len = passphrase.len();
    mlock(passphrase.as_ptr(), len);
    let result = decrypt_with_passphrase(creds, &passphrase);
    passphrase.zeroize();
    munlock(passphrase.as_ptr(), len);
    result
}

/// Double-entry passphrase prompt (§9a) — the one field in this scheme with
/// no independent record, so a masked typo would otherwise be invisible
/// until the next `fetch` fails to decrypt for no apparent reason.
fn prompt_confirmed_passphrase() -> Result<String> {
    if !io::stdin().is_terminal() {
        bail!(
            "Setting MaxMind credentials requires interactive prompts, but \
             stdin is not a terminal."
        );
    }
    loop {
        let first = rpassword::prompt_password("Encryption passphrase: ")
            .context("failed to read passphrase")?;
        let second = rpassword::prompt_password("Confirm passphrase: ")
            .context("failed to read passphrase")?;
        if first == second {
            return Ok(first);
        }
        println!("Passphrases did not match — try again.");
    }
}

/// Core of [`encrypt`], parameterized on an already-known passphrase rather
/// than prompting for one — see [`decrypt_with_passphrase`] for why this
/// split exists.
fn encrypt_with_passphrase(
    passphrase: &str,
    account_id: &str,
    license_key: &str,
) -> Result<Credentials> {
    let salt: [u8; SALT_LEN] = Generate::generate();
    let m_cost = Params::DEFAULT_M_COST;
    let t_cost = Params::DEFAULT_T_COST;
    let p_cost = Params::DEFAULT_P_COST;
    let argon2 = argon2_for(m_cost, t_cost, p_cost)?;
    let cipher = derive_cipher(&argon2, passphrase.as_bytes(), &salt)?;

    let nonce = XNonce::generate();

    let blob = Blob {
        account_id: account_id.to_string(),
        license_key: license_key.to_string(),
    };
    let mut plaintext =
        serde_json::to_vec(&blob).context("failed to serialize credentials")?;
    let len = plaintext.len();
    mlock(plaintext.as_ptr(), len);
    let ciphertext_result = cipher
        .encrypt(&nonce, plaintext.as_slice())
        .map_err(|_| anyhow::anyhow!("encryption failed"));
    plaintext.zeroize();
    munlock(plaintext.as_ptr(), len);
    let ciphertext = ciphertext_result?;

    Ok(Credentials {
        m_cost,
        t_cost,
        p_cost,
        salt: hex_encode(&salt),
        nonce: hex_encode(nonce.as_slice()),
        ciphertext: hex_encode(&ciphertext),
    })
}

/// Encrypt `account_id`/`license_key` under a freshly-derived key. Returns
/// the plain-data struct `conf.rs` splices into `/etc/xtgeoip.conf`.
pub fn encrypt(account_id: &str, license_key: &str) -> Result<Credentials> {
    let mut passphrase = prompt_confirmed_passphrase()?;
    let len = passphrase.len();
    mlock(passphrase.as_ptr(), len);
    let result = encrypt_with_passphrase(&passphrase, account_id, license_key);
    passphrase.zeroize();
    munlock(passphrase.as_ptr(), len);
    result
}

#[cfg(test)]
mod tests;
