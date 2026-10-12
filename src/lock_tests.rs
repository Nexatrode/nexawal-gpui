//! Exercise Home's lock handlers with synthetic native wallets, never personal data, Keychain or RPC.
use super::*;
use gpui::TestAppContext;
static NATIVE_SESSION_FIXTURE: Mutex<()> = Mutex::new(());

#[gpui::test]
fn idle_lock_closes_native_keys_before_a_new_session_can_open(cx: &mut TestAppContext) {
    let _guard = NATIVE_SESSION_FIXTURE.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        CACHE_PERSISTENCE_SUPPRESSED.store(false, AtomicOrdering::Relaxed);
        let seed = api::generate_mnemonic_english().unwrap();
        api::open_from_mnemonic(WALLET_ID, &seed, 123, true).unwrap();
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| {
            home.opened = true;
            home.mnemonic = seed.clone();
            home.seed = seed.clone();
            home.challenge_answers = ["one".into(), "two".into(), "three".into()];
            home.forget(cx);
            assert!(home.native_close_pending);
            assert!(!home.opened && home.mnemonic.is_empty() && home.seed.is_empty());
            assert!(home.challenge_answers.iter().all(String::is_empty));
            home.open_with_mnemonic("must not reach native open", 0, cx);
            assert!(!home.opened);
        });
        cx.run_until_parked();
        home.update(cx, |home, _| {
            assert!(!home.native_close_pending && !home.native_close_failed);
            assert!(!home.opened);
        });
        assert_eq!(api::is_sealed(WALLET_ID).unwrap_err().code, -13);
        let cache = fs::read(paths::cache_path()).unwrap();
        api::open_from_mnemonic(WALLET_ID, &seed, 123, true).unwrap();
        api::import_cache(WALLET_ID, &cache).unwrap();
        api::close(WALLET_ID, 1_000).unwrap();
    });
}

#[gpui::test]
fn busy_lock_hides_wallet_blocks_replacement_and_ignores_late_completion(cx: &mut TestAppContext) {
    let _guard = NATIVE_SESSION_FIXTURE.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| {
            home.opened = true;
            home.screen = Screen::Wallet;
            home.mnemonic = "synthetic secret".into();
            home.seed = "synthetic secret".into();
            home.address = "synthetic address".into();
            home.total_piconero = 835_000_000;
            home.unlocked_piconero = 835_000_000;
            home.send_dest = "synthetic recipient".into();
            home.send_fee = Some(5);
            home.send_preview_amount = Some(10);
            let preview = home.send_preview_epoch.token();
            home.send_operation.start();
            // A signed journal must survive lock/removal attempts and late errors.
            paths::save_pending_send("synthetic pending journal").unwrap();

            home.forget(cx);
            assert!(!home.opened);
            assert!(home.screen == Screen::Restore);
            assert!(home.mnemonic.is_empty() && home.seed.is_empty() && home.address.is_empty());
            assert_eq!((home.total_piconero, home.unlocked_piconero), (0, 0));
            assert!(home.send_dest.is_empty() && home.send_fee.is_none());
            assert!(home.send_preview_amount.is_none());
            assert!(!home.send_preview_epoch.accepts(preview));
            assert!(home.send_operation.is_busy());

            home.open_with_mnemonic("must never reach native open", 0, cx);
            home.try_unlock_stored(cx);
            home.remove_stored_wallet(cx);
            assert!(!home.opened);
            assert_eq!(paths::load_pending_send().unwrap().as_deref(), Some("synthetic pending journal"));

            // This is the guard used by success AND error callbacks for previews,
            // sends, and journal recovery. It must suppress normal result publication.
            assert!(!home.finish_send_operation(cx));
            assert!(!home.send_operation.is_busy());
            assert!(home.native_close_pending);
            home.open_with_mnemonic("must wait for native teardown", 0, cx);
            assert!(!home.opened);
            assert!(!home.opened && home.screen == Screen::Restore);
            assert!(home.send_fee.is_none());
            assert_eq!(paths::load_pending_send().unwrap().as_deref(), Some("synthetic pending journal"));
        });
    });
}
