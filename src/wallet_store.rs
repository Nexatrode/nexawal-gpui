//! OS keychain seed + local restore-height marker. Mnemonic is never written to the data dir.

use crate::paths;

const SERVICE: &str = "com.nexatrode.nexawal";
const ACCOUNT: &str = "wallet.mnemonic";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|err| err.to_string())
}

pub fn is_marked_stored() -> bool {
    paths::wallet_slot_path().exists()
}

pub fn save(mnemonic: &str, restore_height: u64) -> Result<(), String> {
    let phrase = mnemonic.split_whitespace().collect::<Vec<_>>().join(" ");
    if phrase.is_empty() {
        return Err("empty mnemonic".into());
    }
    entry()?
        .set_password(&phrase)
        .map_err(|err| format!("Keychain save failed: {err}"))?;
    paths::mark_wallet_stored(restore_height).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn load() -> Result<(String, u64), String> {
    if !is_marked_stored() {
        return Err("no stored wallet".into());
    }
    let mnemonic = entry()?
        .get_password()
        .map_err(|err| format!("Keychain unlock failed: {err}"))?;
    let phrase = mnemonic.split_whitespace().collect::<Vec<_>>().join(" ");
    if phrase.is_empty() {
        return Err("stored mnemonic was empty".into());
    }
    Ok((phrase, paths::load_restore_height()))
}

pub fn delete() {
    if let Ok(entry) = entry() {
        let _ = entry.delete_credential();
    }
    paths::clear_wallet_slot();
}
