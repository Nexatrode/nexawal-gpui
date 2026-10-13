//! Exercise Home's lock handlers with synthetic native wallets, never personal data, Keychain or RPC.
use super::*;
use gpui::TestAppContext;
static NATIVE_SESSION_FIXTURE: Mutex<()> = Mutex::new(());

#[gpui::test]
fn damaged_receive_book_blocks_open_and_all_edits_without_overwriting_it(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        let damaged = "corrupt existing receive book";
        fs::write(paths::receive_book_path(), damaged).unwrap();
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| {
            // Book failure must occur before mnemonic/native open; no RPC or secure store.
            home.open_with_mnemonic("must not reach native open", 0, cx);
            assert!(!home.opened);
            assert!(home.status.contains("Address failed"));
            let original = home.receive_book.clone();
            home.recv_label = "must not overwrite".into();
            home.persist_recv_label();
            home.cycle_receive_address(true, cx);
            assert_eq!(home.receive_book, original);
            assert_eq!(fs::read_to_string(paths::receive_book_path()).unwrap(), damaged);
        });
    });
}

#[test]
fn corrupt_or_changed_receive_book_cannot_issue_a_reused_index() {
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        let mut book = receive_book::Book::primary();
        let damaged = "corrupt existing book";
        fs::write(paths::receive_book_path(), damaged).unwrap();
        assert!(book.update_and_save(|b| b.allocate_new("Shop")).is_err());
        assert_eq!(book.next_index, 1);
        assert_eq!(fs::read_to_string(paths::receive_book_path()).unwrap(), damaged);
        fs::write(paths::receive_book_path(), "52\n51\n0|Primary\n51|Shop\n").unwrap();
        assert!(book.update_and_save(|b| b.allocate_new("Shop")).is_err());
        book = receive_book::load().unwrap();
        assert_eq!(book.update_and_save(|b| b.allocate_new("Tips")).unwrap(), 52);
        assert_eq!(receive_book::load().unwrap(), book);
    });
}

#[gpui::test]
fn failed_receive_book_write_does_not_publish_a_new_address(cx: &mut TestAppContext) {
    let _guard = NATIVE_SESSION_FIXTURE.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        let seed = api::generate_mnemonic_english().unwrap();
        api::open_from_mnemonic(WALLET_ID, &seed, 123, true).unwrap();
        let home = cx.new(Home::new);
        // An existing directory cannot be atomically replaced by the book file.
        fs::create_dir(paths::receive_book_path()).unwrap();
        home.update(cx, |home, cx| {
            home.receive_book.next_index = 51;
            home.receive_address = "unchanged address".into();
            let original = home.receive_book.clone();
            home.create_receive_subaddress(cx);
            assert_eq!(home.receive_book, original);
            assert_eq!(home.receive_address.as_ref(), "unchanged address");
            assert!(home.status.contains("Address failed"));
        });
        api::close(WALLET_ID, 1_000).unwrap();
    });
}

#[gpui::test]
fn history_search_and_date_fields_accept_native_text(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        let (home, cx) = cx.add_window_view(|_, cx| Home::new(cx));
        cx.update(|window, cx| {
            home.update(cx, |home, cx| {
                home.opened = true;
                home.screen = Screen::Transactions;
                for field in [Field::TransferSearch, Field::HistoryFrom, Field::HistoryTo] {
                    home.active = field;
                    assert!(home.accepts_text_input(window, cx));
                }
                home.screen = Screen::Legal;
                assert!(!home.accepts_text_input(window, cx));
            });
        });
    });
}

#[gpui::test]
fn automatic_idle_boundary_hides_keys_and_invalidates_unlock(cx: &mut TestAppContext) {
    let _guard = NATIVE_SESSION_FIXTURE.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| {
            let now = Instant::now();
            home.session_policy.touch(now);
            let attempt = home.session_policy.token();
            home.seed = "synthetic secret".into();
            home.unlocking = true;
            home.security_tick(now + session_policy::IDLE_TIMEOUT, cx);
            assert!(home.seed.is_empty() && !home.opened && !home.unlocking);
            assert!(!home.session_policy.accepts(attempt));
        });
        cx.run_until_parked();
    });
}

#[gpui::test]
fn secret_clipboard_expires_without_erasing_a_new_copy(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| home.copy_secret("synthetic clipboard secret".into(), cx));
        cx.run_until_parked();
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("synthetic clipboard secret"));
        cx.executor().advance_clock(session_policy::CLIPBOARD_TIMEOUT);
        cx.run_until_parked();
        assert!(cx.read_from_clipboard().and_then(|item| item.text()).is_none_or(|text| text.is_empty()));
        home.update(cx, |home, cx| home.copy_secret("another synthetic secret".into(), cx));
        cx.run_until_parked();
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("a later ordinary copy".into()));
        cx.executor().advance_clock(session_policy::CLIPBOARD_TIMEOUT);
        cx.run_until_parked();
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("a later ordinary copy"));
    });
}

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
