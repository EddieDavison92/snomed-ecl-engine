//! The TRUD API key saved by `login`, kept in the operating system's credential
//! store: Windows Credential Manager, macOS Keychain or the Secret Service.
//! `TRUD_API_KEY` takes precedence, so CI and containers need no store.
use anyhow::{anyhow, Result};

const SERVICE: &str = "snomed-ecl-engine";
const ACCOUNT: &str = "trud-api-key";

/// The platform's credential store, for messages.
pub fn store_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else if cfg!(target_os = "macos") {
        "the macOS Keychain"
    } else {
        "the Secret Service keyring"
    }
}

fn entry() -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|error| {
        let detail = match error {
            // No store started, as in a container or a server without a desktop.
            keyring::Error::NoDefaultStore => String::new(),
            error => format!(" ({error})"),
        };
        anyhow!(
            "{} is not available{detail}. Set {} instead",
            store_name(),
            crate::download::ENV_KEY
        )
    })
}

/// The saved key. A missing entry or an unavailable store reads as no key.
pub fn saved() -> Option<String> {
    let key = entry().ok()?.get_password().ok()?.trim().to_owned();
    (!key.is_empty()).then_some(key)
}

pub fn save(key: &str) -> Result<()> {
    entry()?
        .set_password(key)
        .map_err(|error| anyhow!("Cannot save the key in {}: {error}", store_name()))
}

/// Deletes the saved key, reporting whether there was one.
pub fn forget() -> Result<bool> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(anyhow!(
            "Cannot remove the key from {}: {error}",
            store_name()
        )),
    }
}
