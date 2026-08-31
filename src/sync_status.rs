//! Wallet sync card: progress, remaining blocks, and scan throughput.

use std::time::{Duration, Instant};

use crate::l10n;
use monerowalletcore::api::SyncStatus;

const TIP_TOLERANCE: u64 = 3;
const RECENT_WINDOW: Duration = Duration::from_secs(30);

/// English labels matching iOS/Android. Full locale catalogs are not copied yet.
pub const SHOW_SYNC_DETAILS: &str = "Show sync details";
pub const HIDE_SYNC_DETAILS: &str = "Hide sync details";

#[derive(Clone, Debug, Default)]
pub struct ScanRate {
    session_start: Option<Instant>,
    session_scanned: Option<u64>,
    last_progress_scanned: Option<u64>,
    recent: Vec<(Instant, u64)>,
    pub avg: f64,
    pub recent_avg: f64,
}

impl ScanRate {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Session-average plus a trailing ~30s window, matching iOS/Android.
    pub fn note(&mut self, last_scanned: u64, running: bool) {
        if !running {
            return;
        }
        self.note_at(last_scanned, Instant::now());
    }

    fn note_at(&mut self, last_scanned: u64, now: Instant) {
        if self.session_start.is_none() || self.session_scanned.is_none() {
            self.session_start = Some(now);
            self.session_scanned = Some(last_scanned);
            self.last_progress_scanned = Some(last_scanned);
            return;
        }

        let previous = self.last_progress_scanned.unwrap_or(last_scanned);
        if last_scanned < previous {
            self.reset();
            self.session_start = Some(now);
            self.session_scanned = Some(last_scanned);
            self.last_progress_scanned = Some(last_scanned);
            return;
        }
        // A fetch, final ledger rebuild, or cache persist can leave the scanned height
        // unchanged for a while. Keep the last measured rate instead of letting the
        // displayed average decay on every UI poll.
        if last_scanned == previous {
            return;
        }
        self.last_progress_scanned = Some(last_scanned);

        let start = self.session_start.unwrap();
        let baseline = self.session_scanned.unwrap();
        let elapsed = now.saturating_duration_since(start).as_secs_f64();
        let scanned = last_scanned.saturating_sub(baseline);
        if scanned > 0 && elapsed >= 0.5 {
            self.avg = scanned as f64 / elapsed;
        }
        self.recent.push((now, last_scanned));
        self.recent
            .retain(|(at, _)| now.saturating_duration_since(*at) <= RECENT_WINDOW);
        if self.recent.len() < 2 {
            return;
        }
        let first = self.recent[0];
        let last = self.recent[self.recent.len() - 1];
        let dt = last.0.saturating_duration_since(first.0).as_secs_f64();
        let db = last.1.saturating_sub(first.1);
        if db > 0 && dt >= 0.5 {
            self.recent_avg = db as f64 / dt;
        }
    }
}

pub fn has_observed_tip(sync: &SyncStatus) -> bool {
    sync.chain_height > sync.restore_height || sync.chain_time > 0
}

pub fn remaining_blocks(sync: &SyncStatus) -> u64 {
    if !has_observed_tip(sync) {
        return 0;
    }
    let diff = sync.chain_height.saturating_sub(sync.last_scanned);
    if diff > TIP_TOLERANCE { diff } else { 0 }
}

pub fn progress(sync: &SyncStatus) -> f64 {
    if !has_observed_tip(sync) {
        return 0.0;
    }
    if sync.chain_height > 0 && sync.last_scanned.saturating_add(TIP_TOLERANCE) >= sync.chain_height
    {
        return 1.0;
    }
    if sync.chain_height <= sync.restore_height {
        return 0.0;
    }
    let clamped = sync.last_scanned.min(sync.chain_height);
    let work = sync.chain_height - sync.restore_height;
    let completed = clamped.saturating_sub(sync.restore_height);
    (completed as f64 / work as f64).clamp(0.0, 1.0)
}

pub fn is_synced(sync: &SyncStatus, running: bool, transfers_empty: bool) -> bool {
    if running || !has_observed_tip(sync) || sync.chain_height == 0 {
        return false;
    }
    if sync.last_scanned.saturating_add(TIP_TOLERANCE) < sync.chain_height {
        return false;
    }
    if sync.chain_height > sync.restore_height.saturating_add(10_000) && transfers_empty {
        return false;
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyncErrorKind {
    Stalled,
    NodeUnreachable,
    Failed,
}

fn classify_error(message: &str, stalled: bool) -> SyncErrorKind {
    let lower = message.to_ascii_lowercase();
    if stalled || lower.contains("sync stalled") || lower.contains("no scan progress") {
        return SyncErrorKind::Stalled;
    }
    const NETWORK_ERRORS: [&str; 14] = [
        "connection refused",
        "connection reset",
        "connection timed out",
        "timed out",
        "timeout/disconnect",
        "failed to connect",
        "could not connect",
        "couldn't connect",
        "network is unreachable",
        "node unreachable",
        "not reachable",
        "no route to host",
        "name or service not known",
        "transport error",
    ];
    if NETWORK_ERRORS.iter().any(|pattern| lower.contains(pattern))
        || lower.contains("dns")
        || lower.contains("tls handshake")
    {
        SyncErrorKind::NodeUnreachable
    } else {
        SyncErrorKind::Failed
    }
}

pub fn headline(
    synced: bool,
    running: bool,
    stalled: bool,
    error: Option<&str>,
    has_tip: bool,
    last_scanned_eq_restore: bool,
) -> String {
    if let Some(kind) = error
        .filter(|_| !running && !synced)
        .map(|message| classify_error(message, stalled))
    {
        return match kind {
            SyncErrorKind::Stalled => l10n::t("Sync stalled").into(),
            SyncErrorKind::NodeUnreachable => l10n::t("Node unreachable").into(),
            SyncErrorKind::Failed => l10n::t("Sync failed").into(),
        };
    }
    if synced {
        return l10n::t("Wallet synced").into();
    }
    if !has_tip {
        return l10n::t("Connecting to node").into();
    }
    if running && last_scanned_eq_restore {
        return l10n::t("Scanning blockchain").into();
    }
    l10n::t("Syncing wallet").into()
}

pub fn detail(
    synced: bool,
    running: bool,
    stalled: bool,
    error: Option<&str>,
    has_tip: bool,
    last_scanned: u64,
    restore_height: u64,
    remaining: u64,
) -> String {
    if error
        .filter(|_| !running && !synced)
        .is_some_and(|message| classify_error(message, stalled) == SyncErrorKind::Stalled)
    {
        return l10n::t("Tap Retry sync to continue (or reopen the app).").into();
    }
    if let Some(err) = error.filter(|_| !running && !synced) {
        let trimmed = err.trim();
        if trimmed.chars().count() <= 120 {
            return trimmed.to_string();
        }
        return format!("{}…", trimmed.chars().take(117).collect::<String>());
    }
    if synced {
        return with_i64("Scanned to block %lld", last_scanned);
    }
    if !has_tip {
        return l10n::t("Waiting for network height").into();
    }
    if running && last_scanned == restore_height {
        return with_i64("Fetching initial blocks from %lld", restore_height);
    }
    with_i64("%lld blocks remaining", remaining)
}

fn with_i64(key: &str, value: u64) -> String {
    format!("{}", l10n::t(key)).replace("%lld", &value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync(restore: u64, scanned: u64, chain: u64, chain_time: u64) -> SyncStatus {
        SyncStatus {
            chain_height: chain,
            chain_time,
            last_refresh_timestamp: 0,
            last_scanned: scanned,
            restore_height: restore,
        }
    }

    #[test]
    fn progress_from_restore_to_tip() {
        let st = sync(1_000, 1_500, 2_000, 1);
        assert!((progress(&st) - 0.5).abs() < 0.001);
        assert_eq!(remaining_blocks(&st), 500);
        assert!(!is_synced(&st, true, false));
        assert!(!is_synced(&st, false, false));
    }

    #[test]
    fn near_tip_is_complete() {
        let st = sync(0, 99, 100, 1);
        assert_eq!(progress(&st), 1.0);
        assert_eq!(remaining_blocks(&st), 0);
        assert!(is_synced(&st, false, false));
    }

    #[test]
    fn no_tip_yet() {
        let st = sync(80, 80, 80, 0);
        assert_eq!(progress(&st), 0.0);
        assert_eq!(
            headline(false, true, false, None, false, true).as_str(),
            "Connecting to node"
        );
        assert_eq!(
            detail(false, true, false, None, false, 80, 80, 0),
            "Waiting for network height"
        );
    }

    #[test]
    fn unchanged_height_does_not_decay_average() {
        let start = Instant::now();
        let mut rate = ScanRate::default();
        rate.note_at(1_000, start);
        rate.note_at(1_500, start + Duration::from_secs(2));
        let average = rate.avg;

        rate.note_at(1_500, start + Duration::from_secs(20));

        assert_eq!(rate.avg, average);
        assert_eq!(rate.avg, 250.0);
    }

    #[test]
    fn rewind_starts_a_new_rate_session() {
        let start = Instant::now();
        let mut rate = ScanRate::default();
        rate.note_at(1_000, start);
        rate.note_at(1_500, start + Duration::from_secs(2));
        rate.note_at(900, start + Duration::from_secs(3));
        rate.note_at(1_100, start + Duration::from_secs(4));

        assert_eq!(rate.avg, 200.0);
    }

    #[test]
    fn sync_detail_a11y_labels_match_ios_android_english() {
        assert_eq!(SHOW_SYNC_DETAILS, "Show sync details");
        assert_eq!(HIDE_SYNC_DETAILS, "Hide sync details");
    }

    #[test]
    fn sync_errors_only_blame_the_node_for_transport_failures() {
        assert_eq!(
            classify_error("refresh already running for wallet", false),
            SyncErrorKind::Failed
        );
        assert_eq!(
            classify_error("cache JSON was malformed", false),
            SyncErrorKind::Failed
        );
        assert_eq!(
            classify_error("connection refused", false),
            SyncErrorKind::NodeUnreachable
        );
        assert_eq!(classify_error("anything", true), SyncErrorKind::Stalled);
        assert_eq!(
            headline(
                false,
                false,
                false,
                Some("refresh already running for wallet"),
                true,
                false
            ),
            "Sync failed"
        );
    }
}
