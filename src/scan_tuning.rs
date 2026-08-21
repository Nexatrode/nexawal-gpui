//! Match iOS/Android scan env so range `get_blocks.bin` runs with prune=true.

use std::env;

const FAST_BATCH: &str = "500";
const BATCH_750: &str = "750";
const BATCH_1000: &str = "1000";
const BATCH_150: &str = "150";
const BATCH_25: &str = "25";
const BATCH_50: &str = "50";
const BATCH_75: &str = "75";
const BATCH_100: &str = "100";
const BATCH_125: &str = "125";
const CUPRATE_BATCH: &str = "500";
const REFRESH_TELEMETRY_DEFAULT: u8 = 0;
pub const STALL_SECS: u64 = 125;

#[derive(Clone, Copy)]
enum ScanProfile {
    Fast,
    CuprateSafe,
    Batch750,
    Batch1000,
    Batch150,
    Batch25,
    Batch50,
    Batch75,
    Batch100,
    Batch125,
    Serial75,
    Parallel75,
    DecodeSerial75,
    DecodeParallel75,
    DecodeSerial500,
    DecodeParallel500,
}

impl ScanProfile {
    fn batch(self) -> &'static str {
        match self {
            Self::Fast => FAST_BATCH,
            Self::CuprateSafe => CUPRATE_BATCH,
            Self::Batch750 => BATCH_750,
            Self::Batch1000 => BATCH_1000,
            Self::Batch150 => BATCH_150,
            Self::Batch25 => BATCH_25,
            Self::Batch50 => BATCH_50,
            Self::Batch75 => BATCH_75,
            Self::Batch100 => BATCH_100,
            Self::Batch125 => BATCH_125,
            Self::Serial75 | Self::Parallel75 | Self::DecodeSerial75 | Self::DecodeParallel75 => {
                BATCH_75
            }
            Self::DecodeSerial500 | Self::DecodeParallel500 => FAST_BATCH,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::CuprateSafe => "cuprate",
            Self::Batch750 => "batch-750",
            Self::Batch1000 => "batch-1000",
            Self::Batch150 => "batch-150",
            Self::Batch25 => "batch-25",
            Self::Batch50 => "batch-50",
            Self::Batch75 => "batch-75",
            Self::Batch100 => "batch-100",
            Self::Batch125 => "batch-125",
            Self::Serial75 => "serial-75",
            Self::Parallel75 => "parallel-75",
            Self::DecodeSerial75 => "decode-serial-75",
            Self::DecodeParallel75 => "decode-parallel-75",
            Self::DecodeSerial500 => "decode-serial-500",
            Self::DecodeParallel500 => "decode-parallel-500",
        }
    }

    fn scan_parallelism(self) -> Option<&'static str> {
        match self {
            Self::Serial75 => Some("1"),
            Self::Parallel75 => Some("auto"),
            _ => None,
        }
    }

    fn range_decode_parallelism(self) -> Option<&'static str> {
        match self {
            Self::DecodeSerial75 | Self::DecodeSerial500 => Some("0"),
            Self::DecodeParallel75 | Self::DecodeParallel500 => Some("1"),
            _ => None,
        }
    }
}

fn profile_from_env() -> Option<ScanProfile> {
    let raw = env::var("NEXAWAL_SCAN_PROFILE").ok()?;
    profile_from_name(&raw)
}

fn profile_from_name(name: &str) -> Option<ScanProfile> {
    match name.to_ascii_lowercase().as_str() {
        "fast" => Some(ScanProfile::Fast),
        "cuprate" | "cuprate_safe" | "cuprate-safe" => Some(ScanProfile::CuprateSafe),
        "batch-750" | "batch_750" | "750" => Some(ScanProfile::Batch750),
        "batch-1000" | "batch_1000" | "1000" => Some(ScanProfile::Batch1000),
        "stall" | "fallback" | "stall_fallback" | "stall-fallback" | "batch-150" | "batch_150"
        | "150" => Some(ScanProfile::Batch150),
        "batch-25" | "batch_25" | "25" => Some(ScanProfile::Batch25),
        "batch-50" | "batch_50" | "50" => Some(ScanProfile::Batch50),
        "batch-75" | "batch_75" | "75" => Some(ScanProfile::Batch75),
        "batch-100" | "batch_100" | "100" => Some(ScanProfile::Batch100),
        "batch-125" | "batch_125" | "125" => Some(ScanProfile::Batch125),
        "serial-75" | "serial_75" => Some(ScanProfile::Serial75),
        "parallel-75" | "parallel_75" => Some(ScanProfile::Parallel75),
        "decode-serial-75" | "decode_serial_75" => Some(ScanProfile::DecodeSerial75),
        "decode-parallel-75" | "decode_parallel_75" => Some(ScanProfile::DecodeParallel75),
        "decode-serial-500" | "decode_serial_500" => Some(ScanProfile::DecodeSerial500),
        "decode-parallel-500" | "decode_parallel_500" => Some(ScanProfile::DecodeParallel500),
        _ => None,
    }
}

fn set_range_batch(batch: &str) {
    unsafe {
        std::env::remove_var("WALLETCORE_SCAN_PAR");
        std::env::remove_var("WALLETCORE_SCAN_BATCH");
        std::env::remove_var("WALLETCORE_BULK_FETCH");
        std::env::remove_var("WALLETCORE_WALLET2_FAST_FALLBACK");
        std::env::remove_var("WALLETCORE_BULK_BIN_DEBUG");
        std::env::remove_var("WALLETCORE_RANGE_DECODE_PAR");
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
    unsafe {
        match profile.scan_parallelism() {
            Some(value) => std::env::set_var("WALLETCORE_SCAN_PAR", value),
            None => std::env::remove_var("WALLETCORE_SCAN_PAR"),
        }
        match profile.range_decode_parallelism() {
            Some(value) => std::env::set_var("WALLETCORE_RANGE_DECODE_PAR", value),
            None => std::env::remove_var("WALLETCORE_RANGE_DECODE_PAR"),
        }
    }
    println!(
        "🧪 walletcore scan tuning profile={} batch={} scan_threads={} range_decode={}",
        profile.label(),
        batch,
        profile.scan_parallelism().unwrap_or("auto"),
        match profile.range_decode_parallelism() {
            Some("1") => "parallel-shared",
            _ => "serial",
        }
    );
}

fn apply_walletcore_defaults() {
    unsafe {
        std::env::remove_var("WALLETCORE_SCAN_PAR");
        std::env::remove_var("WALLETCORE_SCAN_BATCH");
        std::env::remove_var("WALLETCORE_WALLET2_FAST_FALLBACK");
        std::env::remove_var("WALLETCORE_BULK_BIN_DEBUG");
        std::env::remove_var("WALLETCORE_RANGE_DECODE_PAR");
        std::env::set_var(
            "WALLETCORE_REFRESH_TELEMETRY",
            REFRESH_TELEMETRY_DEFAULT.to_string(),
        );
        if cfg!(debug_assertions) {
            std::env::set_var("WALLETCORE_SCAN_LOG", "1");
        } else {
            std::env::set_var("WALLETCORE_SCAN_LOG", "0");
        }
    }
    println!("🧪 walletcore scan tuning: using platform-aware WalletCore defaults");
}

/// Normal scan path: use WalletCore's platform-aware range and decode defaults.
#[allow(dead_code)]
pub fn apply() {
    apply_walletcore_defaults();
}

/// Fast-sync path with optional Cuprate-safe heuristic.
pub fn apply_for_node(node_url: &str) {
    let _ = node_url;
    if let Some(profile) = profile_from_env() {
        apply_profile(profile);
    } else {
        apply_walletcore_defaults();
    }
}

/// Apply an explicitly named profile for diagnostics and benchmarks.
pub fn apply_named(name: &str) -> Option<&'static str> {
    let profile = profile_from_name(name)?;
    apply_profile(profile);
    Some(profile.label())
}

pub fn batch_for(name: &str) -> Option<&'static str> {
    profile_from_name(name).map(ScanProfile::batch)
}

/// Remove temporary benchmark profile variables so normal scans return to WalletCore defaults.
pub fn clear_profile_override() {
    unsafe {
        std::env::remove_var("WALLETCORE_SCAN_PAR");
        std::env::remove_var("WALLETCORE_BULK_MODE");
        std::env::remove_var("WALLETCORE_BULK_FETCH_BATCH");
        std::env::remove_var("WALLETCORE_UPSTREAM_BLOCK_BATCH");
        std::env::remove_var("WALLETCORE_RANGE_DECODE_PAR");
    }
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

    #[test]
    fn benchmark_parallelism_profiles_share_the_same_batch() {
        let serial = profile_from_name("serial-75").expect("serial profile");
        let parallel = profile_from_name("parallel-75").expect("parallel profile");
        assert_eq!(serial.batch(), BATCH_75);
        assert_eq!(parallel.batch(), BATCH_75);
        assert_eq!(serial.scan_parallelism(), Some("1"));
        assert_eq!(parallel.scan_parallelism(), Some("auto"));
    }

    #[test]
    fn decode_profiles_only_change_range_decoding() {
        for (serial_name, parallel_name, expected_batch) in [
            ("decode-serial-75", "decode-parallel-75", BATCH_75),
            ("decode-serial-500", "decode-parallel-500", FAST_BATCH),
        ] {
            let serial = profile_from_name(serial_name).expect("serial decode profile");
            let parallel = profile_from_name(parallel_name).expect("parallel decode profile");
            assert_eq!(serial.batch(), expected_batch);
            assert_eq!(parallel.batch(), expected_batch);
            assert_eq!(serial.scan_parallelism(), None);
            assert_eq!(parallel.scan_parallelism(), None);
            assert_eq!(serial.range_decode_parallelism(), Some("0"));
            assert_eq!(parallel.range_decode_parallelism(), Some("1"));
        }
    }

    #[test]
    fn large_response_profiles_are_diagnostic_only_named_batches() {
        assert_eq!(profile_from_name("batch-750").unwrap().batch(), BATCH_750);
        assert_eq!(profile_from_name("batch-1000").unwrap().batch(), BATCH_1000);
        assert_eq!(profile_from_name("750").unwrap().label(), "batch-750");
        assert_eq!(profile_from_name("1000").unwrap().label(), "batch-1000");
    }
}
