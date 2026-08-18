//! Match iOS/Android scan env so range `get_blocks.bin` runs with prune=true.

const FAST_BATCH: &str = "500";
const STALL_BATCH: &str = "150";
pub const STALL_SECS: u64 = 125;

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
        if cfg!(debug_assertions) {
            std::env::set_var("WALLETCORE_SCAN_LOG", "1");
        } else {
            std::env::set_var("WALLETCORE_SCAN_LOG", "0");
        }
    }
}

/// Fast-sync path Catalyst uses: range batches of 500 with pruned txs.
pub fn apply() {
    set_range_batch(FAST_BATCH);
}

/// After a stall or truncated fetch, shrink to 150 like iOS/Android.
pub fn apply_stall_fallback() {
    set_range_batch(STALL_BATCH);
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
