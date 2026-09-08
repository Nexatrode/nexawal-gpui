//! Exercise Home's real lock and completion handlers without a wallet, Keychain or RPC.
use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn busy_lock_hides_wallet_blocks_replacement_and_ignores_late_completion(cx: &mut TestAppContext) {
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
            assert!(!home.opened && home.screen == Screen::Restore);
            assert!(home.send_fee.is_none());
            assert_eq!(paths::load_pending_send().unwrap().as_deref(), Some("synthetic pending journal"));
        });
    });
}
