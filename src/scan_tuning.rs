//! Match iOS/Android scan env so range `get_blocks.bin` runs with prune=true.

use std::env;

const FAST_BATCH: &str = "500";
const STALL_BATCH: &str = "150";
const CUPRATE_BATCH: &str = "500";
const REFRESH_TELEMETRY_DEFAULT: u8 = 0;
pub const STALL_SECS: u64 = 125;

#[derive(Clone, Copy)]
enum ScanProfile {
    Fast,
    CuprateSafe,
    StallFallback,
}

impl ScanProfile {
    fn batch(self) -> &'static str {
        match self {
            Self::Fast => FAST_BATCH,
            Self::CuprateSafe => CUPRATE_BATCH,
            Self::StallFallback => STALL_BATCH,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::CuprateSafe => "cuprate",
            Self::StallFallback => "stall-fallback",
        }
    }
}

fn profile_from_env() -> Option<ScanProfile> {
    let raw = env::var("NEXAWAL_SCAN_PROFILE").ok()?;
    match raw.to_ascii_lowercase().as_str() {
        "fast" => Some(ScanProfile::Fast),
        "cuprate" | "cuprate_safe" | "cuprate-safe" => Some(ScanProfile::CuprateSafe),
        "stall" | "fallback" | "stall_fallback" | "stall-fallback" => {
            Some(ScanProfile::StallFallback)
        }
        _ => None,
    }
}

fn profile_for_node(node_url: &str) -> ScanProfile {
    if let Some(profile) = profile_from_env() {
        return profile;
    }
    if node_url.contains("rpc.nexatrode.com") || node_url.contains("cuprate") {
        ScanProfile::CuprateSafe
    } else {
        ScanProfile::Fast
    }
}

fn set_range_batch(batch: &str) {
    unsafe {
        std::env::remove_var("WALLETCORE_SCAN_PAR");
        std::env::remove_var("WALLETCORE_SCAN_BATCH");
        std::env::remove_var("WALLETCORE_BULK_FETCH");
        std::env::remove_var("WALLETCORE_WALLET2_FAST_FALLBACK");
        std::env::remove_var("WALLETCORE_BULK_BIN_DEBUG");
        std::env::set_var("WALLETCORE_BULK_MODE", "range");
        std::env::set_var("WALLETCORE_BULK_FETCH_BATCH", batch);
        std::env::set_var("WALLETCORE_UPSTREAM_BLOCK_BATCH", batch);
        std::env::set_var(
            "WALLETCORE_REFRESH_TELEMETRY",
            REFRESH_TELEMETRY_DEFAULT.to_string(),
        );
        std::env::remove_var("WALLETCORE_PREFETCH_DEPTH");
        if cfg!(debug_assertions) {
            std::env::set_var("WALLETCORE_SCAN_LOG", "1");
        } else {
            std::env::set_var("WALLETCORE_SCAN_LOG", "0");
        }
    }
}

fn apply_profile(profile: ScanProfile) {
    let batch = profile.batch();
    set_range_batch(batch);
    println!(
        "🧪 walletcore scan tuning profile={} batch={}",
        profile.label(),
        batch
    );
}

/// Fast-sync path Catalyst uses: range batches with prune=true.
#[allow(dead_code)]
pub fn apply() {
    apply_profile(ScanProfile::Fast);
}

/// Fast-sync path with optional Cuprate-safe heuristic.
pub fn apply_for_node(node_url: &str) {
    apply_profile(profile_for_node(node_url));
}

/// After a stall or truncated fetch, shrink to 150 like iOS/Android.
pub fn apply_stall_fallback() {
    apply_profile(ScanProfile::StallFallback);
}

pub fn is_recoverable_fetch_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("429")
        || msg.contains("too many requests")
        || msg.contains("rate limit")
        || msg.contains("timeout")
        || msg.contains("timed out")
        || msg.contains("response body closed before all bytes were read")
        || msg.contains("interface error")
        || msg.contains("channelclosed")
        || msg.contains("channel closed")
        || msg.contains("connection reset")
        || msg.contains("broken pipe")
        || msg.contains("unexpected eof")
        || msg.contains("http 4")
        || msg.contains("http 5")
        || msg.contains("status code 4")
        || msg.contains("status code 5")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recoverable_truncated_body() {
        assert!(is_recoverable_fetch_error(
            "contiguous_scannable_blocks_error: response body closed before all bytes were read"
        ));
        assert!(!is_recoverable_fetch_error("wallet not opened"));
    }
}
