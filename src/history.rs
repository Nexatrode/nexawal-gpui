//! Virtualized desktop history, backed by bounded local WalletCore pages.
use super::*;
use api::{HistoryFilter, HistoryPage, HistoryQuery};
use chrono::{Datelike, Local, NaiveDate, TimeZone};

const ROW_HEIGHT: f32 = 132.;
use std::collections::{BTreeMap, HashSet, VecDeque};

/// Match WalletCore counts without re-fetching pages when only the tip moves.
pub(super) fn confirmations(
    height: Option<u64>,
    pending: bool,
    chain_height: u64,
    cached: u64,
) -> u64 {
    if pending {
        return 0;
    }
    match height {
        Some(0) => 0,
        Some(height) if chain_height > 0 => chain_height.saturating_sub(height).saturating_add(1),
        _ => cached,
    }
}

#[derive(Default)]
pub(super) struct Pager {
    pages: BTreeMap<usize, Vec<Transfer>>,
    recency: VecDeque<usize>,
    in_flight: HashSet<usize>,
    failed: HashSet<usize>,
    pub(super) generation: u64,
    query_key: String,
    anchor: Option<String>,
    count: usize,
    pub total: usize,
    pub pending: usize,
    revision: Option<String>,
    error: Option<String>,
    changed: bool,
    pub selected: Option<Transfer>,
    pub preview_running: bool,
    pub(super) scroll: gpui::UniformListScrollHandle,
}
impl Pager {
    fn row(&self, index: usize) -> Option<&Transfer> {
        self.pages.get(&(index / 50 * 50))?.get(index % 50)
    }
    fn insert(&mut self, page: HistoryPage) {
        if self.anchor.take().is_some() {
            self.scroll
                .scroll_to_item(page.anchor_offset.unwrap_or(0), gpui::ScrollStrategy::Top);
        }
        self.count = page.matching_count;
        self.total = page.total_count;
        self.pending = page.pending_count;
        self.revision = Some(page.revision);
        self.recency.retain(|&offset| offset != page.offset);
        self.recency.push_back(page.offset);
        self.pages.insert(page.offset, page.transfers);
        while self.recency.len() > 4 {
            if let Some(old) = self.recency.pop_front() {
                self.pages.remove(&old);
            }
        }
    }
    pub fn clear(&mut self) {
        let generation = self.generation.wrapping_add(1);
        *self = Self::default();
        self.generation = generation;
    }
}

/// Resolve local midnight, advancing through a rare midnight DST gap if necessary.
/// Computing the next date separately preserves 23/25-hour days across DST.
fn local_start_of_day(date: NaiveDate) -> Option<i64> {
    for minute in 0..(24 * 60) {
        let time = date.and_hms_opt((minute / 60) as u32, (minute % 60) as u32, 0)?;
        if let Some(local) = Local.from_local_datetime(&time).earliest() {
            return Some(local.timestamp());
        }
    }
    None
}

fn date_seconds(value: &str, end: bool) -> Result<Option<u64>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let value = value.trim();
    if value.len() != 10 {
        return Err(
            l10n::t("Enter valid dates in YYYY-MM-DD format, or leave them blank.").to_string(),
        );
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        l10n::t("Enter valid dates in YYYY-MM-DD format, or leave them blank.").to_string()
    })?;
    if !(1970..=9999).contains(&date.year()) {
        return Err(
            l10n::t("Enter valid dates in YYYY-MM-DD format, or leave them blank.").to_string(),
        );
    }
    let boundary_date = if end {
        date.succ_opt().ok_or_else(|| {
            l10n::t("Enter valid dates in YYYY-MM-DD format, or leave them blank.").to_string()
        })?
    } else {
        date
    };
    let mut seconds = local_start_of_day(boundary_date).ok_or_else(|| {
        l10n::t("Enter valid dates in YYYY-MM-DD format, or leave them blank.").to_string()
    })?;
    if end {
        seconds = seconds.saturating_sub(1);
    }
    Ok(Some(seconds.max(0) as u64))
}

impl Home {
    pub(super) fn history_query(&self) -> Result<HistoryQuery, String> {
        let from = date_seconds(&self.transfer_from, false)?;
        let to = date_seconds(&self.transfer_to, true)?;
        if from.zip(to).is_some_and(|(a, b)| a > b) {
            return Err(
                l10n::t("Enter valid dates in YYYY-MM-DD format, or leave them blank.").to_string(),
            );
        }
        Ok(HistoryQuery {
            filter: match self.transfer_filter {
                TransferFilter::All => HistoryFilter::All,
                TransferFilter::Received => HistoryFilter::Received,
                TransferFilter::Sent => HistoryFilter::Sent,
                TransferFilter::Pending => HistoryFilter::Pending,
            },
            search: self.transfer_search.clone(),
            from_timestamp: from,
            to_timestamp: to,
            ..Default::default()
        })
    }
    pub(super) fn go_history(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Transactions;
        self.active = Field::TransferSearch;
        cx.notify();
    }
    fn reset_history_query(&mut self, key: String, cx: &mut Context<Self>) {
        let total = self.history_pages.total;
        let pending = self.history_pages.pending;
        self.history_pages.clear();
        self.history_pages.query_key = key;
        self.history_pages.total = total;
        self.history_pages.pending = pending;
        self.load_history_page(0, cx);
    }
    fn reload_history(&mut self, cx: &mut Context<Self>) {
        let anchor = self
            .history_pages
            .row(
                self.history_pages
                    .scroll
                    .0
                    .borrow()
                    .base_handle
                    .logical_scroll_top()
                    .0,
            )
            .map(|r| r.txid.clone());
        let key = self.history_pages.query_key.clone();
        let total = self.history_pages.total;
        let pending = self.history_pages.pending;
        self.history_pages.clear();
        self.history_pages.query_key = key;
        self.history_pages.total = total;
        self.history_pages.pending = pending;
        self.history_pages.anchor = anchor;
        self.load_history_page(0, cx);
    }
    fn load_history_page(&mut self, index: usize, cx: &mut Context<Self>) {
        let offset = index / 50 * 50;
        if self.history_pages.pages.contains_key(&offset) {
            self.history_pages.recency.retain(|o| *o != offset);
            self.history_pages.recency.push_back(offset);
            return;
        }
        if self.history_pages.changed
            || self.history_pages.failed.contains(&offset)
            || self.history_pages.in_flight.contains(&offset)
        {
            return;
        }
        let mut query = match self.history_query() {
            Ok(query) => query,
            Err(error) => {
                self.history_pages.error = Some(error);
                return;
            }
        };
        query.offset = offset;
        query.revision = self.history_pages.revision.clone();
        query.anchor_txid = self.history_pages.anchor.clone();
        if offset > 0 && query.revision.is_none() {
            return;
        }
        let generation = self.history_pages.generation;
        let address = self.address.clone();
        self.history_pages.in_flight.insert(offset);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { api::query_transfers(WALLET_ID, &query) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.history_pages.generation != generation
                    || !this.opened
                    || this.address != address
                {
                    return;
                }
                this.history_pages.in_flight.remove(&offset);
                match result {
                    Ok(page) => {
                        this.record_history_fiat(&page.transfers);
                        this.history_pages.insert(page);
                    }
                    Err(error) => {
                        this.history_pages.failed.insert(offset);
                        if error.message.contains("stale_history_cursor") {
                            this.history_pages.changed = true;
                        } else {
                            this.history_pages.error = Some(error.message);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn refresh_history_preview(&mut self, cx: &mut Context<Self>) {
        if self.history_pages.preview_running || !self.opened {
            return;
        }
        self.history_pages.preview_running = true;
        let address = self.address.clone();
        let generation = self.history_pages.generation;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async {
                    (
                        api::query_transfers(
                            WALLET_ID,
                            &HistoryQuery {
                                limit: 10,
                                ..Default::default()
                            },
                        ),
                        api::refresh_job(WALLET_ID),
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.opened
                    || this.address != address
                    || this.history_pages.generation != generation
                {
                    return;
                }
                this.history_pages.preview_running = false;
                if let (Ok(page), job) = result {
                    let clean = matches!(job, RefreshJob::Idle)
                        && !this.scan_needs_retry
                        && page.chain_height > 0
                        && page.last_scanned_height >= page.chain_height;
                    if page.total_count == 0 && this.history_pages.total > 0 && !clean {
                        return;
                    }
                    if this
                        .history_pages
                        .revision
                        .as_ref()
                        .is_some_and(|r| r != &page.revision)
                    {
                        this.history_pages.changed = true;
                    }
                    this.history_pages.total = page.total_count;
                    this.history_pages.pending = page.pending_count;
                    this.record_history_fiat(&page.transfers);
                    this.transfers = page.transfers;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn record_history_fiat(&mut self, rows: &[Transfer]) {
        if self.fiat_enabled {
            let rate = self.live_rate().cloned();
            let opted_in = paths::ensure_fiat_opted_in_at();
            self.fiat_snapshots.record_new_transfers(
                rows.iter().map(|t| (t.txid.as_str(), t.timestamp)),
                rate.as_ref(),
                opted_in,
            );
        }
    }
    pub(super) fn select_history_transaction(&mut self, txid: String, cx: &mut Context<Self>) {
        let address = self.address.clone();
        let generation = self.history_pages.generation;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { api::transfer_by_id(WALLET_ID, &txid) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if !this.opened
                    || this.address != address
                    || this.history_pages.generation != generation
                {
                    return;
                }
                match result {
                    Ok(Some(row)) => this.history_pages.selected = Some(row),
                    Ok(None) => this.history_pages.changed = true,
                    Err(error) => this.history_pages.error = Some(error.message),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn row(row: &Transfer, home: &Home, cx: &mut Context<Home>) -> gpui::Stateful<gpui::Div> {
    let txid = row.txid.clone();
    let (label, sign, color) = match row.direction.as_str() {
        "in" => ("Received", "+", theme_in()),
        "out" => ("Sent", "−", theme_out()),
        _ => ("Self", "", theme_text()),
    };
    div()
        .id(SharedString::from(row.txid.clone()))
        .h(px(ROW_HEIGHT))
        .px_3()
        .py_2()
        .bg(rgb(theme_row()))
        .flex()
        .flex_col()
        .gap_1()
        .overflow_hidden()
        .cursor_pointer()
        .on_click(
            cx.listener(move |this, _, _, cx| this.select_history_transaction(txid.clone(), cx)),
        )
        .child(div().text_sm().text_color(rgb(color)).child(format!(
            "{} {sign}{}",
            l10n::t(label),
            format_xmr(row.amount)
        )))
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme_muted()))
                .child(format!(
                    "{} · {}",
                    format_timestamp(row.timestamp),
                    if row.is_pending {
                        "Pending".into()
                    } else {
                        format!(
                            "{} confirmations",
                            confirmations(
                                row.height,
                                row.is_pending,
                                home.sync.as_ref().map_or(0, |s| s.chain_height),
                                row.confirmations
                            )
                        )
                    }
                )),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme_muted()))
                .child(truncate_middle(&row.txid, 18, 18)),
        )
        .when_some(
            home.fiat_snapshots
                .get(&row.txid)
                .map(|snap| fiat::recorded_approx(row.amount, snap.fiat_per_xmr, &snap.currency)),
            |view, line| view.child(div().text_xs().text_color(rgb(theme_muted())).child(line)),
        )
        .when_some(row.fee, |col, fee| {
            col.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme_muted()))
                    .child(format!(
                        "Fee {}{}",
                        format_xmr(fee),
                        if row.direction == "in" {
                            " · paid by sender"
                        } else {
                            ""
                        }
                    )),
            )
        })
}

pub(super) fn preview(home: &Home, cx: &mut Context<Home>) -> gpui::AnyElement {
    if let Some(selected) = &home.history_pages.selected {
        return transfer_detail(
            selected,
            home.sync.as_ref().map_or(0, |s| s.chain_height),
            cx,
        )
        .into_any_element();
    }
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .bg(rgb(theme_card()))
        .rounded_lg()
        .child(div().text_sm().child(l10n::t("Recent Transactions")))
        .children(
            home.transfers
                .iter()
                .map(|transfer| row(transfer, home, cx)),
        )
        .when(home.transfers.is_empty(), |view| {
            view.child(div().text_sm().child(l10n::t("No transactions yet.")))
        })
        .child(secondary_action_button(
            "view-all-history",
            format!("{}", l10n::t("View all transactions (%lld)"))
                .replace("%lld", &home.history_pages.total.to_string()),
            cx.listener(|this, _, _, cx| this.go_history(cx)),
        ))
        .when(home.history_pages.pending > 0, |view| {
            view.child(secondary_action_button(
                "view-pending-history",
                format!("{}", l10n::t("%lld pending transactions"))
                    .replace("%lld", &home.history_pages.pending.to_string()),
                cx.listener(|this, _, _, cx| {
                    this.transfer_filter = TransferFilter::Pending;
                    this.go_history(cx);
                }),
            ))
        })
        .into_any_element()
}

pub(super) fn screen(home: &mut Home, window: &Window, cx: &mut Context<Home>) -> gpui::AnyElement {
    if let Some(selected) = &home.history_pages.selected {
        return transfer_detail(
            selected,
            home.sync.as_ref().map_or(0, |s| s.chain_height),
            cx,
        )
        .into_any_element();
    }
    let key = format!(
        "{}|{}|{}|{}",
        home.transfer_filter as u8, home.transfer_search, home.transfer_from, home.transfer_to
    );
    if home.history_pages.query_key != key {
        home.reset_history_query(key, cx);
    }
    let count = home.history_pages.count;
    div()
        .flex()
        .flex_col()
        .gap_2()
        .min_h(px(0.))
        .flex_1()
        .p_4()
        .bg(rgb(theme_card()))
        .rounded_lg()
        .child(div().text_lg().child(l10n::t("Transactions")))
        .child(field_input(
            home,
            window,
            cx,
            Field::TransferSearch,
            "transactions-search",
            l10n::t("Search transaction ID"),
            false,
            true,
        ))
        .child(
            div().flex().gap_2().children(
                [
                    ("all", "All", TransferFilter::All),
                    ("in", "Received", TransferFilter::Received),
                    ("out", "Sent", TransferFilter::Sent),
                    ("pending", "Pending", TransferFilter::Pending),
                ]
                .into_iter()
                .map(|(id, label, filter)| {
                    history_filter_button(
                        id,
                        l10n::t(label),
                        home.transfer_filter == filter,
                        cx.listener(move |this, _, _, cx| {
                            this.transfer_filter = filter;
                            cx.notify();
                        }),
                    )
                }),
            ),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(div().flex_1().min_w(px(0.)).child(field_input(
                    home,
                    window,
                    cx,
                    Field::HistoryFrom,
                    "history-from",
                    format!("{} (YYYY-MM-DD)", l10n::t("From")),
                    false,
                    true,
                )))
                .child(div().flex_1().min_w(px(0.)).child(field_input(
                    home,
                    window,
                    cx,
                    Field::HistoryTo,
                    "history-to",
                    format!("{} (YYYY-MM-DD)", l10n::t("Through")),
                    false,
                    true,
                ))),
        )
        .child(
            div().text_sm().child(
                l10n::t("%1$d matching · %2$d total")
                    .replace("%1$d", &count.to_string())
                    .replace("%2$d", &home.history_pages.total.to_string()),
            ),
        )
        .when(home.scan_was_running || home.scan_needs_retry, |view| {
            view.child(
                div()
                    .text_sm()
                    .child(l10n::t("History may be incomplete while syncing.")),
            )
        })
        .when(home.history_pages.changed, |view| {
            view.child(secondary_action_button(
                "history-reload",
                l10n::t("History changed · Reload transactions"),
                cx.listener(|this, _, _, cx| {
                    this.reload_history(cx);
                }),
            ))
        })
        .when_some(home.history_pages.error.clone(), |view, error| {
            view.child(div().text_sm().child(error))
                .child(secondary_action_button(
                    "history-retry",
                    l10n::t("Retry loading history"),
                    cx.listener(|this, _, _, cx| {
                        let offsets: Vec<_> = this.history_pages.failed.drain().collect();
                        this.history_pages.error = None;
                        for offset in offsets {
                            this.load_history_page(offset, cx);
                        }
                    }),
                ))
        })
        .when(!home.history_pages.in_flight.is_empty(), |view| {
            view.child(div().text_sm().child(l10n::t("Loading transaction…")))
        })
        .when(
            count == 0
                && home.history_pages.in_flight.is_empty()
                && home.history_pages.error.is_none(),
            |view| {
                view.child(div().text_sm().child(if home.history_pages.total > 0 {
                    l10n::t("No transactions match these filters.")
                } else {
                    l10n::t("No transactions yet.")
                }))
            },
        )
        .child(
            gpui::uniform_list(
                "transaction-rows",
                count,
                cx.processor(|this, range: Range<usize>, _, cx| {
                    let mut rows = Vec::new();
                    for index in range {
                        this.load_history_page(index, cx);
                        rows.push(
                            if let Some(transfer) = this.history_pages.row(index).cloned() {
                                row(&transfer, this, cx).into_any_element()
                            } else {
                                div()
                                    .h(px(ROW_HEIGHT))
                                    .child(if this.history_pages.changed {
                                        l10n::t("Reload history to continue.")
                                    } else {
                                        l10n::t("Loading transaction…")
                                    })
                                    .into_any_element()
                            },
                        );
                    }
                    rows
                }),
            )
            .track_scroll(&home.history_pages.scroll)
            .h(px(
                (f32::from(window.viewport_size().height) - 430.).max(180.)
            )),
        )
        .child(secondary_action_button(
            "history-export",
            l10n::t("Export matching transactions (CSV)"),
            cx.listener(|this, _, _, cx| this.export_transfer_history(cx)),
        ))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_counts_follow_tip_without_mutating_pages() {
        assert_eq!(confirmations(Some(100), false, 100, 1), 1);
        assert_eq!(confirmations(Some(100), false, 109, 1), 10);
        assert_eq!(confirmations(Some(100), false, 102, 10), 3);
        assert_eq!(confirmations(Some(100), false, 99, 10), 1);
        assert_eq!(confirmations(Some(100), true, 109, 9), 0);
        assert_eq!(confirmations(None, false, 109, 9), 9);
        assert_eq!(confirmations(Some(100), false, 0, 9), 9);
        assert_eq!(confirmations(Some(0), false, 109, 9), 0);
        assert_eq!(confirmations(Some(1), false, u64::MAX, 0), u64::MAX);
    }

    #[test]
    fn desktop_cache_is_bounded_and_old_pages_reload() {
        let mut pager = Pager::default();
        for offset in (0..10000).step_by(50) {
            let transfers: Vec<_> = (offset..offset + 50)
                .map(|i| Transfer {
                    txid: format!("{i:064x}"),
                    direction: "in".into(),
                    amount: i as u64,
                    fee: Some(7),
                    height: Some(100),
                    timestamp: None,
                    confirmations: 10,
                    is_pending: false,
                    subaddress_major: None,
                    subaddress_minor: None,
                })
                .collect();
            pager.insert(HistoryPage {
                schema_version: 1,
                wallet_id: "test".into(),
                revision: "1".into(),
                total_count: 10000,
                matching_count: 10000,
                pending_count: 0,
                offset,
                next_offset: None,
                anchor_offset: None,
                last_scanned_height: 100,
                chain_height: 110,
                chain_time: 0,
                transfers,
            });
            assert!(pager.pages.len() <= 4);
            assert!(pager.pages.values().map(Vec::len).sum::<usize>() <= 200);
            assert_eq!(pager.row(offset).unwrap().amount, offset as u64);
        }
        assert!(pager.row(0).is_none());
        let generation = pager.generation;
        pager.clear();
        assert_eq!(pager.total, 0);
        assert_ne!(pager.generation, generation);
        assert!(pager.pages.is_empty());
    }

    #[test]
    fn local_date_ranges_validate_calendar_dates_and_end_at_next_local_midnight() {
        let start = date_seconds("1970-01-01", false).unwrap().unwrap();
        let end = date_seconds("1970-01-01", true).unwrap().unwrap();
        let next_day = NaiveDate::from_ymd_opt(1970, 1, 2).unwrap();
        let next_start = local_start_of_day(next_day).unwrap().max(0) as u64;
        assert_eq!(end + 1, next_start);
        let local_start = Local.timestamp_opt(start as i64, 0).single().unwrap();
        assert_eq!(
            local_start.date_naive(),
            NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()
        );
        assert!(date_seconds("2025-02-29", false).is_err());
        assert!(date_seconds("2024-02-29", false).is_ok());
        assert!(date_seconds("2025-13-01", false).is_err());
        for (year, month, day) in [(2024, 3, 10), (2024, 11, 3)] {
            let date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
            let next = date.succ_opt().unwrap();
            let seconds = local_start_of_day(next).unwrap() - local_start_of_day(date).unwrap();
            assert!(matches!(seconds, 82_800 | 86_400 | 90_000));
            assert_eq!(
                date_seconds(&date.format("%Y-%m-%d").to_string(), true)
                    .unwrap()
                    .unwrap() as i64
                    + 1,
                local_start_of_day(next).unwrap().max(0)
            );
        }
    }
}
