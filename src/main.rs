use std::cmp::Ordering;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use gpui::SystemMenuType;
use gpui::{
    App, Bounds, ClickEvent, ClipboardEntry, Context, CursorStyle, FocusHandle, Focusable,
    ImageSource, KeyBinding, KeyDownEvent, Menu, MenuItem, MouseButton, ObjectFit, OsAction,
    PathPromptOptions, RenderImage, ResizeEdge, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, actions, div, img, prelude::*, px, relative, rgb, size,
};
use gpui_platform::application;
use monerowalletcore::api::{self, RefreshJob, SyncStatus, Transfer};

mod amount;
mod benchmark;
mod camera;
mod daemon;
mod device_auth;
mod fiat;
mod fiat_snapshot;
mod l10n;
mod legal;
mod network;
mod paths;
mod platform_icon;
mod qr;
mod receive_book;
mod scan_tuning;
mod seed_backup;
mod send_flow;
mod sync_status;
mod uri;
mod wallet_store;

actions!(
    nexawal,
    [
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        ShowApp,
        Minimize,
        PasteField,
        CopyField,
        CutField,
        SelectAllField,
        BackspaceField,
        OpenWallet,
        RetrySync,
        CopyAddress,
        ShowReceive,
        ShowSend,
        ShowWallet,
        ShowSettings
    ]
);

const WALLET_ID: &str = "main_wallet";
const GAP_LIMIT: u32 = 50;
const BG: u32 = 0x0A0F0A;
const CARD: u32 = 0x121812;
const ROW: u32 = 0x161E16;
const FIELD: u32 = 0x0D140D;
const TEXT: u32 = 0xF2F2F2;
const MUTED: u32 = 0x8A9A8A;
const ACCENT: u32 = 0xFF6B35;
const IN: u32 = 0x6EE7B7;
const OUT: u32 = 0xF87171;
const ACTIVE_SYNC_AUX_POLL_INTERVAL: Duration = Duration::from_secs(10);
const ACTIVE_SYNC_CACHE_INTERVAL: Duration = Duration::from_secs(120);
const ACTIVE_SYNC_CACHE_BLOCK_DELTA: u64 = 1_000;

fn should_auto_unlock_stored() -> bool {
    let initial_seed = env_or("NEXAWAL_MNEMONIC", "");
    wallet_store::is_marked_stored() && !paths::terms_need_accept() && initial_seed.is_empty()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Seed,
    Height,
    Dest,
    Amount,
    Node,
    RecvAmount,
    RecvDesc,
    RecvLabel,
    Challenge,
    I2pNode,
    I2pProxy,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AmountMode {
    Xmr,
    Fiat,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Terms,
    Restore,
    Wallet,
    Receive,
    Send,
    Settings,
    Legal,
}

struct Home {
    core_version: SharedString,
    node_url: String,
    seed: String,
    restore_height_text: String,
    active: Field,
    ui_focus: FocusHandle,
    seed_focus: FocusHandle,
    height_focus: FocusHandle,
    dest_focus: FocusHandle,
    amount_focus: FocusHandle,
    node_focus: FocusHandle,
    recv_amount_focus: FocusHandle,
    recv_desc_focus: FocusHandle,
    recv_label_focus: FocusHandle,
    challenge_focus: FocusHandle,
    i2p_rpc_focus: FocusHandle,
    i2p_proxy_focus: FocusHandle,
    status: SharedString,
    opened: bool,
    screen: Screen,
    legal_doc: legal::Document,
    legal_return: Screen,
    terms_checked: bool,
    has_stored: bool,
    show_restore_form: bool,
    unlocking: bool,
    unlock_started: bool,
    mnemonic: String,
    address: SharedString,
    receive_address: SharedString,
    receive_book: receive_book::Book,
    require_device_auth: bool,
    total_piconero: u64,
    unlocked_piconero: u64,
    sync: Option<SyncStatus>,
    transfers: Vec<Transfer>,
    last_exported_scanned: Option<u64>,
    last_cache_persist_at: Option<Instant>,
    last_balance_poll_at: Option<Instant>,
    last_transfers_poll_at: Option<Instant>,
    polling: bool,
    scan_needs_retry: bool,
    scan_was_running: bool,
    last_scan_error: Option<String>,
    scan_rate: sync_status::ScanRate,
    scan_details_expanded: bool,
    last_scan_progress_at: Option<Instant>,
    last_scanned_for_stall: u64,
    benchmark_running: bool,
    benchmark_status: Option<SharedString>,
    address_copied_at: Option<Instant>,
    send_dest: String,
    send_amount: String,
    send_max: bool,
    send_busy: bool,
    send_fee: Option<u64>,
    send_preview_amount: Option<u64>,
    send_amount_mode: AmountMode,
    send_from_subaddress: bool,
    send_source_index: u32,
    send_source_unlocked: Option<u64>,
    send_qr_busy: bool,
    recv_amount_mode: AmountMode,
    recv_amount: String,
    recv_desc: String,
    recv_label: String,
    created_seed: bool,
    wrote_seed_down: bool,
    challenge_indices: Vec<usize>,
    challenge_answers: [String; 3],
    challenge_slot: usize,
    height_fetching: bool,
    qr_uri: String,
    qr_image: Option<Arc<RenderImage>>,
    fiat_enabled: bool,
    fiat_currency: String,
    fiat_rate: Option<fiat::Rate>,
    fiat_fetching: bool,
    fiat_snapshots: fiat_snapshot::Store,
    network_policy: network::Policy,
    i2p_rpc: String,
    i2p_proxy: String,
}

impl Home {
    fn new(cx: &mut Context<Self>) -> Self {
        let has_stored = wallet_store::is_marked_stored();
        let needs_terms = paths::terms_need_accept();
        let initial_seed = env_or("NEXAWAL_MNEMONIC", "");
        let show_restore_form = !has_stored || !initial_seed.trim().is_empty();
        let should_auto_unlock = should_auto_unlock_stored();
        let saved_auth_preference = paths::load_device_auth_preference();
        let require_device_auth =
            saved_auth_preference.unwrap_or_else(|| has_stored && device_auth::is_available());
        if has_stored && saved_auth_preference.is_none() && require_device_auth {
            let _ = paths::save_device_auth(true);
        }

        Self {
            core_version: api::version().into(),
            node_url: std::env::var("NEXAWAL_NODE_URL")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(paths::load_node_url),
            seed: initial_seed.clone(),
            restore_height_text: {
                let from_env = std::env::var("NEXAWAL_RESTORE_HEIGHT")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                if let Some(value) = from_env {
                    value
                } else if has_stored {
                    paths::load_restore_height().to_string()
                } else {
                    "0".into()
                }
            },
            active: Field::Seed,
            ui_focus: cx.focus_handle(),
            seed_focus: cx.focus_handle(),
            height_focus: cx.focus_handle(),
            dest_focus: cx.focus_handle(),
            amount_focus: cx.focus_handle(),
            node_focus: cx.focus_handle(),
            recv_amount_focus: cx.focus_handle(),
            recv_desc_focus: cx.focus_handle(),
            recv_label_focus: cx.focus_handle(),
            challenge_focus: cx.focus_handle(),
            i2p_rpc_focus: cx.focus_handle(),
            i2p_proxy_focus: cx.focus_handle(),
            status: if should_auto_unlock {
                l10n::t("Unlocking Wallet...").into()
            } else if has_stored {
                l10n::t("Open the existing wallet from {}, or restore a different seed.")
                    .replace("{}", wallet_store::secure_store_name())
                    .into()
            } else {
                l10n::t("Click the seed box, paste with ⌘V, set restore height, then Open & sync.")
                    .into()
            },
            opened: false,
            screen: if needs_terms {
                Screen::Terms
            } else {
                Screen::Restore
            },
            legal_doc: legal::Document::Terms,
            legal_return: Screen::Restore,
            terms_checked: false,
            has_stored,
            show_restore_form,
            unlocking: should_auto_unlock,
            unlock_started: false,
            mnemonic: String::new(),
            address: "".into(),
            receive_address: "".into(),
            receive_book: receive_book::load(),
            require_device_auth,
            total_piconero: 0,
            unlocked_piconero: 0,
            sync: None,
            transfers: Vec::new(),
            last_exported_scanned: None,
            last_cache_persist_at: None,
            last_balance_poll_at: None,
            last_transfers_poll_at: None,
            polling: false,
            scan_needs_retry: false,
            scan_was_running: false,
            last_scan_error: None,
            scan_rate: sync_status::ScanRate::default(),
            scan_details_expanded: paths::load_sync_details_expanded(),
            last_scan_progress_at: None,
            last_scanned_for_stall: 0,
            benchmark_running: false,
            benchmark_status: None,
            address_copied_at: None,
            send_dest: String::new(),
            send_amount: String::new(),
            send_max: false,
            send_busy: false,
            send_fee: None,
            send_preview_amount: None,
            send_amount_mode: AmountMode::Xmr,
            send_from_subaddress: false,
            send_source_index: 0,
            send_source_unlocked: None,
            send_qr_busy: false,
            recv_amount_mode: AmountMode::Xmr,
            recv_amount: String::new(),
            recv_desc: String::new(),
            recv_label: String::new(),
            created_seed: false,
            wrote_seed_down: false,
            challenge_indices: Vec::new(),
            challenge_answers: Default::default(),
            challenge_slot: 0,
            height_fetching: false,
            qr_uri: String::new(),
            qr_image: None,
            fiat_enabled: paths::load_fiat_enabled(),
            fiat_currency: paths::load_fiat_currency(),
            fiat_rate: paths::load_fiat_rate(),
            fiat_fetching: false,
            fiat_snapshots: fiat_snapshot::Store::load(),
            network_policy: paths::load_network_policy(),
            i2p_rpc: paths::load_i2p_rpc(),
            i2p_proxy: paths::load_i2p_proxy(),
        }
    }

    fn restore_height(&self) -> u64 {
        self.restore_height_text.trim().parse().unwrap_or(0)
    }

    fn word_count(&self) -> usize {
        self.seed.split_whitespace().count()
    }

    fn seed_backup_passed(&self) -> bool {
        !self.created_seed
            || (self.wrote_seed_down
                && seed_backup::answers_match(
                    &self.seed,
                    &self.challenge_indices,
                    &self.challenge_answers,
                ))
    }

    fn mark_imported_seed(&mut self) {
        self.created_seed = false;
        self.wrote_seed_down = false;
        self.challenge_indices.clear();
        self.challenge_answers = Default::default();
        self.challenge_slot = 0;
    }

    fn persist_recv_label(&mut self) {
        self.receive_book.set_selected_label(&self.recv_label);
        receive_book::save(&self.receive_book);
    }

    fn sync_recv_label_from_book(&mut self) {
        self.recv_label = self.receive_book.selected_entry().label.clone();
    }

    fn fetch_suggested_height(&mut self, cx: &mut Context<Self>) {
        if self.height_fetching {
            return;
        }
        self.height_fetching = true;
        let node = self.scan_node_url();
        let proxy = self.scan_proxy();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    daemon::fetch_suggested_restore_height(&node, proxy.as_deref())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.height_fetching = false;
                match result {
                    Ok(height) => {
                        if this.created_seed || this.restore_height() == 0 {
                            this.restore_height_text = height.to_string();
                        }
                        this.status = format!(
                            "Starting height (fast): {}.",
                            this.restore_height()
                        )
                        .into();
                    }
                    Err(err) => {
                        this.status = format!(
                            "Couldn't fetch a fast restore height ({err}). Leaving restore height as 0."
                        )
                        .into();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn try_apply_clipboard_qr(
        &mut self,
        item: &gpui::ClipboardItem,
        cx: &mut Context<Self>,
    ) -> bool {
        for entry in item.entries() {
            let ClipboardEntry::Image(image) = entry else {
                continue;
            };
            let Some(payload) = qr::decode_bytes(&image.bytes) else {
                continue;
            };
            self.apply_payment_payload(&payload, cx);
            return true;
        }
        false
    }

    fn apply_payment_payload(&mut self, payload: &str, cx: &mut Context<Self>) {
        let trimmed = payload.trim();
        if let Some(parsed) = uri::parse(trimmed) {
            self.send_dest = parsed.address;
            if let Some(amt) = parsed.amount_xmr {
                self.send_amount = amt;
                self.send_max = false;
            }
            self.clear_send_preview();
            self.status = l10n::t("QR filled destination.").into();
        } else if uri::looks_like_address(trimmed) {
            self.send_dest = trimmed.to_string();
            self.clear_send_preview();
            self.status = l10n::t("QR filled destination.").into();
        } else {
            self.status = l10n::t("QR did not contain a Monero address.").into();
        }
        cx.notify();
    }

    fn paste_send_qr(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            self.status = l10n::t("Clipboard is empty.").into();
            cx.notify();
            return;
        };
        if self.try_apply_clipboard_qr(&item, cx) {
            return;
        }
        let saw_image = item
            .entries()
            .iter()
            .any(|entry| matches!(entry, ClipboardEntry::Image(_)));
        self.status = if saw_image {
            l10n::t("Clipboard image is not a readable QR code.").into()
        } else {
            l10n::t("Clipboard has no image. Copy a QR screenshot first.").into()
        };
        cx.notify();
    }

    fn open_send_qr_image(&mut self, cx: &mut Context<Self>) {
        if self.send_qr_busy {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open QR image".into()),
        });
        self.send_qr_busy = true;
        self.status = l10n::t("Choose a QR image…").into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let picked = receiver.await;
            let path = match picked {
                Ok(Ok(Some(mut paths))) => paths.pop(),
                Ok(Ok(None)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.send_qr_busy = false;
                        this.status = l10n::t("QR image cancelled.").into();
                        cx.notify();
                    });
                    return;
                }
                _ => {
                    let _ = this.update(cx, |this, cx| {
                        this.send_qr_busy = false;
                        this.status = l10n::t("Could not open the file picker.").into();
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(path) = path else {
                let _ = this.update(cx, |this, cx| {
                    this.send_qr_busy = false;
                    cx.notify();
                });
                return;
            };
            let payload = cx
                .background_executor()
                .spawn(async move { qr::decode_path(&path) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.send_qr_busy = false;
                match payload {
                    Some(payload) => this.apply_payment_payload(&payload, cx),
                    None => {
                        this.status = l10n::t("No QR code found in that image.").into();
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn scan_send_qr_camera(&mut self, cx: &mut Context<Self>) {
        if self.send_qr_busy {
            return;
        }
        self.send_qr_busy = true;
        self.status = l10n::t("Looking for a QR with the camera…").into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { camera::scan_qr() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.send_qr_busy = false;
                match result {
                    Ok(payload) => this.apply_payment_payload(&payload, cx),
                    Err(err) => {
                        this.status = err.into();
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn copy_seed(&mut self, cx: &mut Context<Self>) {
        if self.seed.trim().is_empty() {
            self.status = l10n::t("Seed field is empty.").into();
            cx.notify();
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(self.seed.clone()));
        self.status = l10n::t("Seed copied. Store it offline, then clear the clipboard.").into();
        cx.notify();
    }

    fn scan_node_url(&self) -> String {
        network::scan_node_url(self.network_policy, &self.node_url, &self.i2p_rpc)
    }

    fn broadcast_node_url(&self) -> String {
        network::broadcast_node_url(self.network_policy, &self.node_url, &self.i2p_rpc)
    }

    fn scan_proxy(&self) -> Option<String> {
        if network::should_use_i2p_http_proxy(
            self.network_policy,
            !self.i2p_proxy.trim().is_empty(),
            false,
        ) {
            Some(self.i2p_proxy.clone())
        } else {
            None
        }
    }

    fn broadcast_proxy(&self) -> Option<String> {
        if network::should_use_i2p_http_proxy(
            self.network_policy,
            !self.i2p_proxy.trim().is_empty(),
            true,
        ) {
            Some(self.i2p_proxy.clone())
        } else {
            None
        }
    }

    fn apply_scan_proxy(&self) {
        network::apply_http_proxy(self.scan_proxy().as_deref());
    }

    fn apply_broadcast_proxy(&self) {
        network::apply_http_proxy(self.broadcast_proxy().as_deref());
    }

    fn kick_refresh(&self) -> api::Result<()> {
        self.apply_scan_proxy();
        api::refresh_async(WALLET_ID, &self.scan_node_url())
    }

    fn start_fast_scan(&mut self) -> api::Result<()> {
        self.benchmark_status = None;
        scan_tuning::apply_for_node(&self.scan_node_url());
        self.last_scan_progress_at = Some(Instant::now());
        self.last_scanned_for_stall = 0;
        self.kick_refresh()
    }

    fn persist_scan_cache_at(&mut self, last_scanned: u64) {
        let saved = api::export_cache(WALLET_ID)
            .ok()
            .filter(|blob| !blob.is_empty())
            .is_some_and(|blob| paths::save_cache(&blob).is_ok());
        if saved {
            self.last_exported_scanned = Some(last_scanned);
            self.last_cache_persist_at = Some(Instant::now());
        }
    }

    fn persist_scan_cache(&mut self) {
        if let Ok(sync) = api::sync_status(WALLET_ID) {
            self.persist_scan_cache_at(sync.last_scanned);
        }
    }

    fn live_rate(&self) -> Option<&fiat::Rate> {
        if !self.fiat_enabled {
            return None;
        }
        fiat::live(self.fiat_rate.as_ref(), fiat::now_ms())
    }

    fn amount_piconero(&self, text: &str, mode: AmountMode) -> Option<u64> {
        match mode {
            AmountMode::Xmr => amount::parse_piconero(text),
            AmountMode::Fiat => self
                .live_rate()
                .and_then(|rate| fiat::piconero_from_fiat(text, rate)),
        }
    }

    fn send_piconero(&self) -> Option<u64> {
        self.amount_piconero(&self.send_amount, self.send_amount_mode)
    }

    fn send_from_minor(&self) -> Option<u32> {
        self.send_from_subaddress.then_some(self.send_source_index)
    }

    fn send_unlocked(&self) -> u64 {
        if self.send_from_subaddress {
            self.send_source_unlocked.unwrap_or(0)
        } else {
            self.unlocked_piconero
        }
    }

    fn refresh_send_source_balance(&mut self) {
        if !self.send_from_subaddress || !self.opened {
            self.send_source_unlocked = None;
            return;
        }
        match api::get_balance_for_subaddress(WALLET_ID, self.send_source_index) {
            Ok(balance) => self.send_source_unlocked = Some(balance.unlocked_piconero),
            Err(_) => self.send_source_unlocked = None,
        }
    }

    fn refresh_balance_snapshot(&mut self) {
        if let Ok(balance) = api::get_balance(WALLET_ID) {
            self.total_piconero = balance.total_piconero;
            self.unlocked_piconero = balance.unlocked_piconero;
        }
        self.refresh_send_source_balance();
    }

    fn refresh_transfers_snapshot(&mut self) {
        let Ok(mut rows) = api::list_transfers(WALLET_ID) else {
            return;
        };
        sort_transfers(&mut rows);
        if self.fiat_enabled {
            let rate = self.live_rate().cloned();
            let opted_in = paths::ensure_fiat_opted_in_at();
            self.fiat_snapshots.record_new_transfers(
                rows.iter().map(|t| (t.txid.as_str(), t.timestamp)),
                rate.as_ref(),
                opted_in,
            );
        }
        self.transfers = rows;
    }

    fn toggle_send_from_subaddress(&mut self, cx: &mut Context<Self>) {
        if self.send_busy {
            return;
        }
        self.send_from_subaddress = !self.send_from_subaddress;
        if self.send_from_subaddress {
            self.send_source_index = self.receive_book.selected;
        }
        self.send_max = false;
        self.clear_send_preview();
        self.refresh_send_source_balance();
        self.status = if self.send_from_subaddress {
            format!(
                "Spend from {} · unlocked {}.",
                self.receive_book.display_label_for(self.send_source_index),
                amount::format_xmr(self.send_unlocked())
            )
            .into()
        } else {
            l10n::t("Spend from the whole wallet.").into()
        };
        cx.notify();
    }

    fn cycle_send_source(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.send_busy || !self.send_from_subaddress {
            return;
        }
        self.send_source_index = self.receive_book.cycle_index(self.send_source_index, next);
        self.send_max = false;
        self.clear_send_preview();
        self.refresh_send_source_balance();
        self.status = format!(
            "Spend from {} · unlocked {}.",
            self.receive_book.display_label_for(self.send_source_index),
            amount::format_xmr(self.send_unlocked())
        )
        .into();
        cx.notify();
    }

    fn recv_piconero(&self) -> Option<u64> {
        self.amount_piconero(&self.recv_amount, self.recv_amount_mode)
    }

    fn amount_secondary(&self, piconero: u64, mode: AmountMode) -> Option<String> {
        match mode {
            AmountMode::Xmr => self.fiat_line(piconero),
            AmountMode::Fiat => Some(fiat::format_xmr_approx(piconero)),
        }
    }

    fn coerce_amount_modes(&mut self) {
        if self.live_rate().is_some() {
            return;
        }
        if self.send_amount_mode == AmountMode::Fiat {
            self.send_amount_mode = AmountMode::Xmr;
        }
        if self.recv_amount_mode == AmountMode::Fiat {
            self.recv_amount_mode = AmountMode::Xmr;
        }
    }

    fn swap_amount_mode(&mut self, send: bool, cx: &mut Context<Self>) {
        let Some(rate) = self.live_rate().cloned() else {
            self.status = l10n::t("Turn on fiat estimates first.").into();
            cx.notify();
            return;
        };
        if send {
            let pico = self.send_piconero();
            match self.send_amount_mode {
                AmountMode::Xmr => {
                    if let Some(pico) = pico {
                        self.send_amount = fiat::format_fiat_for_input(pico, &rate);
                    }
                    self.send_amount_mode = AmountMode::Fiat;
                }
                AmountMode::Fiat => {
                    if let Some(pico) = pico {
                        self.send_amount = amount::format_for_input(pico);
                    }
                    self.send_amount_mode = AmountMode::Xmr;
                }
            }
        } else {
            let pico = self.recv_piconero();
            match self.recv_amount_mode {
                AmountMode::Xmr => {
                    if let Some(pico) = pico {
                        self.recv_amount = fiat::format_fiat_for_input(pico, &rate);
                    }
                    self.recv_amount_mode = AmountMode::Fiat;
                }
                AmountMode::Fiat => {
                    if let Some(pico) = pico {
                        self.recv_amount = amount::format_for_input(pico);
                    }
                    self.recv_amount_mode = AmountMode::Xmr;
                }
            }
        }
        cx.notify();
    }

    fn cycle_network_policy(&mut self, cx: &mut Context<Self>) {
        self.network_policy = self.network_policy.next();
        let _ = paths::save_network_policy(self.network_policy);
        self.apply_network(cx);
    }

    fn apply_network(&mut self, cx: &mut Context<Self>) {
        let _ = paths::save_network_policy(self.network_policy);
        let _ = paths::save_i2p_rpc(&self.i2p_rpc);
        let _ = paths::save_i2p_proxy(&self.i2p_proxy);
        self.apply_scan_proxy();
        self.status = format!(
            "{} · scan {} · broadcast {}",
            self.network_policy.label(),
            self.scan_node_url(),
            self.broadcast_node_url()
        )
        .into();
        if self.opened {
            let _ = api::refresh_cancel(WALLET_ID);
            if let Err(err) = self.start_fast_scan() {
                self.status = format!("Saved network, but rescan failed: {err}").into();
            }
        }
        cx.notify();
    }

    fn focus_field(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        self.active = field;
        match field {
            Field::Seed => self.seed_focus.focus(window, cx),
            Field::Height => self.height_focus.focus(window, cx),
            Field::Dest => self.dest_focus.focus(window, cx),
            Field::Amount => self.amount_focus.focus(window, cx),
            Field::Node => self.node_focus.focus(window, cx),
            Field::RecvAmount => self.recv_amount_focus.focus(window, cx),
            Field::RecvDesc => self.recv_desc_focus.focus(window, cx),
            Field::RecvLabel => self.recv_label_focus.focus(window, cx),
            Field::Challenge => self.challenge_focus.focus(window, cx),
            Field::I2pNode => self.i2p_rpc_focus.focus(window, cx),
            Field::I2pProxy => self.i2p_proxy_focus.focus(window, cx),
        }
        cx.notify();
    }

    fn active_text_mut(&mut self) -> &mut String {
        match self.active {
            Field::Seed => &mut self.seed,
            Field::Height => &mut self.restore_height_text,
            Field::Dest => &mut self.send_dest,
            Field::Amount => &mut self.send_amount,
            Field::Node => &mut self.node_url,
            Field::RecvAmount => &mut self.recv_amount,
            Field::RecvDesc => &mut self.recv_desc,
            Field::RecvLabel => &mut self.recv_label,
            Field::Challenge => {
                let slot = self.challenge_slot.min(2);
                &mut self.challenge_answers[slot]
            }
            Field::I2pNode => &mut self.i2p_rpc,
            Field::I2pProxy => &mut self.i2p_proxy,
        }
    }

    fn paste_field(&mut self, _: &PasteField, _window: &mut Window, cx: &mut Context<Self>) {
        if self.screen == Screen::Terms || self.screen == Screen::Legal {
            return;
        }
        if self.opened
            && !matches!(
                self.screen,
                Screen::Send | Screen::Settings | Screen::Receive
            )
        {
            return;
        }
        if self.screen == Screen::Send {
            self.active = if self.active == Field::Amount {
                Field::Amount
            } else {
                Field::Dest
            };
        }
        if self.screen == Screen::Settings
            && !matches!(self.active, Field::Node | Field::I2pNode | Field::I2pProxy)
        {
            self.active = Field::Node;
        }
        if self.screen == Screen::Receive
            && !matches!(
                self.active,
                Field::RecvAmount | Field::RecvDesc | Field::RecvLabel
            )
        {
            self.active = Field::RecvAmount;
        }
        let Some(item) = cx.read_from_clipboard() else {
            self.status = l10n::t("Clipboard is empty.").into();
            cx.notify();
            return;
        };
        if self.screen == Screen::Send && self.try_apply_clipboard_qr(&item, cx) {
            return;
        }
        let Some(text) = item.text() else {
            self.status = l10n::t("Clipboard has no text.").into();
            cx.notify();
            return;
        };
        self.replace_active(text, cx);
    }

    fn paste_seed_button(&mut self, cx: &mut Context<Self>) {
        self.active = Field::Seed;
        let Some(item) = cx.read_from_clipboard() else {
            self.status = l10n::t("Clipboard is empty.").into();
            cx.notify();
            return;
        };
        let Some(text) = item.text() else {
            self.status = l10n::t("Clipboard has no text.").into();
            cx.notify();
            return;
        };
        self.seed = normalize_seed(&text);
        self.mark_imported_seed();
        self.status = format!(
            "Seed field has {} words. Set restore height, then Open & sync.",
            self.word_count()
        )
        .into();
        cx.notify();
    }

    fn replace_active(&mut self, text: String, cx: &mut Context<Self>) {
        match self.active {
            Field::Seed => {
                self.seed = normalize_seed(&text);
                self.mark_imported_seed();
                self.status = format!(
                    "Seed field has {} words. Set restore height, then Open & sync.",
                    self.word_count()
                )
                .into();
            }
            Field::Height => {
                self.restore_height_text = text.chars().filter(|c| c.is_ascii_digit()).collect();
                if self.restore_height_text.is_empty() {
                    self.restore_height_text = "0".into();
                }
                self.status = format!("Restore height {}.", self.restore_height()).into();
            }
            Field::Dest => {
                let trimmed = text.trim();
                if let Some(uri) = uri::parse(trimmed) {
                    self.send_dest = uri.address;
                    if let Some(amt) = uri.amount_xmr {
                        self.send_amount = amt;
                        self.send_max = false;
                    }
                    self.clear_send_preview();
                    self.status = l10n::t("Payment URI filled destination.").into();
                } else {
                    self.send_dest = trimmed.to_string();
                    self.clear_send_preview();
                    self.status = l10n::t("Destination filled.").into();
                }
            }
            Field::Amount => {
                self.send_amount = text.trim().replace(',', ".");
                self.send_max = false;
                self.clear_send_preview();
                self.status = l10n::t("Amount filled.").into();
            }
            Field::Node => {
                self.node_url = text.trim().to_string();
                let _ = paths::save_node_url(&self.node_url);
                self.status = format!("Node {}", self.node_url).into();
            }
            Field::RecvAmount => {
                self.recv_amount = text.trim().replace(',', ".");
                self.status = l10n::t("Receive amount updated.").into();
            }
            Field::RecvDesc => {
                self.recv_desc = text;
                self.status = l10n::t("Receive description updated.").into();
            }
            Field::RecvLabel => {
                self.recv_label = text;
                self.persist_recv_label();
                self.status = l10n::t("Receive label updated.").into();
            }
            Field::Challenge => {
                let slot = self.challenge_slot.min(2);
                self.challenge_answers[slot] = text.trim().to_string();
                self.status = l10n::t("Seed confirmation updated.").into();
            }
            Field::I2pNode => {
                self.i2p_rpc = text.trim().to_string();
                let _ = paths::save_i2p_rpc(&self.i2p_rpc);
                self.status = l10n::t("I2P node updated.").into();
            }
            Field::I2pProxy => {
                self.i2p_proxy = text.trim().to_string();
                let _ = paths::save_i2p_proxy(&self.i2p_proxy);
                self.status = l10n::t("I2P HTTP proxy updated.").into();
            }
        }
        cx.notify();
    }

    fn copy_field(&mut self, _: &CopyField, _window: &mut Window, cx: &mut Context<Self>) {
        if self.opened
            && !matches!(
                self.screen,
                Screen::Send | Screen::Settings | Screen::Receive
            )
        {
            self.copy_address(cx);
            return;
        }
        let text = match self.active {
            Field::Seed => self.seed.clone(),
            Field::Height => self.restore_height_text.clone(),
            Field::Dest => self.send_dest.clone(),
            Field::Amount => self.send_amount.clone(),
            Field::Node => self.node_url.clone(),
            Field::RecvAmount => self.recv_amount.clone(),
            Field::RecvDesc => self.recv_desc.clone(),
            Field::RecvLabel => self.recv_label.clone(),
            Field::Challenge => self.challenge_answers[self.challenge_slot.min(2)].clone(),
            Field::I2pNode => self.i2p_rpc.clone(),
            Field::I2pProxy => self.i2p_proxy.clone(),
        };
        if text.is_empty() && self.opened {
            self.copy_address(cx);
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        cx.notify();
    }

    fn copy_address_action(
        &mut self,
        _: &CopyAddress,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_address(cx);
    }

    fn copy_address(&mut self, cx: &mut Context<Self>) {
        if !self.opened {
            self.status = l10n::t("Open a wallet first.").into();
            cx.notify();
            return;
        }
        let address = if self.screen == Screen::Receive {
            self.receive_address.to_string()
        } else {
            self.address.to_string()
        };
        if address.is_empty() {
            self.status = l10n::t("No address to copy.").into();
            cx.notify();
            return;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(address));
        self.address_copied_at = Some(Instant::now());
        self.status = l10n::t("Address copied.").into();
        cx.notify();
    }

    fn clear_send_preview(&mut self) {
        self.send_fee = None;
        self.send_preview_amount = None;
    }

    fn show_wallet(&mut self, _: &ShowWallet, _window: &mut Window, cx: &mut Context<Self>) {
        if self.opened {
            self.screen = Screen::Wallet;
            cx.notify();
        }
    }

    fn show_receive(&mut self, _: &ShowReceive, _window: &mut Window, cx: &mut Context<Self>) {
        if self.opened {
            self.screen = Screen::Receive;
            self.active = Field::RecvAmount;
            self.sync_recv_label_from_book();
            self.ensure_qr();
            self.status = l10n::t("Receive · QR encodes a monero: payment URI.").into();
            cx.notify();
        }
    }

    fn show_send(&mut self, _: &ShowSend, _window: &mut Window, cx: &mut Context<Self>) {
        if self.opened {
            self.screen = Screen::Send;
            self.active = Field::Dest;
            self.status =
                l10n::t("Send · paste destination, amount or Send max, then Preview.").into();
            self.refresh_send_source_balance();
            cx.notify();
        }
    }

    fn go_receive(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Receive;
        self.active = Field::RecvAmount;
        self.sync_recv_label_from_book();
        self.ensure_qr();
        self.status = l10n::t("Receive · QR encodes a monero: payment URI.").into();
        cx.notify();
    }

    fn go_send(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Send;
        self.active = Field::Dest;
        self.status = l10n::t("Send · paste destination, amount or Send max, then Preview.").into();
        self.refresh_send_source_balance();
        cx.notify();
    }

    fn toggle_sync_details(&mut self, cx: &mut Context<Self>) {
        self.scan_details_expanded = !self.scan_details_expanded;
        let _ = paths::save_sync_details_expanded(self.scan_details_expanded);
        cx.notify();
    }

    fn go_wallet(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Wallet;
        cx.notify();
    }

    fn show_settings(&mut self, _: &ShowSettings, _window: &mut Window, cx: &mut Context<Self>) {
        self.go_settings(cx);
    }

    fn blocked_by_terms(&self) -> bool {
        paths::terms_need_accept()
    }

    fn go_settings(&mut self, cx: &mut Context<Self>) {
        if self.blocked_by_terms() {
            self.screen = Screen::Terms;
            self.status = l10n::t("Accept the Terms of Use first.").into();
            cx.notify();
            return;
        }
        self.screen = Screen::Settings;
        self.active = Field::Node;
        self.status = l10n::t("Settings · node URL is saved locally.").into();
        cx.notify();
    }

    fn accept_terms(&mut self, cx: &mut Context<Self>) {
        if !self.terms_checked {
            self.status = l10n::t("Check the box to agree to the Terms of Use.").into();
            cx.notify();
            return;
        }
        if let Err(err) = paths::accept_terms() {
            self.status = format!("Could not save terms acceptance: {err}").into();
            cx.notify();
            return;
        }
        self.screen = Screen::Restore;
        self.status = l10n::t("Terms accepted. Paste your seed, then Open & sync.").into();
        cx.notify();
        self.try_unlock_stored(cx);
    }

    fn open_legal(&mut self, doc: legal::Document, cx: &mut Context<Self>) {
        self.legal_return = self.screen;
        self.legal_doc = doc;
        self.screen = Screen::Legal;
        cx.notify();
    }

    fn close_legal(&mut self, cx: &mut Context<Self>) {
        self.screen = self.legal_return;
        cx.notify();
    }

    fn fill_send_max(&mut self, cx: &mut Context<Self>) {
        if self.send_busy {
            return;
        }
        self.send_max = true;
        self.send_amount_mode = AmountMode::Xmr;
        self.send_amount = amount::format_for_input(self.send_unlocked());
        self.clear_send_preview();
        self.status = l10n::t("Send max · Preview to see the sweep amount and fee.").into();
        cx.notify();
    }

    fn run_preview(&mut self, cx: &mut Context<Self>) {
        if self.send_busy {
            return;
        }
        let dest = self.send_dest.trim().to_string();
        if !uri::looks_like_address(&dest) {
            self.status = l10n::t("Enter a valid mainnet address (or paste a monero: URI).").into();
            cx.notify();
            return;
        }
        let is_max = self.send_max;
        let amount = if is_max {
            None
        } else {
            match self.send_piconero() {
                Some(v) if v > 0 => Some(v),
                _ => {
                    self.status = l10n::t("Enter a valid amount, or Send max.").into();
                    cx.notify();
                    return;
                }
            }
        };
        if let Some(amt) = amount {
            if amt > self.send_unlocked() {
                self.status = l10n::t("Amount is larger than unlocked balance.").into();
                cx.notify();
                return;
            }
        }
        self.send_busy = true;
        self.status = l10n::t("Estimating fee…").into();
        cx.notify();
        self.apply_broadcast_proxy();
        let node = self.broadcast_node_url();
        let from = self.send_from_minor();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if is_max {
                        api::preview_sweep_filtered(WALLET_ID, &node, &dest, from)
                            .map(|p| (p.amount, p.fee, true))
                    } else {
                        let amt = amount.unwrap();
                        api::preview_fee_filtered(WALLET_ID, &node, &dest, amt, from)
                            .map(|fee| (amt, fee, false))
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.send_busy = false;
                this.apply_scan_proxy();
                match result {
                    Ok((amt, fee, was_max)) => {
                        this.send_preview_amount = Some(amt);
                        this.send_fee = Some(fee);
                        this.send_max = was_max;
                        if was_max {
                            this.send_amount_mode = AmountMode::Xmr;
                            this.send_amount = amount::format_for_input(amt);
                        }
                        this.status = format!(
                            "Fee {} · amount {}. Press Send to broadcast.",
                            amount::format_xmr(fee),
                            amount::format_xmr(amt)
                        )
                        .into();
                    }
                    Err(err) => {
                        this.clear_send_preview();
                        this.status = format!("Fee preview failed: {err}").into();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn run_send(&mut self, cx: &mut Context<Self>) {
        if self.send_busy {
            return;
        }
        let dest = self.send_dest.trim().to_string();
        if !uri::looks_like_address(&dest) {
            self.status = l10n::t("Enter a valid mainnet address.").into();
            cx.notify();
            return;
        }
        let Some(fee) = self.send_fee else {
            self.status = l10n::t("Preview the fee before sending.").into();
            cx.notify();
            return;
        };
        let is_max = self.send_max;
        let amount = if is_max {
            self.send_preview_amount.unwrap_or(0)
        } else {
            match self.send_piconero() {
                Some(v) if v > 0 => v,
                _ => {
                    self.status = l10n::t("Enter a valid amount.").into();
                    cx.notify();
                    return;
                }
            }
        };
        if !is_max && !api::has_unlocked_for_exact_send(amount, fee, self.send_unlocked()) {
            self.status = l10n::t("Unlocked balance cannot cover amount + fee.").into();
            cx.notify();
            return;
        }
        if !self.authenticate_if_required("Authenticate to send Monero", cx) {
            return;
        }
        self.send_busy = true;
        self.status = l10n::t("Sending…").into();
        cx.notify();
        self.apply_broadcast_proxy();
        let node = self.broadcast_node_url();
        let from = self.send_from_minor();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if is_max {
                        send_flow::send_max(&node, &dest, from)
                    } else {
                        send_flow::send_exact(&node, &dest, amount, from)
                            .map(|r| (r.txid, amount, r.fee))
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.send_busy = false;
                this.apply_scan_proxy();
                match result {
                    Ok((txid, amt, fee)) => {
                        this.clear_send_preview();
                        this.send_max = false;
                        this.send_amount_mode = AmountMode::Xmr;
                        this.screen = Screen::Wallet;
                        let rate = this.live_rate().cloned();
                        this.fiat_snapshots.record_send(&txid, rate.as_ref());
                        this.status = format!(
                            "Sent {} (fee {}) · {}",
                            amount::format_xmr(amt),
                            amount::format_xmr(fee),
                            txid.chars().take(12).collect::<String>()
                        )
                        .into();
                        this.apply_scan_proxy();
                        let _ = this.start_fast_scan();
                    }
                    Err(err) => {
                        this.status = format!("Send failed: {err}").into();
                    }
                }
                this.poll_core();
                cx.notify();
            });
        })
        .detach();
    }

    fn address_copy_hint_active(&self) -> bool {
        self.address_copied_at
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
    }

    fn cut_field(&mut self, _: &CutField, window: &mut Window, cx: &mut Context<Self>) {
        if self.opened
            && self.screen != Screen::Send
            && self.screen != Screen::Settings
            && self.screen != Screen::Receive
        {
            return;
        }
        self.copy_field(&CopyField, window, cx);
        self.active_text_mut().clear();
        if self.active == Field::Height {
            self.restore_height_text = "0".into();
        }
        if self.active == Field::Dest || self.active == Field::Amount {
            self.send_max = false;
            self.clear_send_preview();
        }
        if self.active == Field::RecvLabel {
            self.persist_recv_label();
        }
        if self.active == Field::I2pNode {
            let _ = paths::save_i2p_rpc(&self.i2p_rpc);
        }
        if self.active == Field::I2pProxy {
            let _ = paths::save_i2p_proxy(&self.i2p_proxy);
        }
        cx.notify();
    }

    fn select_all_field(
        &mut self,
        _: &SelectAllField,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Fields replace on paste; select-all is a no-op besides focusing.
        cx.notify();
    }

    fn backspace_field(
        &mut self,
        _: &BackspaceField,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.opened
            && self.screen != Screen::Send
            && self.screen != Screen::Settings
            && self.screen != Screen::Receive
        {
            return;
        }
        match self.active {
            Field::Seed => {
                self.seed.pop();
            }
            Field::Height => {
                self.restore_height_text.pop();
                if self.restore_height_text.is_empty() {
                    self.restore_height_text = "0".into();
                }
            }
            Field::Dest => {
                self.send_dest.pop();
                self.clear_send_preview();
            }
            Field::Amount => {
                self.send_amount.pop();
                self.send_max = false;
                self.clear_send_preview();
            }
            Field::Node => {
                self.node_url.pop();
                let _ = paths::save_node_url(&self.node_url);
            }
            Field::RecvAmount => {
                self.recv_amount.pop();
            }
            Field::RecvDesc => {
                self.recv_desc.pop();
            }
            Field::RecvLabel => {
                self.recv_label.pop();
                self.persist_recv_label();
            }
            Field::Challenge => {
                let slot = self.challenge_slot.min(2);
                self.challenge_answers[slot].pop();
            }
            Field::I2pNode => {
                self.i2p_rpc.pop();
                let _ = paths::save_i2p_rpc(&self.i2p_rpc);
            }
            Field::I2pProxy => {
                self.i2p_proxy.pop();
                let _ = paths::save_i2p_proxy(&self.i2p_proxy);
            }
        }
        cx.notify();
    }

    fn insert_typed(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if matches!(self.screen, Screen::Terms | Screen::Legal) {
            return;
        }
        if self.opened
            && self.screen != Screen::Send
            && self.screen != Screen::Settings
            && self.screen != Screen::Receive
        {
            return;
        }
        if event.keystroke.modifiers.platform || event.keystroke.modifiers.control {
            return;
        }
        let Some(ch) = event.keystroke.key_char.as_deref() else {
            return;
        };
        if ch.is_empty() || ch == "\u{7f}" || ch == "\u{8}" {
            return;
        }
        match self.active {
            Field::Seed => self.seed.push_str(ch),
            Field::Height => {
                let digits: String = ch.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.is_empty() {
                    return;
                }
                if self.restore_height_text == "0" {
                    self.restore_height_text = digits;
                } else {
                    self.restore_height_text.push_str(&digits);
                }
            }
            Field::Dest => {
                self.send_dest.push_str(ch.trim());
                self.clear_send_preview();
            }
            Field::Amount => {
                let filtered: String = ch
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
                    .collect();
                if filtered.is_empty() {
                    return;
                }
                self.send_amount.push_str(&filtered.replace(',', "."));
                self.send_max = false;
                self.clear_send_preview();
            }
            Field::Node => {
                self.node_url.push_str(ch.trim());
                let _ = paths::save_node_url(&self.node_url);
            }
            Field::RecvAmount => {
                let filtered: String = ch
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
                    .collect();
                if filtered.is_empty() {
                    return;
                }
                self.recv_amount.push_str(&filtered.replace(',', "."));
            }
            Field::RecvDesc => self.recv_desc.push_str(ch),
            Field::RecvLabel => {
                self.recv_label.push_str(ch);
                self.persist_recv_label();
            }
            Field::Challenge => {
                let filtered: String = ch
                    .chars()
                    .filter(|c| c.is_ascii_alphabetic() || *c == '\'')
                    .collect();
                if filtered.is_empty() {
                    return;
                }
                let slot = self.challenge_slot.min(2);
                self.challenge_answers[slot].push_str(&filtered);
            }
            Field::I2pNode => {
                self.i2p_rpc.push_str(ch.trim());
                let _ = paths::save_i2p_rpc(&self.i2p_rpc);
            }
            Field::I2pProxy => {
                self.i2p_proxy.push_str(ch.trim());
                let _ = paths::save_i2p_proxy(&self.i2p_proxy);
            }
        }
        cx.notify();
    }

    fn create_seed(&mut self, cx: &mut Context<Self>) {
        match api::generate_mnemonic_english() {
            Ok(mnemonic) => {
                self.seed = normalize_seed(&mnemonic);
                self.created_seed = true;
                self.wrote_seed_down = false;
                self.challenge_indices = seed_backup::pick_indices(self.word_count());
                self.challenge_answers = Default::default();
                self.challenge_slot = 0;
                self.fetch_suggested_height(cx);
                self.status = l10n::t(
                    "Write this seed down on paper. Open is locked until you confirm three words.",
                )
                .into();
            }
            Err(err) => self.status = format!("Could not generate seed: {err}").into(),
        }
        cx.notify();
    }

    fn open_wallet(&mut self, _: &OpenWallet, _window: &mut Window, cx: &mut Context<Self>) {
        if self.has_stored && !self.show_restore_form {
            self.try_unlock_stored(cx);
            return;
        }
        self.open_from_form(cx);
    }

    fn open_from_form(&mut self, cx: &mut Context<Self>) {
        if self.blocked_by_terms() {
            self.screen = Screen::Terms;
            self.status = l10n::t("Accept the Terms of Use first.").into();
            cx.notify();
            return;
        }
        let mnemonic = normalize_seed(&self.seed);
        if mnemonic.is_empty() {
            self.status =
                l10n::t("Seed field is empty. Paste or type the 25-word phrase first.").into();
            cx.notify();
            return;
        }
        if self.created_seed && !self.seed_backup_passed() {
            self.status = l10n::t(
                "Check that you wrote the seed down and confirm the three words before opening.",
            )
            .into();
            cx.notify();
            return;
        }
        self.open_with_mnemonic(&mnemonic, self.restore_height(), cx);
    }

    fn open_with_mnemonic(&mut self, mnemonic: &str, restore_height: u64, cx: &mut Context<Self>) {
        if let Err(err) = api::open_from_mnemonic(WALLET_ID, mnemonic, restore_height, true) {
            self.status = format!("Open failed: {err}").into();
            cx.notify();
            return;
        }
        let _ = api::set_gap_limit(WALLET_ID, GAP_LIMIT);

        match api::primary_address_from_mnemonic(mnemonic, true) {
            Ok(address) => self.address = address.into(),
            Err(err) => {
                self.status = format!("Address failed: {err}").into();
                cx.notify();
                return;
            }
        }

        if let Some(cache) = paths::load_cache() {
            match api::import_cache(WALLET_ID, &cache) {
                Ok(()) => self.status = l10n::t("Opened with local cache. Syncing…").into(),
                Err(err) => {
                    self.status =
                        format!("Cache skipped ({err}). Syncing from restore height…").into()
                }
            }
        } else {
            self.status = l10n::t("Opened. Syncing…").into();
        }

        self.last_exported_scanned = None;
        self.last_cache_persist_at = None;
        self.last_balance_poll_at = None;
        self.last_transfers_poll_at = None;
        self.scan_was_running = false;
        self.scan_rate.reset();
        self.apply_scan_proxy();
        let scan_error = self.start_fast_scan().err().map(|err| err.to_string());

        self.opened = true;
        self.screen = Screen::Wallet;
        self.scan_needs_retry = scan_error.is_some();
        self.mnemonic = mnemonic.to_string();
        self.created_seed = false;
        self.wrote_seed_down = false;
        self.challenge_indices.clear();
        self.challenge_answers = Default::default();
        self.receive_book = receive_book::load();
        receive_book::save(&self.receive_book);
        self.refresh_receive_address();
        let store_error = match wallet_store::save(mnemonic, restore_height) {
            Ok(()) => {
                self.has_stored = true;
                self.show_restore_form = false;
                self.seed.clear();
                if paths::load_device_auth_preference().is_none() && device_auth::is_available() {
                    self.require_device_auth = true;
                    let _ = paths::save_device_auth(true);
                }
                None
            }
            Err(err) => Some(err),
        };
        if let Some(err) = scan_error {
            self.status =
                format!("Wallet opened, but refresh failed: {err}. Use Wallet → Retry sync.")
                    .into();
        }
        if let Some(err) = store_error {
            self.status = format!("{} Secure-store save skipped: {err}", self.status).into();
        }
        self.poll_core();
        self.start_poll(cx);
        self.maybe_refresh_fiat(cx);
        self.recover_pending_send(cx);
        cx.notify();
    }

    fn recover_pending_send(&mut self, cx: &mut Context<Self>) {
        if paths::load_pending_send().is_none() {
            return;
        }
        self.apply_broadcast_proxy();
        let node = self.broadcast_node_url();
        self.status = l10n::t("Relaying a pending send…").into();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { send_flow::recover_pending(&node) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Some(recovered)) => {
                        let rate = this.live_rate().cloned();
                        this.fiat_snapshots
                            .record_send(&recovered.txid, rate.as_ref());
                        this.status = format!(
                            "Relayed a pending send · {} · {}",
                            amount::format_xmr(recovered.amount),
                            recovered.txid.chars().take(12).collect::<String>()
                        )
                        .into();
                    }
                    Ok(None) => {}
                    Err(err) => {
                        this.status =
                            format!("Pending send still on disk ({err}). Retry send later.").into();
                    }
                }
                this.apply_scan_proxy();
                this.poll_core();
                cx.notify();
            });
        })
        .detach();
    }

    fn retry_sync(&mut self, _: &RetrySync, _window: &mut Window, cx: &mut Context<Self>) {
        self.retry_refresh(cx);
    }

    fn retry_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.opened {
            self.status = l10n::t("Open the wallet before retrying sync.").into();
            cx.notify();
            return;
        }
        if matches!(api::refresh_job(WALLET_ID), RefreshJob::Running) {
            self.status = l10n::t("Scan is already running.").into();
            cx.notify();
            return;
        }
        self.apply_scan_proxy();
        if let Err(err) = self.start_fast_scan() {
            self.status = format!("Retry failed: {err}").into();
            self.scan_needs_retry = true;
            cx.notify();
            return;
        }
        self.scan_needs_retry = false;
        self.status = l10n::t("Retrying sync…").into();
        self.start_poll(cx);
        cx.notify();
    }

    fn run_scan_benchmark(&mut self, cx: &mut Context<Self>) {
        if !self.opened || self.mnemonic.is_empty() {
            self.status = l10n::t("Open the wallet before running a scan benchmark.").into();
            cx.notify();
            return;
        }
        if self.benchmark_running {
            self.status = l10n::t("Scan benchmark is already running.").into();
            cx.notify();
            return;
        }

        let baseline_start_height = api::sync_status(WALLET_ID)
            .ok()
            .map(|sync| sync.last_scanned.saturating_sub(10_000))
            .unwrap_or_default();
        let start_height = std::env::var("NEXAWAL_BENCHMARK_START_HEIGHT")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(baseline_start_height);
        let node = self.scan_node_url();
        let mnemonic = self.mnemonic.clone();
        let run_id = benchmark::run_id();

        let _ = api::refresh_cancel(WALLET_ID);
        self.benchmark_running = true;
        self.benchmark_status = None;
        self.scan_needs_retry = false;
        self.status = format!(
            "Stopping the current sync · benchmarking from block {} on {}…",
            start_height, node
        )
        .into();
        cx.notify();

        cx.spawn(async move |this, cx| {
            let report = cx
                .background_executor()
                .spawn(async move { benchmark::run(node, mnemonic, start_height, run_id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.benchmark_running = false;
                let status = format!(
                    "Benchmark complete · {}. Results saved to {} · RPC trace {}",
                    report.summary, report.results_path, report.rpc_results_path
                );
                this.benchmark_status = Some(status.clone().into());
                this.status = status.into();
                this.poll_core();
                cx.notify();
            });
        })
        .detach();
    }

    fn forget(&mut self, cx: &mut Context<Self>) {
        let _ = api::refresh_cancel(WALLET_ID);
        let _ = api::reset_tracked_outputs(WALLET_ID);
        self.opened = false;
        self.unlocking = false;
        self.unlock_started = false;
        self.screen = Screen::Restore;
        self.address = "".into();
        self.total_piconero = 0;
        self.unlocked_piconero = 0;
        self.sync = None;
        self.transfers.clear();
        self.last_exported_scanned = None;
        self.last_cache_persist_at = None;
        self.last_balance_poll_at = None;
        self.last_transfers_poll_at = None;
        self.scan_needs_retry = false;
        self.scan_was_running = false;
        self.last_scan_error = None;
        self.scan_rate.reset();
        self.last_scan_progress_at = None;
        self.last_scanned_for_stall = 0;
        self.address_copied_at = None;
        self.send_dest.clear();
        self.send_amount.clear();
        self.send_max = false;
        self.send_busy = false;
        self.send_from_subaddress = false;
        self.send_source_index = 0;
        self.send_source_unlocked = None;
        self.send_qr_busy = false;
        self.clear_send_preview();
        self.recv_amount.clear();
        self.recv_desc.clear();
        self.recv_label.clear();
        self.qr_uri.clear();
        self.qr_image = None;
        self.seed.clear();
        self.mnemonic.clear();
        self.created_seed = false;
        self.wrote_seed_down = false;
        self.challenge_indices.clear();
        self.challenge_answers = Default::default();
        self.challenge_slot = 0;
        self.receive_address = "".into();
        self.has_stored = wallet_store::is_marked_stored();
        self.show_restore_form = !self.has_stored;
        self.active = Field::Seed;
        self.status = if self.has_stored {
            l10n::t("Locked. Open the existing wallet from {}, or restore a different seed.")
                .replace("{}", wallet_store::secure_store_name())
                .into()
        } else {
            l10n::t("Session cleared.").into()
        };
        cx.notify();
    }

    fn remove_stored_wallet(&mut self, cx: &mut Context<Self>) {
        self.forget(cx);
        if let Err(err) = wallet_store::delete() {
            self.has_stored = wallet_store::is_marked_stored();
            self.show_restore_form = !self.has_stored;
            self.status = err.into();
            cx.notify();
            return;
        }
        let _ = fs::remove_file(paths::cache_path());
        paths::clear_pending_send();
        receive_book::clear();
        self.receive_book = receive_book::Book::primary();
        self.has_stored = false;
        self.show_restore_form = true;
        self.restore_height_text = "0".into();
        self.status = l10n::t("Removed the stored wallet from this computer.").into();
        cx.notify();
    }

    fn try_unlock_stored(&mut self, cx: &mut Context<Self>) {
        if self.blocked_by_terms() || self.opened || self.unlock_started {
            return;
        }
        if !env_or("NEXAWAL_MNEMONIC", "").is_empty() {
            self.unlocking = false;
            return;
        }
        if !wallet_store::is_marked_stored() {
            self.unlocking = false;
            return;
        }
        if self.require_device_auth && !device_auth::is_available() {
            self.unlocking = false;
            self.status = l10n::t(
                "Device authentication is required but unavailable. Enable biometrics or a screen lock, then retry.",
            )
            .into();
            cx.notify();
            return;
        }

        let require_device_auth = self.require_device_auth;
        self.unlocking = true;
        self.unlock_started = true;
        self.status = if require_device_auth {
            l10n::t("Unlocking Wallet...").into()
        } else {
            l10n::t("Unlocking Wallet...").into()
        };
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if require_device_auth {
                        let reason = l10n::t("Authenticate to unlock nexawal");
                        device_auth::authenticate(&reason)?;
                    }
                    wallet_store::load()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.unlocking = false;
                this.unlock_started = false;
                match result {
                    Ok((mnemonic, height)) => {
                        this.restore_height_text = height.to_string();
                        this.show_restore_form = false;
                        this.open_with_mnemonic(&mnemonic, height, cx);
                        this.seed.clear();
                    }
                    Err(err) => {
                        this.has_stored = true;
                        this.show_restore_form = false;
                        this.status = format!(
                            "{err}. {}",
                            l10n::t("Use Open existing wallet to try again, or restore from seed.")
                        )
                        .into();
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn receive_uri(&self) -> String {
        let amount = self
            .recv_piconero()
            .filter(|v| *v > 0)
            .map(amount::format_for_input);
        let desc = self.recv_desc.trim();
        uri::build(
            &self.receive_address,
            amount.as_deref(),
            if desc.is_empty() { None } else { Some(desc) },
        )
    }

    fn ensure_qr(&mut self) {
        let uri = self.receive_uri();
        if self.qr_uri == uri && self.qr_image.is_some() {
            return;
        }
        self.qr_uri = uri.clone();
        self.qr_image = qr::render_image(&uri);
    }

    fn authenticate_if_required(&mut self, reason: &str, cx: &mut Context<Self>) -> bool {
        if !self.require_device_auth {
            return true;
        }
        if !device_auth::is_available() {
            self.status = l10n::t(
                "Device authentication is required but unavailable. Enable biometrics or a screen lock, then retry.",
            )
            .into();
            cx.notify();
            return false;
        }
        match device_auth::authenticate(reason) {
            Ok(()) => true,
            Err(err) => {
                self.status = err.into();
                cx.notify();
                false
            }
        }
    }

    fn refresh_receive_address(&mut self) {
        let index = self.receive_book.selected;
        let derived = if self.mnemonic.is_empty() {
            self.address.to_string()
        } else {
            api::derive_subaddress_from_mnemonic(&self.mnemonic, 0, index, true)
                .unwrap_or_else(|_| self.address.to_string())
        };
        self.receive_address = derived.into();
        self.qr_uri.clear();
        self.qr_image = None;
    }

    fn cycle_receive_address(&mut self, next: bool, cx: &mut Context<Self>) {
        if next {
            self.receive_book.select_next();
        } else {
            self.receive_book.select_prev();
        }
        receive_book::save(&self.receive_book);
        self.sync_recv_label_from_book();
        self.refresh_receive_address();
        self.ensure_qr();
        self.status = format!("Receive · {}", self.receive_book.display_label()).into();
        cx.notify();
    }

    fn create_receive_subaddress(&mut self, cx: &mut Context<Self>) {
        let index = self.receive_book.allocate_new("");
        receive_book::save(&self.receive_book);
        self.recv_label.clear();
        self.refresh_receive_address();
        self.ensure_qr();
        self.status =
            format!("New receive address · subaddress {index}. Name it if you want.").into();
        cx.notify();
    }

    fn toggle_device_auth(&mut self, cx: &mut Context<Self>) {
        if self.require_device_auth {
            if !self.authenticate_if_required("Authenticate to turn off device authentication", cx)
            {
                return;
            }
            self.require_device_auth = false;
            let _ = paths::save_device_auth(false);
            self.status = l10n::t("Device authentication off.").into();
        } else {
            if !device_auth::is_available() {
                self.status =
                    l10n::t("Touch ID or a login password is not available on this computer.")
                        .into();
                cx.notify();
                return;
            }
            if let Err(err) =
                device_auth::authenticate("Authenticate to require device authentication")
            {
                self.status = err.into();
                cx.notify();
                return;
            }
            self.require_device_auth = true;
            let _ = paths::save_device_auth(true);
            self.status = l10n::t("Device authentication on · required to unlock and send.").into();
        }
        cx.notify();
    }

    fn copy_receive_uri(&mut self, cx: &mut Context<Self>) {
        let uri = self.receive_uri();
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(uri));
        self.address_copied_at = Some(Instant::now());
        self.status = l10n::t("Payment URI copied.").into();
        cx.notify();
    }

    fn toggle_fiat(&mut self, cx: &mut Context<Self>) {
        self.fiat_enabled = !self.fiat_enabled;
        let _ = paths::save_fiat_enabled(self.fiat_enabled);
        if self.fiat_enabled {
            let _ = paths::ensure_fiat_opted_in_at();
            self.maybe_refresh_fiat(cx);
            self.status = format!(
                "Fiat estimates on · {}. Kraken / Frankfurter will see your IP.",
                self.fiat_currency
            )
            .into();
        } else {
            self.fiat_rate = None;
            paths::clear_fiat_rate();
            self.coerce_amount_modes();
            self.status = l10n::t("Fiat estimates off.").into();
        }
        cx.notify();
    }

    fn cycle_fiat_currency(&mut self, next: bool, cx: &mut Context<Self>) {
        self.fiat_currency = if next {
            fiat::next_currency(&self.fiat_currency)
        } else {
            fiat::prev_currency(&self.fiat_currency)
        }
        .to_string();
        let _ = paths::save_fiat_currency(&self.fiat_currency);
        self.fiat_rate = None;
        if self.fiat_enabled {
            self.maybe_refresh_fiat(cx);
        }
        self.status = format!("Fiat currency {}", self.fiat_currency).into();
        cx.notify();
    }

    fn maybe_refresh_fiat(&mut self, cx: &mut Context<Self>) {
        if !self.fiat_enabled || self.fiat_fetching {
            return;
        }
        let now = fiat::now_ms();
        if let Some(rate) = &self.fiat_rate {
            if rate.currency == self.fiat_currency && fiat::is_fresh(rate.fetched_at_ms, now) {
                if now.saturating_sub(rate.fetched_at_ms) < fiat::REFRESH_INTERVAL_MS {
                    return;
                }
            }
        }
        self.fiat_fetching = true;
        let currency = self.fiat_currency.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { fiat::fetch_rate(&currency) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.fiat_fetching = false;
                if !this.fiat_enabled {
                    this.fiat_rate = None;
                    cx.notify();
                    return;
                }
                match result {
                    Ok(rate) => {
                        if rate.currency == this.fiat_currency {
                            let _ = paths::save_fiat_rate(&rate);
                            this.fiat_rate = Some(rate);
                        }
                    }
                    Err(_) => {
                        this.fiat_rate =
                            paths::load_fiat_rate().filter(|r| r.currency == this.fiat_currency);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn fiat_line(&self, piconero: u64) -> Option<String> {
        if !self.fiat_enabled {
            return None;
        }
        fiat::live_approx(piconero, self.fiat_rate.as_ref())
    }

    fn start_poll(&mut self, cx: &mut Context<Self>) {
        if self.polling {
            return;
        }
        self.polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.opened {
                            this.poll_core();
                            this.maybe_refresh_fiat(cx);
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    fn poll_core(&mut self) {
        if self.benchmark_running {
            return;
        }
        if let Some(status) = self.benchmark_status.clone() {
            self.status = status;
            return;
        }
        match api::sync_status(WALLET_ID) {
            Ok(sync) => {
                let now = Instant::now();
                let remaining = sync.chain_height.saturating_sub(sync.last_scanned);
                let job = api::refresh_job(WALLET_ID);
                let running = matches!(job, RefreshJob::Running);
                let was_running = self.scan_was_running;
                let scan_started = running && !was_running;
                if scan_started {
                    self.scan_rate.reset();
                    self.last_balance_poll_at = None;
                    self.last_transfers_poll_at = None;
                    // Treat the current state as the checkpoint baseline. During an active
                    // scan, checkpoint no more often than Catalyst does.
                    self.last_exported_scanned = Some(sync.last_scanned);
                    self.last_cache_persist_at = Some(now);
                }
                self.scan_was_running = running;
                self.scan_rate.note(sync.last_scanned, running);
                match job {
                    RefreshJob::Failed(err) => {
                        self.scan_needs_retry = true;
                        self.last_scan_error = Some(err.clone());
                        self.status = format!(
                            "Scan stopped at {} / {}: {}. Wallet → Retry sync.",
                            sync.last_scanned, sync.chain_height, err
                        )
                        .into();
                    }
                    RefreshJob::Running => {
                        self.scan_needs_retry = false;
                        self.last_scan_error = None;
                        if sync.last_scanned > self.last_scanned_for_stall {
                            self.last_scanned_for_stall = sync.last_scanned;
                            self.last_scan_progress_at = Some(Instant::now());
                        }
                        let remaining = sync.chain_height.saturating_sub(sync.last_scanned);
                        let stalled = self.last_scan_progress_at.is_some_and(|t| {
                            t.elapsed() >= Duration::from_secs(scan_tuning::STALL_SECS)
                        });
                        let recoverable = api::last_error()
                            .as_deref()
                            .is_some_and(scan_tuning::is_recoverable_fetch_error)
                            && self
                                .last_scan_progress_at
                                .is_some_and(|t| t.elapsed() >= Duration::from_secs(2));
                        if remaining > 3 && (stalled || recoverable) {
                            self.persist_scan_cache();
                            let _ = api::refresh_cancel(WALLET_ID);
                            self.scan_needs_retry = true;
                            self.last_scan_error = Some(if recoverable {
                                api::last_error().unwrap_or_else(|| {
                                    "Recoverable RPC failure; retry without changing the scan profile.".to_string()
                                })
                            } else {
                                format!(
                                    "Sync stalled: no scan progress for over {}s (lastScanned={}, target={})",
                                    scan_tuning::STALL_SECS,
                                    sync.last_scanned,
                                    sync.chain_height
                                )
                            });
                            self.status = format!(
                                "Scan stopped at {} / {}. Wallet → Retry sync.",
                                sync.last_scanned, sync.chain_height
                            )
                            .into();
                        }
                    }
                    RefreshJob::Idle => {
                        if remaining <= 3 && sync.chain_height > 0 {
                            self.scan_needs_retry = false;
                            self.last_scan_error = None;
                        } else {
                            self.scan_needs_retry = true;
                            if self.last_scan_error.is_none() {
                                self.last_scan_error = Some(format!(
                                    "Sync stalled: no scan progress (lastScanned={}, target={})",
                                    sync.last_scanned, sync.chain_height
                                ));
                            }
                            self.status = format!(
                                "Scan paused at {} / {} ({} left). Wallet → Retry sync.",
                                sync.last_scanned, sync.chain_height, remaining
                            )
                            .into();
                        }
                    }
                }

                // Status stays responsive at 500 ms, but balance and transfer snapshots
                // contend on WalletCore's store lock and do not need that cadence while
                // the scanner is busy. Match Catalyst's 10-second active-sync cadence.
                let balance_due = !running
                    || self.last_balance_poll_at.is_none_or(|last| {
                        now.saturating_duration_since(last) >= ACTIVE_SYNC_AUX_POLL_INTERVAL
                    });
                if balance_due {
                    self.refresh_balance_snapshot();
                    self.last_balance_poll_at = Some(now);
                }
                let transfers_due = !running
                    || self.last_transfers_poll_at.is_none_or(|last| {
                        now.saturating_duration_since(last) >= ACTIVE_SYNC_AUX_POLL_INTERVAL
                    });
                if transfers_due {
                    self.refresh_transfers_snapshot();
                    self.last_transfers_poll_at = Some(now);
                }
                self.coerce_amount_modes();

                let blocks_since_checkpoint = self
                    .last_exported_scanned
                    .map_or(0, |height| sync.last_scanned.saturating_sub(height));
                let checkpoint_interval_elapsed = self.last_cache_persist_at.is_some_and(|last| {
                    now.saturating_duration_since(last) >= ACTIVE_SYNC_CACHE_INTERVAL
                });
                let periodic_checkpoint_due = running
                    && blocks_since_checkpoint >= ACTIVE_SYNC_CACHE_BLOCK_DELTA
                    && checkpoint_interval_elapsed;
                // Also catches a short refresh that starts and finishes between two UI polls.
                let final_checkpoint_due =
                    !running && self.last_exported_scanned != Some(sync.last_scanned);
                if periodic_checkpoint_due || final_checkpoint_due {
                    self.persist_scan_cache_at(sync.last_scanned);
                }
                self.sync = Some(sync);
            }
            Err(err) => self.status = format!("{err}").into(),
        }
    }
}

impl Focusable for Home {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.ui_focus.clone()
    }
}

impl Render for Home {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.screen == Screen::Receive {
            self.ensure_qr();
        }

        let body = div()
            .id("main-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .when(self.screen == Screen::Terms, |body| {
                body.child(terms_card(self, cx))
            })
            .when(self.screen == Screen::Legal, |body| {
                body.child(legal_card(self, cx))
            })
            .when(self.screen == Screen::Restore && self.unlocking, |body| {
                body.child(unlocking_card(self))
            })
            .when(self.screen == Screen::Restore && !self.unlocking, |body| {
                body.child(locked_card(self, window, cx))
            })
            .when(self.screen == Screen::Settings, |body| {
                body.child(settings_card(self, window, cx))
            })
            .when(self.opened && self.screen == Screen::Wallet, |body| {
                body.child(opened_card(self, cx)).child(sync_card(self, cx))
            })
            .when(self.opened && self.screen == Screen::Receive, |body| {
                body.child(receive_card(self, window, cx))
            })
            .when(self.opened && self.screen == Screen::Send, |body| {
                body.child(send_card(self, window, cx))
            })
            .child(status_line(self))
            .when(self.opened && self.screen == Screen::Wallet, |body| {
                body.child(history(self))
            });

        div()
            .size_full()
            .relative()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .key_context("nexawal")
            .track_focus(&self.ui_focus)
            .on_action(cx.listener(Self::paste_field))
            .on_action(cx.listener(Self::copy_field))
            .on_action(cx.listener(Self::cut_field))
            .on_action(cx.listener(Self::select_all_field))
            .on_action(cx.listener(Self::backspace_field))
            .on_action(cx.listener(Self::open_wallet))
            .on_action(cx.listener(Self::retry_sync))
            .on_action(cx.listener(Self::copy_address_action))
            .on_action(cx.listener(Self::show_receive))
            .on_action(cx.listener(Self::show_send))
            .on_action(cx.listener(Self::show_wallet))
            .on_action(cx.listener(Self::show_settings))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                this.insert_typed(event, cx);
            }))
            .child(header(self))
            .child(body)
            .child(resize_handle())
    }
}

fn header(home: &Home) -> impl IntoElement {
    div()
        .id("window-drag")
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .cursor(CursorStyle::Arrow)
        .on_mouse_down(MouseButton::Left, |_, window, _| {
            window.start_window_move();
        })
        .on_click(|event, window, _| {
            if event.click_count() == 2 {
                window.zoom_window();
            }
        })
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(ACCENT))
                .child("nexawal"),
        )
        .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
            "{} · {} · {}",
            home.core_version,
            std::env::consts::OS,
            std::env::consts::ARCH
        )))
}

fn resize_handle() -> impl IntoElement {
    div()
        .id("window-resize")
        .absolute()
        .right_0()
        .bottom_0()
        .size(px(20.))
        .cursor(CursorStyle::ResizeUpLeftDownRight)
        .on_mouse_down(MouseButton::Left, |_, window, _| {
            window.start_window_resize(ResizeEdge::BottomRight);
        })
        .child("⌟")
}

fn unlocking_card(home: &Home) -> impl IntoElement {
    div()
        .flex_1()
        .min_h(px(360.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .p_6()
        .rounded_lg()
        .bg(rgb(CARD))
        .child(div().text_3xl().text_color(rgb(ACCENT)).child("◉"))
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(l10n::t("Unlocking Wallet...")),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .child(if home.require_device_auth {
                    l10n::t("Use biometrics or device credentials for wallet access and sending.")
                } else {
                    l10n::t("Opening the wallet from secure storage.")
                }),
        )
}

fn locked_card(home: &Home, window: &Window, cx: &mut Context<Home>) -> impl IntoElement {
    let show_restore_form = !home.has_stored || home.show_restore_form;
    let seed_focused = home.seed_focus.is_focused(window);
    let height_focused = home.height_focus.is_focused(window);
    let node_focused = home.node_focus.is_focused(window);
    let seed_label = if home.seed.trim().is_empty() {
        if home.has_stored {
            l10n::t("Paste here only to restore a different wallet.").to_string()
        } else {
            l10n::t("Click this box, then ⌘V or Edit → Paste. Open is a separate button.")
                .to_string()
        }
    } else {
        home.seed.clone()
    };
    let seed_muted = home.seed.trim().is_empty();

    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .rounded_lg()
        .bg(rgb(CARD))
        .when(home.has_stored, |card| {
            card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(ACCENT))
                    .bg(rgb(FIELD))
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(l10n::t("Existing wallet")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "Your recovery seed is stored in {}. Open it with the same wallet screens, settings, history, and local scan cache.",
                                wallet_store::secure_store_name()
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(action_button(
                                "open-existing-wallet",
                                l10n::t("Unlock Existing Wallet"),
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.try_unlock_stored(cx)
                                }),
                            ))
                            .child(action_button(
                                "open-existing-wallet-settings",
                                l10n::t("Settings"),
                                cx.listener(|this, _: &ClickEvent, _, cx| this.go_settings(cx)),
                            )),
                    ),
            )
            .child(
                if home.show_restore_form {
                    div().pt_2().child(action_button(
                        "hide-restore-form",
                        l10n::t("Use stored wallet"),
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.show_restore_form = false;
                            cx.notify();
                        }),
                    ))
                } else {
                    div().pt_2().child(action_button(
                        "show-restore-form",
                        l10n::t("Restore a different wallet"),
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.show_restore_form = true;
                            cx.notify();
                        }),
                    ))
                },
            )
        })
        .when(show_restore_form, |card| {
            card
                .child(
                    div().text_xs().text_color(rgb(MUTED)).child(if home.network_policy == network::Policy::I2p {
                        l10n::t("Scan uses the I2P node from Settings")
                    } else {
                        l10n::t("Node URL")
                    }),
                )
                .when(home.network_policy != network::Policy::I2p, |card| {
                    card.child(
                        div()
                            .id("node-field")
                            .key_context("Field")
                            .track_focus(&home.node_focus)
                            .cursor(CursorStyle::IBeam)
                            .p_3()
                            .rounded_md()
                            .bg(rgb(FIELD))
                            .border_1()
                            .border_color(rgb(if node_focused { ACCENT } else { 0x2A3A2A }))
                            .text_sm()
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.focus_field(Field::Node, window, cx);
                            }))
                            .child(home.node_url.clone()),
                    )
                })
                .child(div().text_xs().text_color(rgb(MUTED)).child(l10n::t("Seed phrase")))
                .child(
                    div()
                        .id("seed-field")
                        .key_context("Field")
                        .track_focus(&home.seed_focus)
                        .cursor(CursorStyle::IBeam)
                        .min_h(px(88.))
                        .p_3()
                        .rounded_md()
                        .bg(rgb(FIELD))
                        .border_1()
                        .border_color(rgb(if seed_focused { ACCENT } else { 0x2A3A2A }))
                        .text_sm()
                        .text_color(rgb(if seed_muted { MUTED } else { TEXT }))
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.focus_field(Field::Seed, window, cx);
                        }))
                        .child(seed_label),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(format!("{} words", home.word_count())),
                )
                .when(home.created_seed, |card| {
                    card.child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(l10n::t(
                                "This is your recovery seed. Write it down on paper and store it somewhere safe. Anyone with these words can access your funds.",
                            )),
                    )
                    .child(
                        div()
                            .id("wrote-seed")
                            .cursor(CursorStyle::PointingHand)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.wrote_seed_down = !this.wrote_seed_down;
                                cx.notify();
                            }))
                            .child(format!(
                                "{}  {}",
                                if home.wrote_seed_down { "[x]" } else { "[ ]" },
                                l10n::t("I wrote down my recovery seed")
                            )),
                    )
                })
                .when(home.created_seed && home.wrote_seed_down, |card| {
                    card.child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(l10n::t(
                                "Confirm you wrote it down: enter the requested words below.",
                            )),
                    )
                    .when(!home.challenge_indices.is_empty(), |card| {
                        card.child(challenge_row(
                            home,
                            window,
                            cx,
                            0,
                            home.challenge_indices[0],
                        ))
                    })
                    .when(home.challenge_indices.len() > 1, |card| {
                        card.child(challenge_row(
                            home,
                            window,
                            cx,
                            1,
                            home.challenge_indices[1],
                        ))
                    })
                    .when(home.challenge_indices.len() > 2, |card| {
                        card.child(challenge_row(
                            home,
                            window,
                            cx,
                            2,
                            home.challenge_indices[2],
                        ))
                    })
                    .when(!home.seed_backup_passed(), |card| {
                        card.child(
                            div()
                                .text_xs()
                                .text_color(rgb(OUT))
                                .child(l10n::t("Word(s) don't match yet.")),
                        )
                    })
                })
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(MUTED))
                        .child(if home.created_seed && home.height_fetching {
                            l10n::t("Fetching a fast restore height…")
                        } else if home.created_seed {
                            l10n::t("Starting height (fast)")
                        } else {
                            l10n::t("Restore height (0 = scan from genesis)")
                        }),
                )
                .child(
                    div()
                        .id("height-field")
                        .key_context("Field")
                        .track_focus(&home.height_focus)
                        .cursor(CursorStyle::IBeam)
                        .h(px(36.))
                        .px_3()
                        .rounded_md()
                        .bg(rgb(FIELD))
                        .border_1()
                        .border_color(rgb(if height_focused { ACCENT } else { 0x2A3A2A }))
                        .flex()
                        .items_center()
                        .text_sm()
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.focus_field(Field::Height, window, cx);
                        }))
                        .child(home.restore_height_text.clone()),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(action_button(
                            "paste-seed",
                            l10n::t("Paste clipboard into seed"),
                            cx.listener(|this, _: &ClickEvent, _, cx| this.paste_seed_button(cx)),
                        ))
                        .child(action_button(
                            "create-seed",
                            l10n::t("Create seed"),
                            cx.listener(|this, _: &ClickEvent, _, cx| this.create_seed(cx)),
                        ))
                        .when(home.created_seed, |row| {
                            row.child(action_button(
                                "copy-seed",
                                l10n::t("Copy seed"),
                                cx.listener(|this, _: &ClickEvent, _, cx| this.copy_seed(cx)),
                            ))
                        })
                        .when(home.seed_backup_passed(), |row| {
                            row.child(action_button(
                                "open-wallet",
                                l10n::t("Open & sync"),
                                cx.listener(|this, _: &ClickEvent, _, cx| this.open_from_form(cx)),
                            ))
                        })
                        .when(!home.seed_backup_passed(), |row| {
                            row.child(
                                div()
                                    .id("open-wallet-locked")
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(rgb(0x2A332A))
                                    .text_color(rgb(MUTED))
                                    .child(l10n::t("Open & sync")),
                            )
                        })
                        .child(action_button(
                            "restore-settings",
                            l10n::t("Settings"),
                            cx.listener(|this: &mut Home, _: &ClickEvent, _, cx| this.go_settings(cx)),
                        )),
                )
        })
}

fn challenge_row(
    home: &Home,
    window: &Window,
    cx: &mut Context<Home>,
    slot: usize,
    word_index: usize,
) -> impl IntoElement {
    let focused = home.active == Field::Challenge
        && home.challenge_slot == slot
        && home.challenge_focus.is_focused(window);
    let answer = home
        .challenge_answers
        .get(slot)
        .cloned()
        .unwrap_or_default();
    let muted = answer.trim().is_empty();
    let shown = if muted { String::new() } else { answer };
    div()
        .flex()
        .flex_row()
        .gap_2()
        .items_center()
        .child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .w(px(80.))
                .child(format!("Word #{}", word_index + 1)),
        )
        .child(
            div()
                .id(SharedString::from(format!("challenge-{slot}")))
                .key_context("Field")
                .when(home.challenge_slot == slot, |field| {
                    field.track_focus(&home.challenge_focus)
                })
                .cursor(CursorStyle::IBeam)
                .flex_1()
                .p_3()
                .rounded_md()
                .bg(rgb(FIELD))
                .border_1()
                .border_color(rgb(if focused { ACCENT } else { 0x2A3A2A }))
                .text_sm()
                .text_color(rgb(if muted { MUTED } else { TEXT }))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.challenge_slot = slot;
                    this.focus_field(Field::Challenge, window, cx);
                }))
                .child(if muted {
                    "type the word".to_string()
                } else {
                    shown
                }),
        )
}

fn opened_card(home: &Home, cx: &mut Context<Home>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .rounded_lg()
        .bg(rgb(CARD))
        .child(
            div()
                .id("copy-address-row")
                .cursor(CursorStyle::PointingHand)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.copy_address(cx)))
                .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                    "{}  ·  {}",
                    truncate_middle(&home.address, 12, 12),
                    if home.address_copy_hint_active() {
                        "copied"
                    } else {
                        "click to copy"
                    }
                ))),
        )
        .child(div().text_3xl().child(format_xmr(home.total_piconero)))
        .when(home.fiat_line(home.total_piconero).is_some(), |card| {
            card.child(
                div()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child(home.fiat_line(home.total_piconero).unwrap_or_default()),
            )
        })
        .child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .child(format!("Unlocked {}", format_xmr(home.unlocked_piconero))),
        )
        .child(action_button(
            "copy-address",
            l10n::t("Copy address"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.copy_address(cx)),
        ))
        .child(action_button(
            "go-receive",
            l10n::t("Receive"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.go_receive(cx)),
        ))
        .child(action_button(
            "go-send",
            l10n::t("Send"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.go_send(cx)),
        ))
        .child(action_button(
            "go-settings",
            l10n::t("Settings"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.go_settings(cx)),
        ))
        .when(home.scan_needs_retry, |card| {
            card.child(action_button(
                "retry-sync",
                l10n::t("Retry sync"),
                cx.listener(|this, _: &ClickEvent, _, cx| this.retry_refresh(cx)),
            ))
        })
        .child(action_button(
            "forget",
            l10n::t("Lock"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.forget(cx)),
        ))
}

fn sync_card(home: &Home, cx: &mut Context<Home>) -> impl IntoElement {
    let running = home.scan_was_running;
    let Some(sync) = home.sync.as_ref() else {
        return div()
            .flex()
            .flex_col()
            .gap_2()
            .p_5()
            .rounded_lg()
            .bg(rgb(CARD))
            .child(div().text_sm().child(l10n::t("Connecting to node")))
            .into_any_element();
    };
    let has_tip = sync_status::has_observed_tip(sync);
    let synced = sync_status::is_synced(sync, running, home.transfers.is_empty());
    let remaining = sync_status::remaining_blocks(sync);
    let progress = sync_status::progress(sync);
    let error = home.last_scan_error.as_deref();
    let stalled = home.scan_needs_retry && !running && !synced;
    let headline = sync_status::headline(
        synced,
        running,
        stalled,
        error,
        has_tip,
        sync.last_scanned == sync.restore_height,
    );
    let detail = sync_status::detail(
        synced,
        running,
        stalled,
        error,
        has_tip,
        sync.last_scanned,
        sync.restore_height,
        remaining,
    );
    let visible_rate = if home.scan_rate.recent_avg > 0.0 {
        Some(home.scan_rate.recent_avg)
    } else if home.scan_rate.avg > 0.0 {
        Some(home.scan_rate.avg)
    } else {
        None
    };
    let detail = match visible_rate {
        Some(rate) if running => format!("{detail} · {rate:.1} blk/s"),
        _ => detail,
    };
    let dot = if error.is_some() && !running {
        OUT
    } else if synced {
        IN
    } else {
        ACCENT
    };
    let show_progress = !synced || running;
    let expanded = home.scan_details_expanded;
    let fill = progress.clamp(0.0, 1.0) as f32;

    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .rounded_lg()
        .bg(rgb(CARD))
        .child(
            div()
                .id("sync-toggle")
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .cursor(CursorStyle::PointingHand)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.toggle_sync_details(cx);
                }))
                .child(div().size(px(10.)).rounded_full().bg(rgb(dot)))
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(headline),
                )
                .child(div().text_xs().text_color(rgb(ACCENT)).child(if expanded {
                    format!("{} {}", "v", l10n::t(sync_status::HIDE_SYNC_DETAILS))
                } else {
                    format!("{} {}", ">", l10n::t(sync_status::SHOW_SYNC_DETAILS))
                })),
        )
        .when(expanded, |card| {
            card.child(div().text_xs().text_color(rgb(MUTED)).child(detail.clone()))
        })
        .when(show_progress, |card| {
            card.child(
                div()
                    .w_full()
                    .h(px(6.))
                    .rounded_full()
                    .bg(rgb(FIELD))
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .w(relative(fill))
                            .bg(rgb(ACCENT)),
                    ),
            )
        })
        .when(expanded, |card| {
            let mut rows = vec![
                (l10n::t("Node"), home.scan_node_url()),
                (l10n::t("Scanned"), sync.last_scanned.to_string()),
                (l10n::t("Network Height"), sync.chain_height.to_string()),
                (l10n::t("Progress"), format!("{:.1}%", progress * 100.0)),
            ];
            if !synced {
                rows.push((l10n::t("Remaining"), format!("{remaining} blocks")));
            }
            if home.scan_rate.avg > 0.0 {
                rows.push((
                    l10n::t("Avg throughput"),
                    format!("{:.1} blk/s", home.scan_rate.avg),
                ));
            }
            if home.scan_rate.recent_avg > 0.0 {
                rows.push((
                    l10n::t("Recent throughput"),
                    format!("{:.1} blk/s", home.scan_rate.recent_avg),
                ));
            }
            card.children(rows.into_iter().map(|(label, value)| sync_kv(label, value)))
        })
        .into_any_element()
}

fn sync_kv(label: SharedString, value: String) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_between()
        .gap_3()
        .child(div().text_xs().text_color(rgb(MUTED)).child(label))
        .child(div().text_xs().child(value))
}

fn receive_card(home: &Home, window: &Window, cx: &mut Context<Home>) -> impl IntoElement {
    let amount_focused = home.recv_amount_focus.is_focused(window);
    let desc_focused = home.recv_desc_focus.is_focused(window);
    let label_focused = home.recv_label_focus.is_focused(window);
    let amount_label = if home.recv_amount.trim().is_empty() {
        match home.recv_amount_mode {
            AmountMode::Xmr => "Optional amount in XMR".to_string(),
            AmountMode::Fiat => format!("Optional amount in {}", home.fiat_currency),
        }
    } else {
        home.recv_amount.clone()
    };
    let desc_label = if home.recv_desc.trim().is_empty() {
        "Optional description".to_string()
    } else {
        home.recv_desc.clone()
    };
    let label_text = if home.recv_label.trim().is_empty() {
        "Label (optional)".to_string()
    } else {
        home.recv_label.clone()
    };
    let amount_muted = home.recv_amount.trim().is_empty();
    let desc_muted = home.recv_desc.trim().is_empty();
    let label_muted = home.recv_label.trim().is_empty();
    let uri = home.receive_uri();
    let recv_secondary = home
        .recv_piconero()
        .filter(|v| *v > 0)
        .and_then(|pico| home.amount_secondary(pico, home.recv_amount_mode));

    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .rounded_lg()
        .bg(rgb(CARD))
        .child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .child(l10n::t("Receive")),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(action_button(
                    "recv-prev",
                    l10n::t("Prev"),
                    cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.cycle_receive_address(false, cx);
                    }),
                ))
                .child(div().px_3().py_2().child(home.receive_book.display_label()))
                .child(action_button(
                    "recv-next",
                    l10n::t("Next"),
                    cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.cycle_receive_address(true, cx);
                    }),
                ))
                .child(action_button(
                    "recv-new",
                    l10n::t("New address"),
                    cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.create_receive_subaddress(cx);
                    }),
                )),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(l10n::t("Label")),
        )
        .child(
            div()
                .id("recv-label")
                .key_context("Field")
                .track_focus(&home.recv_label_focus)
                .cursor(CursorStyle::IBeam)
                .p_3()
                .rounded_md()
                .bg(rgb(FIELD))
                .border_1()
                .border_color(rgb(if label_focused { ACCENT } else { 0x2A3A2A }))
                .text_sm()
                .text_color(rgb(if label_muted { MUTED } else { TEXT }))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.focus_field(Field::RecvLabel, window, cx);
                }))
                .child(label_text),
        )
        .when_some(home.qr_image.clone(), |card, image| {
            card.child(
                img(ImageSource::Render(image))
                    .w(px(220.))
                    .h(px(220.))
                    .object_fit(ObjectFit::Contain),
            )
        })
        .child(
            div()
                .id("receive-address")
                .cursor(CursorStyle::PointingHand)
                .p_3()
                .rounded_md()
                .bg(rgb(FIELD))
                .text_sm()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.copy_address(cx)))
                .child(home.receive_address.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(match home.recv_amount_mode {
                    AmountMode::Xmr => l10n::t("Amount (optional, XMR)").to_string(),
                    AmountMode::Fiat => format!("Amount (optional, {})", home.fiat_currency),
                }),
        )
        .child(
            div()
                .id("recv-amount")
                .key_context("Field")
                .track_focus(&home.recv_amount_focus)
                .cursor(CursorStyle::IBeam)
                .p_3()
                .rounded_md()
                .bg(rgb(FIELD))
                .border_1()
                .border_color(rgb(if amount_focused { ACCENT } else { 0x2A3A2A }))
                .text_sm()
                .text_color(rgb(if amount_muted { MUTED } else { TEXT }))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.focus_field(Field::RecvAmount, window, cx);
                }))
                .child(amount_label),
        )
        .when(home.live_rate().is_some(), |card| {
            card.child(action_button(
                "recv-unit",
                if home.recv_amount_mode == AmountMode::Xmr {
                    l10n::t("Type in fiat")
                } else {
                    l10n::t("Type in XMR")
                },
                cx.listener(|this, _: &ClickEvent, _, cx| this.swap_amount_mode(false, cx)),
            ))
        })
        .when(recv_secondary.is_some(), |card| {
            card.child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(recv_secondary.clone().unwrap_or_default()),
            )
        })
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(l10n::t("Description (optional)")),
        )
        .child(
            div()
                .id("recv-desc")
                .key_context("Field")
                .track_focus(&home.recv_desc_focus)
                .cursor(CursorStyle::IBeam)
                .p_3()
                .rounded_md()
                .bg(rgb(FIELD))
                .border_1()
                .border_color(rgb(if desc_focused { ACCENT } else { 0x2A3A2A }))
                .text_sm()
                .text_color(rgb(if desc_muted { MUTED } else { TEXT }))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.focus_field(Field::RecvDesc, window, cx);
                }))
                .child(desc_label),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(if home.address_copy_hint_active() {
                    "Copied.".to_string()
                } else {
                    uri
                }),
        )
        .child(action_button(
            "receive-copy",
            l10n::t("Copy address"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.copy_address(cx)),
        ))
        .child(action_button(
            "receive-copy-uri",
            l10n::t("Copy payment URI"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.copy_receive_uri(cx)),
        ))
        .child(action_button(
            "receive-back",
            l10n::t("Back"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.go_wallet(cx)),
        ))
}

fn send_card(home: &Home, window: &Window, cx: &mut Context<Home>) -> impl IntoElement {
    let dest_focused = home.dest_focus.is_focused(window);
    let amount_focused = home.amount_focus.is_focused(window);
    let dest_label = if home.send_dest.trim().is_empty() {
        "Paste a Monero address or monero: URI".to_string()
    } else {
        home.send_dest.clone()
    };
    let amount_label = if home.send_amount.trim().is_empty() {
        match home.send_amount_mode {
            AmountMode::Xmr => "Amount in XMR".to_string(),
            AmountMode::Fiat => format!("Amount in {}", home.fiat_currency),
        }
    } else {
        home.send_amount.clone()
    };
    let dest_muted = home.send_dest.trim().is_empty();
    let amount_muted = home.send_amount.trim().is_empty();
    let fee_line = match (home.send_preview_amount, home.send_fee) {
        (Some(amt), Some(fee)) => {
            let mut line = format!(
                "Preview · {}  ·  fee {}",
                amount::format_xmr(amt),
                amount::format_xmr(fee)
            );
            if let Some(fiat) = home.fiat_line(amt) {
                line.push_str("  ·  ");
                line.push_str(&fiat);
            }
            line
        }
        _ => {
            let unlocked = home.send_unlocked();
            let mut line = if home.send_from_subaddress {
                format!(
                    "Unlocked {} · {}",
                    amount::format_xmr(unlocked),
                    home.receive_book.display_label_for(home.send_source_index)
                )
            } else {
                format!("Unlocked {}", amount::format_xmr(unlocked))
            };
            if let Some(fiat) = home.fiat_line(unlocked) {
                line.push_str("  ·  ");
                line.push_str(&fiat);
            }
            line
        }
    };
    let send_secondary = home
        .send_piconero()
        .filter(|v| *v > 0)
        .and_then(|pico| home.amount_secondary(pico, home.send_amount_mode));

    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .rounded_lg()
        .bg(rgb(CARD))
        .child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .child(l10n::t("Send")),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(l10n::t("Destination")),
        )
        .child(
            div()
                .id("send-dest")
                .key_context("Field")
                .track_focus(&home.dest_focus)
                .cursor(CursorStyle::IBeam)
                .min_h(px(56.))
                .p_3()
                .rounded_md()
                .bg(rgb(FIELD))
                .border_1()
                .border_color(rgb(if dest_focused { ACCENT } else { 0x2A3A2A }))
                .text_sm()
                .text_color(rgb(if dest_muted { MUTED } else { TEXT }))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.focus_field(Field::Dest, window, cx);
                }))
                .child(dest_label),
        )
        .child(action_button(
            "send-paste-qr",
            l10n::t("Paste QR from clipboard"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.paste_send_qr(cx)),
        ))
        .child(action_button(
            "send-open-qr",
            l10n::t("Open QR image"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.open_send_qr_image(cx)),
        ))
        .child(action_button(
            "send-scan-qr",
            if home.send_qr_busy {
                l10n::t("Looking for QR…")
            } else {
                l10n::t("Scan QR with camera")
            },
            cx.listener(|this, _: &ClickEvent, _, cx| this.scan_send_qr_camera(cx)),
        ))
        .child(action_button(
            "send-from-toggle",
            if home.send_from_subaddress {
                l10n::t("Spend from selected address")
            } else {
                l10n::t("Spend from whole wallet")
            },
            cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_send_from_subaddress(cx)),
        ))
        .when(home.send_from_subaddress, |card| {
            card.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(action_button(
                        "send-from-prev",
                        l10n::t("Prev"),
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.cycle_send_source(false, cx);
                        }),
                    ))
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .child(home.receive_book.display_label_for(home.send_source_index)),
                    )
                    .child(action_button(
                        "send-from-next",
                        l10n::t("Next"),
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.cycle_send_source(true, cx);
                        }),
                    )),
            )
        })
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(match home.send_amount_mode {
                    AmountMode::Xmr => l10n::t("Amount (XMR)").to_string(),
                    AmountMode::Fiat => format!("Amount ({})", home.fiat_currency),
                }),
        )
        .child(
            div()
                .id("send-amount")
                .key_context("Field")
                .track_focus(&home.amount_focus)
                .cursor(CursorStyle::IBeam)
                .p_3()
                .rounded_md()
                .bg(rgb(FIELD))
                .border_1()
                .border_color(rgb(if amount_focused { ACCENT } else { 0x2A3A2A }))
                .text_sm()
                .text_color(rgb(if amount_muted { MUTED } else { TEXT }))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.focus_field(Field::Amount, window, cx);
                }))
                .child(amount_label),
        )
        .when(home.live_rate().is_some(), |card| {
            card.child(action_button(
                "send-unit",
                if home.send_amount_mode == AmountMode::Xmr {
                    l10n::t("Type in fiat")
                } else {
                    l10n::t("Type in XMR")
                },
                cx.listener(|this, _: &ClickEvent, _, cx| this.swap_amount_mode(true, cx)),
            ))
        })
        .when(send_secondary.is_some(), |card| {
            card.child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child(send_secondary.clone().unwrap_or_default()),
            )
        })
        .child(div().text_xs().text_color(rgb(MUTED)).child(fee_line))
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(home.network_policy.label()),
        )
        .child(action_button(
            "send-max",
            l10n::t("Send max"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.fill_send_max(cx)),
        ))
        .child(action_button(
            "send-preview",
            if home.send_busy {
                l10n::t("Working…")
            } else {
                l10n::t("Preview fee")
            },
            cx.listener(|this, _: &ClickEvent, _, cx| this.run_preview(cx)),
        ))
        .child(action_button(
            "send-broadcast",
            l10n::t("Send"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.run_send(cx)),
        ))
        .child(action_button(
            "send-back",
            l10n::t("Back"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.go_wallet(cx)),
        ))
}

fn terms_card(home: &Home, cx: &mut Context<Home>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .rounded_lg()
        .bg(rgb(CARD))
        .child(
            div()
                .text_xl()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(l10n::t("Terms of Use")),
        )
        .children(
            legal::SUMMARY
                .iter()
                .map(|line| div().text_sm().text_color(rgb(MUTED)).child(*line)),
        )
        .child(action_button(
            "review-terms",
            l10n::t("Review full Terms of Use"),
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.open_legal(legal::Document::Terms, cx);
            }),
        ))
        .child(
            div()
                .id("terms-check")
                .cursor(CursorStyle::PointingHand)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.terms_checked = !this.terms_checked;
                    cx.notify();
                }))
                .child(format!(
                    "{}  {}",
                    if home.terms_checked { "[x]" } else { "[ ]" },
                    l10n::t("I have read and agree to the Terms of Use")
                )),
        )
        .when(!home.terms_checked, |card| {
            card.child(
                div()
                    .id("terms-agree")
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x2A332A))
                    .text_color(rgb(MUTED))
                    .child(l10n::t("I Agree")),
            )
        })
        .when(home.terms_checked, |card| {
            card.child(action_button(
                "terms-agree",
                l10n::t("I Agree"),
                cx.listener(|this, _: &ClickEvent, _, cx| this.accept_terms(cx)),
            ))
        })
        .child(action_button(
            "terms-quit",
            l10n::t("Quit"),
            cx.listener(|_, _: &ClickEvent, _, cx| cx.quit()),
        ))
}

fn legal_card(home: &Home, cx: &mut Context<Home>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .rounded_lg()
        .bg(rgb(CARD))
        .flex_1()
        .min_h(px(240.))
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(home.legal_doc.title()),
        )
        .child(
            div()
                .id("legal-body")
                .flex_1()
                .overflow_y_scroll()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(home.legal_doc.body()),
        )
        .child(action_button(
            "legal-back",
            l10n::t("Back"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.close_legal(cx)),
        ))
}

fn settings_card(home: &Home, window: &Window, cx: &mut Context<Home>) -> impl IntoElement {
    let node_focused = home.node_focus.is_focused(window);
    let i2p_node_focused = home.i2p_rpc_focus.is_focused(window);
    let i2p_proxy_focused = home.i2p_proxy_focus.is_focused(window);
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .rounded_lg()
        .bg(rgb(CARD))
        .child(div().text_sm().text_color(rgb(MUTED)).child(l10n::t("Settings")))
        .child(div().text_xs().text_color(rgb(MUTED)).child(l10n::t("How to connect")))
        .child(action_button(
            "net-policy",
            home.network_policy.label(),
            cx.listener(|this, _: &ClickEvent, _, cx| this.cycle_network_policy(cx)),
        ))
        .when(home.network_policy != network::Policy::I2p, |card| {
            card.child(div().text_xs().text_color(rgb(MUTED)).child(l10n::t("Clearnet node")))
                .child(
                    div()
                        .id("settings-node")
                        .key_context("Field")
                        .track_focus(&home.node_focus)
                        .cursor(CursorStyle::IBeam)
                        .p_3()
                        .rounded_md()
                        .bg(rgb(FIELD))
                        .border_1()
                        .border_color(rgb(if node_focused { ACCENT } else { 0x2A3A2A }))
                        .text_sm()
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.focus_field(Field::Node, window, cx);
                        }))
                        .child(home.node_url.clone()),
                )
        })
        .when(home.network_policy != network::Policy::Clearnet, |card| {
            card.child(div().text_xs().text_color(rgb(MUTED)).child(l10n::t("I2P node (host:port)")))
                .child(
                    div()
                        .id("settings-i2p-node")
                        .key_context("Field")
                        .track_focus(&home.i2p_rpc_focus)
                        .cursor(CursorStyle::IBeam)
                        .p_3()
                        .rounded_md()
                        .bg(rgb(FIELD))
                        .border_1()
                        .border_color(rgb(if i2p_node_focused { ACCENT } else { 0x2A3A2A }))
                        .text_sm()
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.focus_field(Field::I2pNode, window, cx);
                        }))
                        .child(if home.i2p_rpc.trim().is_empty() {
                            "hostname.b32.i2p:18081".to_string()
                        } else {
                            home.i2p_rpc.clone()
                        }),
                )
                .child(div().text_xs().text_color(rgb(MUTED)).child(l10n::t("I2P HTTP proxy (host:port)")))
                .child(
                    div()
                        .id("settings-i2p-proxy")
                        .key_context("Field")
                        .track_focus(&home.i2p_proxy_focus)
                        .cursor(CursorStyle::IBeam)
                        .p_3()
                        .rounded_md()
                        .bg(rgb(FIELD))
                        .border_1()
                        .border_color(rgb(if i2p_proxy_focused { ACCENT } else { 0x2A3A2A }))
                        .text_sm()
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.focus_field(Field::I2pProxy, window, cx);
                        }))
                        .child(if home.i2p_proxy.trim().is_empty() {
                            "127.0.0.1:4444".to_string()
                        } else {
                            home.i2p_proxy.clone()
                        }),
                )
                .child(action_button(
                    "apply-i2p",
                    l10n::t("Apply I2P settings"),
                    cx.listener(|this, _: &ClickEvent, _, cx| this.apply_network(cx)),
                ))
        })
        .child(action_button(
            "settings-auth",
            if home.require_device_auth {
                l10n::t("Device authentication: on")
            } else {
                l10n::t("Device authentication: off")
            },
            cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_device_auth(cx)),
        ))
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(if device_auth::is_available() {
                    l10n::t("When on, Touch ID or your login password is required to unlock and send.")
                } else {
                    l10n::t("Touch ID / password is not available on this computer.")
                }),
        )
        .child(action_button(
            "settings-scan-benchmark",
            if home.benchmark_running {
                l10n::t("Scan benchmark running…")
            } else {
                l10n::t("Run scan benchmark")
            },
            cx.listener(|this, _: &ClickEvent, _, cx| this.run_scan_benchmark(cx)),
        ))
        .child(
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(l10n::t(
                    "Stops the current sync, compares 25, 50, 75, 100, 125, 150, and 500-block batches in shuffled repeated samples, then saves JSON results.",
                )),
        )
        .child(action_button(
            "settings-fiat",
            if home.fiat_enabled {
                l10n::t("Fiat estimates: on")
            } else {
                l10n::t("Fiat estimates: off")
            },
            cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_fiat(cx)),
        ))
        .when(home.fiat_enabled, |card| {
            card.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(action_button(
                        "fiat-prev",
                        l10n::t("Prev currency"),
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.cycle_fiat_currency(false, cx);
                        }),
                    ))
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .child(home.fiat_currency.clone()),
                    )
                    .child(action_button(
                        "fiat-next",
                        l10n::t("Next currency"),
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.cycle_fiat_currency(true, cx);
                        }),
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("Optional. Fetches XMR/USD from api.kraken.com and, if needed, FX from api.frankfurter.dev. Those servers see your IP. Amounts and addresses are not sent."),
            )
        })
        .child(action_button(
            "settings-terms",
            l10n::t("Terms of Use"),
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.open_legal(legal::Document::Terms, cx);
            }),
        ))
        .child(action_button(
            "settings-privacy",
            l10n::t("Privacy Policy"),
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.open_legal(legal::Document::Privacy, cx);
            }),
        ))
        .child(action_button(
            "settings-license",
            l10n::t("License"),
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.open_legal(legal::Document::License, cx);
            }),
        ))
        .child(action_button(
            "settings-remove",
            l10n::t("Remove wallet from this computer"),
            cx.listener(|this, _: &ClickEvent, _, cx| this.remove_stored_wallet(cx)),
        ))
        .child(action_button(
            "settings-back",
            if home.opened {
                l10n::t("Back")
            } else {
                l10n::t("Back to restore")
            },
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.screen = if this.opened {
                    Screen::Wallet
                } else {
                    Screen::Restore
                };
                cx.notify();
            }),
        ))
}

fn status_line(home: &Home) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(rgb(MUTED))
        .child(home.status.clone())
}

fn history(home: &Home) -> impl IntoElement {
    div()
        .id("history")
        .flex_1()
        .min_h(px(180.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1()
        .children(home.transfers.iter().enumerate().map(|(idx, row)| {
            let color = match row.direction.as_str() {
                "in" => IN,
                "out" => OUT,
                _ => MUTED,
            };
            let label = match row.direction.as_str() {
                "in" => "Received",
                "out" => "Sent",
                "self" => "Self",
                other => other,
            };
            let sign = match row.direction.as_str() {
                "in" => "+ ",
                "out" => "− ",
                _ => "",
            };
            let conf = if row.is_pending || row.confirmations == 0 {
                "pending".to_string()
            } else {
                format!("{} conf", row.confirmations)
            };
            div()
                .id(("tx", idx))
                .px_3()
                .py_2()
                .rounded_md()
                .bg(rgb(ROW))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(color))
                                .child(format!("{label} {sign}{}", format_xmr(row.amount))),
                        )
                        .child(div().text_xs().text_color(rgb(MUTED)).child(format!(
                            "{} · {} · {}",
                            row.height.unwrap_or(0),
                            &row.txid.chars().take(8).collect::<String>(),
                            conf
                        )))
                        .when_some(
                            home.fiat_snapshots.get(&row.txid).map(|snap| {
                                fiat::recorded_approx(row.amount, snap.fiat_per_xmr, &snap.currency)
                            }),
                            |col, line| {
                                col.child(div().text_xs().text_color(rgb(MUTED)).child(line))
                            },
                        ),
                )
        }))
}

fn action_button(
    id: &'static str,
    label: impl Into<SharedString>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(ACCENT))
        .text_color(rgb(0xffffff))
        .on_click(listener)
        .child(label.into())
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn normalize_seed(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_xmr(piconero: u64) -> String {
    const PICONERO: u64 = 1_000_000_000_000;
    let whole = piconero / PICONERO;
    let frac = (piconero % PICONERO) / 1_000_000;
    format!("{whole}.{frac:06} XMR")
}

fn truncate_middle(value: &str, head: usize, tail: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= head + tail + 1 {
        return value.to_string();
    }
    format!(
        "{}…{}",
        chars[..head].iter().collect::<String>(),
        chars[chars.len() - tail..].iter().collect::<String>()
    )
}

fn sort_transfers(rows: &mut [Transfer]) {
    rows.sort_by(|a, b| match (a.is_pending, b.is_pending) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => b
            .height
            .unwrap_or(0)
            .cmp(&a.height.unwrap_or(0))
            .then_with(|| a.txid.cmp(&b.txid)),
    });
}

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
}

fn hide(_: &Hide, cx: &mut App) {
    cx.hide();
}

fn hide_others(_: &HideOthers, cx: &mut App) {
    cx.hide_other_apps();
}

fn show_all(_: &ShowAll, cx: &mut App) {
    cx.unhide_other_apps();
}

fn show_app(_: &ShowApp, cx: &mut App) {
    cx.activate(true);
}

fn minimize(_: &Minimize, cx: &mut App) {
    if let Some(window) = cx.active_window() {
        let _ = window.update(cx, |_, window, _| window.minimize_window());
    }
}

fn install_menus(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let app_items = vec![
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action("Hide nexawal", Hide),
        MenuItem::action("Hide Others", HideOthers),
        MenuItem::action("Show All", ShowAll),
        MenuItem::separator(),
        MenuItem::action("Quit nexawal", Quit),
    ];
    #[cfg(not(target_os = "macos"))]
    let app_items = vec![MenuItem::action("Quit nexawal", Quit)];

    cx.set_menus([
        Menu::new("nexawal").items(app_items),
        Menu::new("Edit").items([
            MenuItem::os_action("Cut", CutField, OsAction::Cut),
            MenuItem::os_action("Copy", CopyField, OsAction::Copy),
            MenuItem::os_action("Paste", PasteField, OsAction::Paste),
            MenuItem::separator(),
            MenuItem::os_action("Select All", SelectAllField, OsAction::SelectAll),
        ]),
        Menu::new("Wallet").items([
            MenuItem::action("Open & sync", OpenWallet),
            MenuItem::action("Retry sync", RetrySync),
            MenuItem::action("Copy address", CopyAddress),
            MenuItem::action("Receive", ShowReceive),
            MenuItem::action("Send", ShowSend),
            MenuItem::action("Wallet", ShowWallet),
            MenuItem::action("Settings", ShowSettings),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Show nexawal", ShowApp),
        ]),
    ]);
    cx.set_dock_menu(vec![MenuItem::action("Show nexawal", ShowApp)]);
}

fn main() {
    let startup_node = std::env::var("NEXAWAL_NODE_URL").unwrap_or_default();
    scan_tuning::apply_for_node(&startup_node);
    application().run(|cx: &mut App| {
        cx.set_app_identity("com.nexatrode.nexawal", "NexaWal");
        platform_icon::install_process_icon();
        // Zed does this first so the process is a normal Mac app (Dock + menu bar + Cmd-Tab).
        cx.activate(true);
        cx.on_action(quit);
        cx.on_action(hide);
        cx.on_action(hide_others);
        cx.on_action(show_all);
        cx.on_action(show_app);
        cx.on_action(minimize);
        install_menus(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
            KeyBinding::new("cmd-m", Minimize, None),
            KeyBinding::new("cmd-v", PasteField, None),
            KeyBinding::new("ctrl-v", PasteField, None),
            KeyBinding::new("cmd-c", CopyField, None),
            KeyBinding::new("ctrl-c", CopyField, None),
            KeyBinding::new("cmd-x", CutField, None),
            KeyBinding::new("ctrl-x", CutField, None),
            KeyBinding::new("cmd-a", SelectAllField, None),
            KeyBinding::new("ctrl-a", SelectAllField, None),
            KeyBinding::new("backspace", BackspaceField, None),
        ]);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(720.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("nexawal".into()),
                    ..Default::default()
                }),
                focus: true,
                app_id: Some("com.nexatrode.nexawal".into()),
                icon: platform_icon::window_icon(),
                window_min_size: Some(size(px(520.), px(520.))),
                ..Default::default()
            },
            |window, cx| {
                let should_unlock = should_auto_unlock_stored();
                let home = cx.new(|cx| {
                    let mut home = Home::new(cx);
                    if !should_unlock {
                        home.seed_focus.focus(window, cx);
                    }
                    if home.fiat_enabled {
                        home.maybe_refresh_fiat(cx);
                    }
                    home
                });
                if should_unlock {
                    let startup_home = home.clone();
                    window.defer(cx, move |_, cx| {
                        let _ = startup_home.update(cx, |home, cx| {
                            home.try_unlock_stored(cx);
                        });
                    });
                }
                home
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
