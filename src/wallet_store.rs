//! OS secure-store seed + local restore-height marker.
//!
//! The mnemonic is never written to the data directory. It is stored in
//! macOS Keychain, Windows Credential Manager, or Linux Secret Service.

use crate::paths;
use zeroize::Zeroizing;

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

pub fn save(mnemonic: &str, restore_height: u64, protected: bool) -> Result<(), String> {
    let phrase = Zeroizing::new(mnemonic.split_whitespace().collect::<Vec<_>>().join(" "));
    if phrase.is_empty() {
        return Err("empty mnemonic".into());
    }
    set_protection(&phrase, protected)?;
    paths::mark_wallet_stored(restore_height).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn load(protected: bool) -> Result<(Zeroizing<String>, u64, bool), String> {
    if !is_marked_stored() {
        return Err("no stored wallet".into());
    }
    #[cfg(target_os = "macos")]
    if protected {
        match protected_macos::load() {
            Ok(phrase) => return Ok((phrase, paths::load_restore_height(), true)),
            Err(error) if error.code() == -25300 => {
                crate::device_auth::authenticate(&crate::l10n::t(
                    "Authenticate to unlock nexawal",
                ))?;
            }
            Err(error) => return Err(format!("Protected Keychain unlock failed: {error}")),
        }
    }
    let mnemonic = match entry()?.get_password() {
        Ok(value) => Zeroizing::new(value),
        #[cfg(target_os = "macos")]
        Err(error) if may_recover_protected_item(&error, protected) => {
            // A committed protection migration can outlive a failed preference write.
            // Only an absent legacy item permits this authenticated recovery; denial or
            // cancellation must never select another backend.
            let phrase = protected_macos::load()
                .map_err(|error| format!("Protected Keychain unlock failed: {error}"))?;
            return Ok((phrase, paths::load_restore_height(), true));
        }
        Err(error) => return Err(load_error(&error)),
    };
    let phrase = Zeroizing::new(mnemonic.split_whitespace().collect::<Vec<_>>().join(" "));
    if phrase.is_empty() {
        return Err("stored mnemonic was empty".into());
    }
    if protected {
        set_protection(&phrase, true)?;
    }
    Ok((phrase, paths::load_restore_height(), protected))
}

#[cfg(any(target_os = "macos", test))]
fn may_recover_protected_item(error: &keyring::Error, protected: bool) -> bool {
    !protected && matches!(error, keyring::Error::NoEntry)
}

fn commit_replacement(
    write: impl FnOnce() -> Result<(), String>,
    retire: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    write()?;
    retire()
}

/// Commit the replacement before retiring the old item. Failed migration never loses the seed.
pub fn set_protection(mnemonic: &str, protected: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if protected {
        return commit_replacement(
            || {
                protected_macos::save(mnemonic).map_err(|error| {
                    format!(
                        "Protected Keychain save failed (a signed build may be required): {error}"
                    )
                })
            },
            || match entry()?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(format!("Could not retire legacy Keychain seed: {error}")),
            },
        );
    }
    #[cfg(not(target_os = "macos"))]
    if protected {
        return Err("Device authentication is not available on this platform.".into());
    }
    entry()?
        .set_password(mnemonic)
        .map_err(|error| format!("{} save failed: {error}", secure_store_name()))?;
    #[cfg(target_os = "macos")]
    protected_macos::delete()
        .map_err(|error| format!("Could not retire protected Keychain seed: {error}"))?;
    Ok(())
}

pub fn delete() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    protected_macos::delete()
        .map_err(|error| format!("Could not remove protected Keychain seed: {error}"))?;
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

#[cfg(target_os = "macos")]
mod protected_macos {
    use security_framework::{
        access_control::{ProtectionMode, SecAccessControl},
        base::Result,
        passwords::{
            delete_generic_password_options, generic_password, set_generic_password_options,
            PasswordOptions,
        },
        passwords_options::AccessControlOptions,
    };
    use zeroize::Zeroizing;
    fn options() -> PasswordOptions {
        let mut options = PasswordOptions::new_generic_password(
            super::SERVICE,
            "wallet.mnemonic.user-presence.v1",
        );
        options.use_protected_keychain();
        options.set_access_synchronized(Some(false));
        options
    }
    pub fn save(phrase: &str) -> Result<()> {
        let mut options = options();
        options.set_access_control(SecAccessControl::create_with_protection(
            Some(ProtectionMode::AccessibleWhenUnlockedThisDeviceOnly),
            AccessControlOptions::USER_PRESENCE.bits(),
        )?);
        set_generic_password_options(phrase.as_bytes(), options)
    }
    pub fn load() -> Result<Zeroizing<String>> {
        let bytes = Zeroizing::new(generic_password(options())?);
        let phrase = std::str::from_utf8(&bytes)
            .map_err(|_| security_framework::base::Error::from_code(-50))?;
        Ok(Zeroizing::new(phrase.to_owned()))
    }
    pub fn delete() -> Result<()> {
        match delete_generic_password_options(options()) {
            Err(error) if error.code() == -25300 => Ok(()),
            result => result,
        }
    }
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
    fn only_missing_legacy_item_permits_authenticated_recovery() {
        assert!(may_recover_protected_item(&keyring::Error::NoEntry, false));
        assert!(!may_recover_protected_item(&keyring::Error::NoEntry, true));
        let denied = keyring::Error::NoStorageAccess(Box::new(std::io::Error::from(
            std::io::ErrorKind::PermissionDenied,
        )));
        assert!(!may_recover_protected_item(&denied, false));
        assert!(!may_recover_protected_item(
            &keyring::Error::BadEncoding(vec![255]),
            false
        ));
    }

    #[test]
    fn migration_failure_never_retires_the_previous_seed() {
        use std::cell::Cell;
        let retired = Cell::new(false);
        assert!(commit_replacement(
            || Err("synthetic write failure".into()),
            || {
                retired.set(true);
                Ok(())
            }
        )
        .is_err());
        assert!(!retired.get());
        let committed = Cell::new(false);
        assert!(commit_replacement(
            || {
                committed.set(true);
                Ok(())
            },
            || Err("synthetic retirement failure".into())
        )
        .is_err());
        assert!(committed.get());
    }

    #[test]
    fn no_entry_explains_how_to_migrate() {
        let message = load_error(&keyring::Error::NoEntry);
        assert!(message.contains(secure_store_name()));
        assert!(message.contains("Enter the seed once"));
    }
}
