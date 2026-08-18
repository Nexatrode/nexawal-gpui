//! OS secure-store seed + local restore-height marker.
//!
//! The mnemonic is never written to the data directory. It is stored in
//! macOS Keychain, Windows Credential Manager, or Linux Secret Service.

use crate::paths;

const SERVICE: &str = "com.nexatrode.nexawal";
const ACCOUNT: &str = "wallet.mnemonic";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|err| err.to_string())
}

pub fn is_marked_stored() -> bool {
    paths::wallet_slot_path().exists()
}

pub const fn secure_store_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS Keychain"
    } else if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else if cfg!(target_os = "linux") {
        "Linux Secret Service"
    } else {
        "OS secure storage"
    }
}

pub fn save(mnemonic: &str, restore_height: u64) -> Result<(), String> {
    let phrase = mnemonic.split_whitespace().collect::<Vec<_>>().join(" ");
    if phrase.is_empty() {
        return Err("empty mnemonic".into());
    }
    entry()?
        .set_password(&phrase)
        .map_err(|err| format!("{} save failed: {err}", secure_store_name()))?;
    paths::mark_wallet_stored(restore_height).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn load() -> Result<(String, u64), String> {
    if !is_marked_stored() {
        return Err("no stored wallet".into());
    }
    let mnemonic = entry()?
        .get_password()
        .map_err(|err| load_error(&err))?;
    let phrase = mnemonic.split_whitespace().collect::<Vec<_>>().join(" ");
    if phrase.is_empty() {
        return Err("stored mnemonic was empty".into());
    }
    Ok((phrase, paths::load_restore_height()))
}

pub fn delete() -> Result<(), String> {
    let entry = entry()?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(err) => {
            return Err(format!(
                "Could not remove the wallet from {}: {err}",
                secure_store_name()
            ));
        }
    }
    paths::clear_wallet_slot();
    Ok(())
}

fn load_error(err: &keyring::Error) -> String {
    if matches!(err, keyring::Error::NoEntry) {
        format!(
            "No saved wallet was found in {}. Enter the seed once to save it persistently",
            secure_store_name()
        )
    } else {
        format!("{} unlock failed: {err}", secure_store_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_entry_explains_how_to_migrate() {
        let message = load_error(&keyring::Error::NoEntry);
        assert!(message.contains(secure_store_name()));
        assert!(message.contains("Enter the seed once"));
    }
}
