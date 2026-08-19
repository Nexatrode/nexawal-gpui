//! Bounded scan benchmarks for comparing WalletCore scan profiles.
//!
//! Each profile gets a fresh in-memory wallet ID derived from the currently opened
//! wallet. The real wallet's checkpoint is never advanced by this diagnostic.

use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use monerowalletcore::api::{self, RefreshJob};
use serde_json::json;

use crate::{paths, scan_tuning};

const PROFILE_NAMES: [&str; 3] = ["fast", "cuprate", "stall"];
const PROFILE_WINDOW: Duration = Duration::from_secs(10);
const CANCEL_WAIT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub struct BenchmarkReport {
    pub results_path: String,
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
    started_at_ms: u128,
    start_height: u64,
    end_height: u64,
    chain_height: u64,
    elapsed_ms: u128,
    blocks_per_second: f64,
    outcome: &'static str,
    error: Option<String>,
}

pub fn run(node_url: String, mnemonic: String, start_height: u64, run_id: u64) -> BenchmarkReport {
    let results_path = paths::scan_benchmark_path().display().to_string();
    let nodes = targets_for(&node_url);
    let mut results = Vec::with_capacity(PROFILE_NAMES.len() * nodes.len());

    // The caller has already requested cancellation. Give WalletCore a short grace
    // period so the benchmark does not compete with the user's prior scan.
    wait_for_idle("main_wallet", CANCEL_WAIT);

    for (node_index, target) in nodes.iter().enumerate() {
        for (profile_index, profile_name) in PROFILE_NAMES.iter().enumerate() {
            let wallet_id = format!("nexawal-benchmark-{run_id}-{node_index}-{profile_index}");
            let result = run_profile(&wallet_id, profile_name, target, &mnemonic, start_height);
            let line = json!({
                "timestamp_ms": result.started_at_ms,
                "node": target,
                "profile": result.profile,
                "batch": result.batch,
                "start_height": result.start_height,
                "end_height": result.end_height,
                "chain_height": result.chain_height,
                "elapsed_ms": result.elapsed_ms,
                "blocks": result.end_height.saturating_sub(result.start_height),
                "blocks_per_second": result.blocks_per_second,
                "outcome": result.outcome,
                "error": result.error,
            });
            let _ = paths::append_scan_benchmark(&line.to_string());
            results.push(result);
        }
    }

    let summary = results
        .iter()
        .map(|result| {
            format!(
                "{} {} ({}) {:.1} blocks/s",
                result.node_label, result.profile, result.batch, result.blocks_per_second
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");

    BenchmarkReport {
        results_path,
        summary,
    }
}

fn run_profile(
    wallet_id: &str,
    profile_name: &'static str,
    node_url: &str,
    mnemonic: &str,
    start_height: u64,
) -> ProfileResult {
    let started_at_ms = now_ms();
    let started = Instant::now();
    let node_label = node_label(node_url);
    let batch = scan_tuning::batch_for(profile_name).unwrap_or("unknown");

    let open_result = api::open_from_mnemonic(wallet_id, mnemonic, start_height, true)
        .and_then(|_| api::set_gap_limit(wallet_id, 50));
    if let Err(err) = open_result {
        return ProfileResult {
            node_label,
            profile: profile_name,
            batch,
            started_at_ms,
            start_height,
            end_height: start_height,
            chain_height: start_height,
            elapsed_ms: started.elapsed().as_millis(),
            blocks_per_second: 0.0,
            outcome: "open-failed",
            error: Some(err.to_string()),
        };
    }

    let _ = scan_tuning::apply_named(profile_name);
    if let Err(err) = api::refresh_async(wallet_id, node_url) {
        return ProfileResult {
            node_label,
            profile: profile_name,
            batch,
            started_at_ms,
            start_height,
            end_height: start_height,
            chain_height: start_height,
            elapsed_ms: started.elapsed().as_millis(),
            blocks_per_second: 0.0,
            outcome: "start-failed",
            error: Some(err.to_string()),
        };
    }

    let sample_started = Instant::now();
    let deadline = sample_started + PROFILE_WINDOW;
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

    let blocks = last_scanned.saturating_sub(start_height);
    let blocks_per_second = if elapsed_ms == 0 {
        0.0
    } else {
        blocks as f64 / (elapsed_ms as f64 / 1_000.0)
    };

    ProfileResult {
        node_label,
        profile: profile_name,
        batch,
        started_at_ms,
        start_height,
        end_height: last_scanned,
        chain_height,
        elapsed_ms,
        blocks_per_second,
        outcome,
        error,
    }
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
