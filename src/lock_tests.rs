//! Exercise Home's lock handlers with synthetic native wallets, never personal data, Keychain or public RPC.
use super::*;

#[gpui::test]
fn explorer_starts_off_and_enabling_does_not_start_a_lookup(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| {
            assert!(!home.explorer_enabled);
            home.opened = true;
            home.open_explorer("a".repeat(64), cx);
            assert!(home.explorer_lookup.visible);
            assert!(!home.explorer_lookup.loading);
            assert!(home.explorer_lookup.result.is_none());
            home.toggle_explorer(cx);
            assert!(home.explorer_enabled && paths::load_explorer_enabled());
            assert!(!home.explorer_lookup.visible && !home.explorer_lookup.loading);
            let token = home.explorer_lookup.cancel.clone();
            home.toggle_explorer(cx);
            assert!(!paths::load_explorer_enabled());
            assert!(token.load(AtomicOrdering::Acquire));
        });
    });
}
use gpui::TestAppContext;
static NATIVE_SESSION_FIXTURE: Mutex<()> = Mutex::new(());

#[gpui::test]
fn send_click_only_opens_review_and_changed_intents_are_rejected(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        let seed = api::generate_mnemonic_english().unwrap();
        let destination = api::primary_address_from_mnemonic(&seed, true).unwrap();
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| {
            home.opened = true; home.require_device_auth = false;
            home.send_dest = destination.clone(); home.send_amount = "0.000000000010".into();
            home.send_fee = Some(1); home.send_preview_amount = Some(10);
            home.run_send(cx);
            assert_eq!(home.send_confirmation.as_ref().unwrap().destination, destination);
            assert!(!home.send_operation.is_busy());
            assert!(paths::load_pending_send().unwrap().is_none());
            home.send_amount = "0.000000000020".into();
            home.confirm_send(cx);
            assert!(!home.send_operation.is_busy());
            assert!(home.send_confirmation.is_none() && home.send_fee.is_none());
            assert!(paths::load_pending_send().unwrap().is_none());
            // A lock invalidates a review even when the form later looks identical.
            home.opened = true; home.send_fee = Some(1); home.send_preview_amount = Some(20);
            home.run_send(cx);
            home.session_policy.invalidate();
            home.confirm_send(cx);
            assert!(!home.send_operation.is_busy());
            assert!(paths::load_pending_send().unwrap().is_none());
        });
    });
}

#[gpui::test]
fn removal_click_requires_a_separate_backup_confirmation(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        paths::save_pending_send("fixture approved journal").unwrap();
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| {
            home.remove_stored_wallet(cx);
            assert!(home.removal_confirmation);
            assert!(!home.removal_backup_confirmed);
            assert_eq!(paths::load_pending_send().unwrap().as_deref(), Some("fixture approved journal"));
            assert!(!home.native_close_pending);
        });
    });
}

#[gpui::test]
fn background_privacy_limit_closes_a_sealed_wallet_and_keeps_its_cache(cx: &mut TestAppContext) {
    let _guard = NATIVE_SESSION_FIXTURE.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        CACHE_PERSISTENCE_SUPPRESSED.store(false, AtomicOrdering::Relaxed);
        let seed = api::generate_mnemonic_english().unwrap();
        api::open_from_mnemonic(WALLET_ID, &seed, 123, true).unwrap();
        session_lock::seal_for_background(WALLET_ID, paths::cache_path()).unwrap();
        assert!(api::is_sealed(WALLET_ID).unwrap());
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| {
            let started = Instant::now();
            home.native_background_sync = true;
            home.native_background_started = Some(started);
            home.security_tick(started + session_policy::BACKGROUND_SYNC_LIMIT, cx);
            assert!(!home.opened && home.mnemonic.is_empty());
            assert!(!home.native_background_sync && home.native_close_pending);
        });
        cx.run_until_parked();
        assert_eq!(api::is_sealed(WALLET_ID).unwrap_err().code, -13);
        assert!(paths::cache_path().is_file());
    });
}

#[gpui::test]
fn background_scan_seals_before_restart_and_never_reopens_ui(cx: &mut TestAppContext) {
    use std::{io::{Read, Write}, net::TcpListener};
    let _guard = NATIVE_SESSION_FIXTURE.lock().unwrap();
    let directory = tempfile::tempdir().unwrap();
    paths::with_test_data_dir(directory.path().to_owned(), || {
        CACHE_PERSISTENCE_SUPPRESSED.store(false, AtomicOrdering::Relaxed);
        let seed = api::generate_mnemonic_english().unwrap();
        api::open_from_mnemonic(WALLET_ID, &seed, 123, true).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                        std::thread::sleep(Duration::from_millis(2)),
                    Err(error) => panic!("fixture HTTP accept failed: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request = [0; 8192];
            stream.read(&mut request).unwrap();
            // A controlled RPC failure must stay behind the locked surface, not reopen UI.
            let _ = stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
        });
        let home = cx.new(Home::new);
        home.update(cx, |home, cx| {
            home.opened = true;
            home.mnemonic = seed.clone(); home.seed = seed.clone();
            home.node_url = endpoint;
            home.lock_wallet(true, cx);
            assert!(!home.opened && home.mnemonic.is_empty() && home.seed.is_empty());
        });
        cx.run_until_parked();
        assert!(api::is_sealed(WALLET_ID).unwrap(), "restart must not retain spend authority");
        home.update(cx, |home, _| {
            assert!(home.native_background_sync && !home.native_close_pending);
            assert!(!home.opened);
        });
        server.join().unwrap();
        home.update(cx, |home, cx| home.forget(cx));
        cx.run_until_parked();
        assert_eq!(api::is_sealed(WALLET_ID).unwrap_err().code, -13);
        assert!(paths::cache_path().is_file());
    });
}

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
