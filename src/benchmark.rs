//! Bounded scan benchmarks for comparing WalletCore scan profiles.
//!
//! Each profile gets a fresh in-memory wallet ID derived from the currently opened
//! wallet. The real wallet's checkpoint is never advanced by this diagnostic.

use std::collections::BTreeMap;
use std::fs;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use monerowalletcore::api::{self, RefreshJob};
use serde_json::{Value, json};

use crate::{paths, scan_tuning};

const PROFILE_NAMES: [&str; 5] = ["fast", "cuprate", "stall", "batch-100", "batch-125"];
const DEFAULT_REPETITIONS: usize = 3;
const DEFAULT_WINDOW_SECS: u64 = 6;
const CANCEL_WAIT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_STALL_BPS: f64 = 40.0;

#[derive(Debug)]
pub struct BenchmarkReport {
    pub results_path: String,
    pub rpc_results_path: String,
    pub summary: String,
}

pub fn run_id() -> u64 {
    now_ms() as u64 ^ u64::from(std::process::id())
}

pub fn targets_for(current: &str) -> Vec<String> {
    if let Ok(raw) = std::env::var("NEXAWAL_BENCHMARK_NODES") {
        let targets = raw
            .split(',')
            .map(str::trim)
            .filter(|node| !node.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if !targets.is_empty() {
            return targets;
        }
    }

    let current = current.trim();
    if current.contains("rpc.nexatrode.com") {
        vec![
            current.to_string(),
            current.replace("rpc.nexatrode.com", "monero.nexatrode.com"),
        ]
    } else if current.contains("monero.nexatrode.com") {
        vec![
            current.to_string(),
            current.replace("monero.nexatrode.com", "rpc.nexatrode.com"),
        ]
    } else {
        vec![current.to_string()]
    }
}

#[derive(Debug)]
struct ProfileResult {
    node_label: String,
    profile: &'static str,
    batch: &'static str,
    repetition: usize,
    order: usize,
    started_at_ms: u128,
    start_height: u64,
    end_height: u64,
    chain_height: u64,
    elapsed_ms: u128,
    blocks_per_second: f64,
    returned_blocks: u64,
    batch_count: u64,
    fetch_wait_ms: u128,
    fetch_rpc_ms: u128,
    rpc_calls: u64,
    rpc_request_bytes: u64,
    rpc_response_bytes: u64,
    rpc_elapsed_ms: u128,
    rpc_errors: u64,
    retries: u64,
    stalled: bool,
    outcome: &'static str,
    error: Option<String>,
}

#[derive(Debug, Default)]
struct SampleMetrics {
    returned_blocks: u64,
    batch_count: u64,
    fetch_wait_ms: u128,
    fetch_rpc_ms: u128,
    rpc_calls: u64,
    rpc_request_bytes: u64,
    rpc_response_bytes: u64,
    rpc_elapsed_ms: u128,
    rpc_errors: u64,
    retries: u64,
}

#[derive(Debug, Default)]
struct SummaryStats {
    total: usize,
    usable: usize,
    stalled: usize,
    all_bps: f64,
    usable_bps: f64,
}

pub fn run(node_url: String, mnemonic: String, start_height: u64, run_id: u64) -> BenchmarkReport {
    let results_path = paths::scan_benchmark_path().display().to_string();
    let rpc_telemetry_path = paths::scan_benchmark_rpc_path(run_id);
    let _ = fs::remove_file(&rpc_telemetry_path);
    unsafe {
        std::env::set_var("WALLETCORE_RPC_TELEMETRY_PATH", &rpc_telemetry_path);
    }
    let nodes = targets_for(&node_url);
    let (repetitions, profile_window) = benchmark_config();
    let mut results = Vec::with_capacity(PROFILE_NAMES.len() * nodes.len() * repetitions);

    // The caller has already requested cancellation. Give WalletCore a short grace
    // period so the benchmark does not compete with the user's prior scan.
    wait_for_idle("main_wallet", CANCEL_WAIT);

    for (node_index, target) in nodes.iter().enumerate() {
        for repetition in 0..repetitions {
            let profile_order = shuffled_profiles(run_id, node_index, repetition);
            for (order, profile_name) in profile_order.iter().enumerate() {
                let sample_index = results.len();
                let wallet_id = format!("nexawal-benchmark-{run_id}-{sample_index}");
                let result = run_profile(
                    &wallet_id,
                    profile_name,
                    target,
                    &mnemonic,
                    start_height,
                    profile_window,
                    repetition + 1,
                    order + 1,
                );
                let line = json!({
                    "timestamp_ms": result.started_at_ms,
                    "node": target,
                    "profile": result.profile,
                    "batch": result.batch,
                    "repetition": result.repetition,
                    "order": result.order,
                    "start_height": result.start_height,
                    "end_height": result.end_height,
                    "chain_height": result.chain_height,
                    "elapsed_ms": result.elapsed_ms,
                    "blocks": result.end_height.saturating_sub(result.start_height),
                    "blocks_per_second": result.blocks_per_second,
                    "returned_blocks": result.returned_blocks,
                    "batch_count": result.batch_count,
                    "fetch_wait_ms": result.fetch_wait_ms,
                    "fetch_rpc_ms": result.fetch_rpc_ms,
                    "rpc_calls": result.rpc_calls,
                    "rpc_request_bytes": result.rpc_request_bytes,
                    "rpc_response_bytes": result.rpc_response_bytes,
                    "rpc_elapsed_ms": result.rpc_elapsed_ms,
                    "rpc_errors": result.rpc_errors,
                    "retries": result.retries,
                    "stalled": result.stalled,
                    "sample_quality": if result.stalled {
                        "stall"
                    } else if result.error.is_some() {
                        "error"
                    } else {
                        "usable"
                    },
                    "outcome": result.outcome,
                    "error": result.error,
                });
                let _ = paths::append_scan_benchmark(&line.to_string());
                results.push(result);
            }
        }
    }

    unsafe {
        std::env::remove_var("WALLETCORE_RPC_TELEMETRY_PATH");
    }

    let mut averages = BTreeMap::<String, SummaryStats>::new();
    for result in &results {
        let key = format!(
            "{} {} ({})",
            result.node_label, result.profile, result.batch
        );
        let entry = averages.entry(key).or_default();
        entry.total += 1;
        entry.all_bps += result.blocks_per_second;
        if result.stalled {
            entry.stalled += 1;
        } else if result.error.is_none() {
            entry.usable += 1;
            entry.usable_bps += result.blocks_per_second;
        }
    }
    let summary = format!(
        "{} samples · {}",
        results.len(),
        averages
            .into_iter()
            .map(|(key, stats)| {
                let usable_avg = if stats.usable == 0 {
                    0.0
                } else {
                    stats.usable_bps / stats.usable as f64
                };
                format!(
                    "{key} {:.1} avg ({}/{} usable, {} stalls; all {:.1})",
                    usable_avg,
                    stats.usable,
                    stats.total,
                    stats.stalled,
                    stats.all_bps / stats.total.max(1) as f64
                )
            })
            .collect::<Vec<_>>()
            .join(" · ")
    );

    BenchmarkReport {
        results_path,
        rpc_results_path: rpc_telemetry_path.display().to_string(),
        summary,
    }
}

fn run_profile(
    wallet_id: &str,
    profile_name: &'static str,
    node_url: &str,
    mnemonic: &str,
    start_height: u64,
    profile_window: Duration,
    repetition: usize,
    order: usize,
) -> ProfileResult {
    let started_at_ms = now_ms();
    let started = Instant::now();
    let node_label = node_label(node_url);
    let batch = scan_tuning::batch_for(profile_name).unwrap_or("unknown");
    let log_path = paths::walletcore_log_path(wallet_id);
    let log_offset = file_len(&log_path);
    let rpc_path = std::env::var_os("WALLETCORE_RPC_TELEMETRY_PATH").map(std::path::PathBuf::from);
    let rpc_offset = rpc_path.as_deref().map(file_len).unwrap_or(0);

    let open_result = api::open_from_mnemonic(wallet_id, mnemonic, start_height, true)
        .and_then(|_| api::set_gap_limit(wallet_id, 50));
    if let Err(err) = open_result {
        return failed_result(
            node_label,
            profile_name,
            batch,
            repetition,
            order,
            started_at_ms,
            start_height,
            started.elapsed().as_millis(),
            "open-failed",
            err.to_string(),
        );
    }

    let _ = scan_tuning::apply_named(profile_name);
    if let Err(err) = api::refresh_async(wallet_id, node_url) {
        return failed_result(
            node_label,
            profile_name,
            batch,
            repetition,
            order,
            started_at_ms,
            start_height,
            started.elapsed().as_millis(),
            "start-failed",
            err.to_string(),
        );
    }

    let sample_started = Instant::now();
    let deadline = sample_started + profile_window;
    let mut last_scanned = start_height;
    let mut chain_height = start_height;
    let mut outcome = "window-complete";
    let mut error = None;

    while Instant::now() < deadline {
        match api::refresh_job(wallet_id) {
            RefreshJob::Failed(message) => {
                outcome = "scan-failed";
                error = Some(message);
                break;
            }
            RefreshJob::Running | RefreshJob::Idle => {
                if let Ok(status) = api::sync_status(wallet_id) {
                    last_scanned = status.last_scanned;
                    chain_height = status.chain_height;
                    if matches!(api::refresh_job(wallet_id), RefreshJob::Idle)
                        && status.last_scanned >= status.chain_height
                        && status.chain_height > start_height
                    {
                        outcome = "completed";
                        break;
                    }
                }
            }
        }
        thread::sleep(POLL_INTERVAL);
    }

    let elapsed_ms = sample_started.elapsed().as_millis();
    let _ = api::refresh_cancel(wallet_id);
    wait_for_idle(wallet_id, CANCEL_WAIT);

    let metrics = collect_metrics(&log_path, log_offset, rpc_path.as_deref(), rpc_offset);

    let blocks = last_scanned.saturating_sub(start_height);
    let blocks_per_second = if elapsed_ms == 0 {
        0.0
    } else {
        blocks as f64 / (elapsed_ms as f64 / 1_000.0)
    };
    let stalled = matches!(outcome, "window-complete" | "completed")
        && (blocks == 0 || blocks_per_second < benchmark_stall_bps());
    if stalled {
        outcome = "stalled";
    }

    ProfileResult {
        node_label,
        profile: profile_name,
        batch,
        repetition,
        order,
        started_at_ms,
        start_height,
        end_height: last_scanned,
        chain_height,
        elapsed_ms,
        blocks_per_second,
        returned_blocks: metrics.returned_blocks.max(blocks),
        batch_count: metrics.batch_count,
        fetch_wait_ms: metrics.fetch_wait_ms,
        fetch_rpc_ms: metrics.fetch_rpc_ms,
        rpc_calls: metrics.rpc_calls,
        rpc_request_bytes: metrics.rpc_request_bytes,
        rpc_response_bytes: metrics.rpc_response_bytes,
        rpc_elapsed_ms: metrics.rpc_elapsed_ms,
        rpc_errors: metrics.rpc_errors,
        retries: metrics.retries,
        stalled,
        outcome,
        error,
    }
}

fn failed_result(
    node_label: String,
    profile: &'static str,
    batch: &'static str,
    repetition: usize,
    order: usize,
    started_at_ms: u128,
    start_height: u64,
    elapsed_ms: u128,
    outcome: &'static str,
    error: String,
) -> ProfileResult {
    ProfileResult {
        node_label,
        profile,
        batch,
        repetition,
        order,
        started_at_ms,
        start_height,
        end_height: start_height,
        chain_height: start_height,
        elapsed_ms,
        blocks_per_second: 0.0,
        returned_blocks: 0,
        batch_count: 0,
        fetch_wait_ms: 0,
        fetch_rpc_ms: 0,
        rpc_calls: 0,
        rpc_request_bytes: 0,
        rpc_response_bytes: 0,
        rpc_elapsed_ms: 0,
        rpc_errors: 0,
        retries: 0,
        stalled: false,
        outcome,
        error: Some(error),
    }
}

fn file_len(path: &std::path::Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn read_suffix(path: &std::path::Path, offset: u64) -> String {
    let Ok(bytes) = fs::read(path) else {
        return String::new();
    };
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

fn field_u64(line: &str, key: &str) -> Option<u64> {
    line.split_whitespace()
        .find_map(|field| field.strip_prefix(key)?.parse::<u64>().ok())
}

fn collect_metrics(
    log_path: &std::path::Path,
    log_offset: u64,
    rpc_path: Option<&std::path::Path>,
    rpc_offset: u64,
) -> SampleMetrics {
    let mut metrics = SampleMetrics::default();

    for line in read_suffix(log_path, log_offset).lines() {
        if line.contains("stage=batch_timing") {
            metrics.batch_count = metrics.batch_count.saturating_add(1);
            metrics.returned_blocks = metrics
                .returned_blocks
                .saturating_add(field_u64(line, "blocks=").unwrap_or(0));
            metrics.fetch_wait_ms = metrics
                .fetch_wait_ms
                .saturating_add(u128::from(field_u64(line, "fetch_wait_ms=").unwrap_or(0)));
            metrics.fetch_rpc_ms = metrics
                .fetch_rpc_ms
                .saturating_add(u128::from(field_u64(line, "fetch_rpc_ms=").unwrap_or(0)));
        }
    }

    if let Some(path) = rpc_path {
        for line in read_suffix(path, rpc_offset).lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            match value.get("event").and_then(Value::as_str) {
                Some("rpc_bin") => {
                    metrics.rpc_calls = metrics.rpc_calls.saturating_add(1);
                    metrics.rpc_request_bytes = metrics.rpc_request_bytes.saturating_add(
                        value
                            .get("request_bytes")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    );
                    metrics.rpc_response_bytes = metrics.rpc_response_bytes.saturating_add(
                        value
                            .get("response_bytes")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    );
                    metrics.rpc_elapsed_ms = metrics.rpc_elapsed_ms.saturating_add(u128::from(
                        value.get("elapsed_ms").and_then(Value::as_u64).unwrap_or(0),
                    ));
                    if value.get("error").and_then(Value::as_str).is_some() {
                        metrics.rpc_errors = metrics.rpc_errors.saturating_add(1);
                    }
                }
                Some("retry") => {
                    metrics.retries = metrics.retries.saturating_add(1);
                }
                _ => {}
            }
        }
    }

    metrics
}

fn benchmark_stall_bps() -> f64 {
    std::env::var("NEXAWAL_BENCHMARK_STALL_BPS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 1.0)
        .unwrap_or(DEFAULT_STALL_BPS)
        .min(10_000.0)
}

fn benchmark_config() -> (usize, Duration) {
    let repetitions = std::env::var("NEXAWAL_BENCHMARK_REPETITIONS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_REPETITIONS)
        .clamp(1, 10);
    let seconds = std::env::var("NEXAWAL_BENCHMARK_SECONDS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_WINDOW_SECS)
        .clamp(2, 60);
    (repetitions, Duration::from_secs(seconds))
}

fn shuffled_profiles(run_id: u64, node_index: usize, repetition: usize) -> [&'static str; 5] {
    let mut profiles = PROFILE_NAMES;
    let mut state = run_id
        .wrapping_add(node_index as u64)
        .wrapping_mul(31)
        .wrapping_add(repetition as u64);
    for index in (1..profiles.len()).rev() {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let swap = (state % (index as u64 + 1)) as usize;
        profiles.swap(index, swap);
    }
    profiles
}

fn node_label(url: &str) -> String {
    url.trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

fn wait_for_idle(wallet_id: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !matches!(api::refresh_job(wallet_id), RefreshJob::Running) {
            return;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_targets_include_both_nodes() {
        let targets = targets_for("https://rpc.nexatrode.com");
        assert_eq!(targets.len(), 2);
        assert!(
            targets
                .iter()
                .any(|node| node.contains("rpc.nexatrode.com"))
        );
        assert!(
            targets
                .iter()
                .any(|node| node.contains("monero.nexatrode.com"))
        );
    }

    #[test]
    fn shuffled_order_keeps_all_profiles() {
        for repetition in 0..10 {
            let profiles = shuffled_profiles(42, 0, repetition);
            assert!(profiles.contains(&"fast"));
            assert!(profiles.contains(&"cuprate"));
            assert!(profiles.contains(&"stall"));
            assert!(profiles.contains(&"batch-100"));
            assert!(profiles.contains(&"batch-125"));
        }
    }
}
