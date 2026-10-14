//! Native teardown after the UI has hidden the wallet and signing has finished.
use monerowalletcore::api;
use std::future::Future;
use std::path::PathBuf;

/// A slow/cancelled RPC may outlast one lifecycle wait. Keep ownership blocked
/// and retry automatically, so leaving the app alone cannot leave keys loaded.
pub async fn retry_until_stopped<T, F, R, W, P>(mut operation: F, mut pause: W) -> api::Result<T>
where
    F: FnMut() -> R,
    R: Future<Output = api::Result<T>>,
    W: FnMut() -> P,
    P: Future<Output = ()>,
{
    loop {
        match operation().await {
            Err(error) if error.code == -31 => pause().await,
            result => return result,
        }
    }
}

pub fn close(
    wallet_id: &str,
    cache_path: PathBuf,
    persist_cache: bool,
) -> api::Result<Option<String>> {
    // Seal joins/cancels refresh and removes spend authority before exporting.
    // A cancelled refresh may have committed one last batch: export after the join.
    match api::seal(wallet_id, 30_000) {
        Ok(()) => {}
        Err(error) if error.code == -13 => return Ok(None), // Already closed.
        Err(error) => return Err(error),
    }
    let cache_error = if persist_cache {
        api::export_cache(wallet_id)
            .map_err(|error| error.to_string())
            .and_then(|blob| {
                crate::paths::write_bytes(cache_path, &blob).map_err(|error| error.to_string())
            })
            .err()
    } else {
        None
    };
    // A disk error must not leave the private view key resident either.
    api::close(wallet_id, 30_000)?;
    Ok(cache_error)
}

/// Join the old unsealed scan and strip spend authority before resuming view-only work.
pub fn seal_for_background(wallet_id: &str, cache_path: PathBuf) -> api::Result<Option<String>> {
    api::seal(wallet_id, 30_000)?;
    Ok(api::export_cache(wallet_id).map_err(|error| error.to_string())
        .and_then(|blob| crate::paths::write_bytes(cache_path, &blob).map_err(|error| error.to_string())).err())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ui-tests")]
    #[gpui::test]
    async fn lifecycle_timeouts_retry_without_waiting_for_another_unlock() {
        let attempts = std::cell::Cell::new(0);
        let pauses = std::cell::Cell::new(0);
        let result: api::Result<()> = retry_until_stopped(
            || {
                attempts.set(attempts.get() + 1);
                std::future::ready(if attempts.get() < 3 {
                    Err(api::Error {
                        code: -31,
                        message: "synthetic lifecycle timeout".into(),
                    })
                } else {
                    Ok(())
                })
            },
            || {
                pauses.set(pauses.get() + 1);
                std::future::ready(())
            },
        )
        .await;
        result.unwrap();
        assert_eq!(attempts.get(), 3);
        assert_eq!(pauses.get(), 2);
    }

    #[cfg(feature = "ui-tests")]
    #[gpui::test]
    async fn unexpected_lifecycle_errors_stop_instead_of_busy_retrying() {
        let result: api::Result<()> = retry_until_stopped(
            || {
                std::future::ready(Err(api::Error {
                    code: -16,
                    message: "synthetic invalid state".into(),
                }))
            },
            || async { panic!("unexpected errors must not retry automatically") },
        )
        .await;
        assert_eq!(result.unwrap_err().code, -16);
    }

    #[test]
    fn lock_drops_native_authority_preserves_cache_and_can_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let id = "gpui-native-lock-regression";
        let seed = api::generate_mnemonic_english().unwrap();
        api::open_from_mnemonic(id, &seed, 123, true).unwrap();
        assert!(!api::is_sealed(id).unwrap());
        let cache = directory.path().join("wallet.cache");
        assert!(close(id, cache.clone(), true).unwrap().is_none());
        assert_eq!(api::is_sealed(id).unwrap_err().code, -13);
        assert!(api::get_balance(id).is_err());
        let blob = std::fs::read(&cache).unwrap();
        api::open_from_mnemonic(id, &seed, 123, true).unwrap();
        api::import_cache(id, &blob).unwrap();
        assert_eq!(api::sync_status(id).unwrap().restore_height, 123);
        close(id, cache, false).unwrap();
    }

    #[test]
    fn cache_failure_still_closes_native_wallet() {
        let directory = tempfile::tempdir().unwrap();
        let id = "gpui-native-lock-disk-error";
        let seed = api::generate_mnemonic_english().unwrap();
        api::open_from_mnemonic(id, &seed, 123, true).unwrap();
        // An existing directory cannot be replaced by an atomic cache file.
        assert!(
            close(id, directory.path().to_owned(), true)
                .unwrap()
                .is_some()
        );
        assert_eq!(api::is_sealed(id).unwrap_err().code, -13);
    }
}
