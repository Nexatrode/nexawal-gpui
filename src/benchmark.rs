//! Bounded scan benchmarks for comparing WalletCore scan profiles.
//!
//! Each profile gets a fresh in-memory wallet ID derived from the currently opened
//! wallet. The real wallet's checkpoint is never advanced by this diagnostic.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::ProtocolObject;
#[cfg(target_os = "macos")]
use objc2_foundation::{
    NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSProcessInfoThermalState, NSString,
};

use monerowalletcore::api::{self, RefreshJob};
use serde_json::{Value, json};

use crate::{paths, scan_tuning};

const DEFAULT_PROFILE_NAMES: [&str; 8] = [
    "fast",
    "cuprate",
    "batch-150",
    "batch-25",
    "batch-50",
    "batch-75",
    "batch-100",
    "batch-125",
];
const PROFILE_NAMES: [&str; 16] = [
    "fast",
    "cuprate",
    "batch-750",
    "batch-1000",
    "batch-150",
    "batch-25",
    "batch-50",
    "batch-75",
    "batch-100",
    "batch-125",
    "serial-75",
    "parallel-75",
    "decode-serial-75",
    "decode-parallel-75",
    "decode-serial-500",
    "decode-parallel-500",
];
const DEFAULT_REPETITIONS: usize = 3;
const DEFAULT_WINDOW_SECS: u64 = 6;
const DEFAULT_COOLDOWN_SECS: u64 = 5;
// WalletCore allows an individual contiguous block fetch to run for up to 30 seconds. A
// benchmark sample must wait longer than that before starting another wallet, otherwise a slow
// cancellation can turn a sequential comparison into competing background scans.
const CANCEL_WAIT: Duration = Duration::from_secs(45);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_STALL_BPS: f64 = 40.0;
const BENCHMARK_THREAD_NAME: &str = "nexawal-scan-benchmark";

#[derive(Debug)]
struct BenchmarkWorkerContext {
    thread_name: String,
    qos_requested: &'static str,
    qos_effective: &'static str,
    qos_priority: i32,
    qos_set_result: i32,
    activity: &'static str,
}

#[cfg(target_os = "macos")]
struct BenchmarkActivityGuard {
    process_info: Retained<NSProcessInfo>,
    activity: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

#[cfg(target_os = "macos")]
impl Drop for BenchmarkActivityGuard {
    fn drop(&mut self) {
        // SAFETY: `activity` was returned by this exact NSProcessInfo instance's
        // beginActivityWithOptions:reason: call and remains retained until this drop.
        unsafe {
            self.process_info.endActivity(&self.activity);
        }
    }
}

#[cfg(not(target_os = "macos"))]
struct BenchmarkActivityGuard;

#[cfg(target_os = "macos")]
fn qos_name(qos: libc::qos_class_t) -> &'static str {
    use libc::qos_class_t::*;
    match qos {
        QOS_CLASS_USER_INTERACTIVE => "user-interactive",
        QOS_CLASS_USER_INITIATED => "user-initiated",
        QOS_CLASS_DEFAULT => "default",
        QOS_CLASS_UTILITY => "utility",
        QOS_CLASS_BACKGROUND => "background",
        QOS_CLASS_UNSPECIFIED => "unspecified",
    }
}

#[cfg(target_os = "macos")]
fn configure_worker_context() -> (BenchmarkActivityGuard, BenchmarkWorkerContext) {
    let requested_qos = libc::qos_class_t::QOS_CLASS_USER_INITIATED;
    // SAFETY: These pthread functions operate on the current live thread. The class and relative
    // priority are valid values documented by Darwin; output pointers remain valid for the call.
    let qos_set_result = unsafe { libc::pthread_set_qos_class_self_np(requested_qos, 0) };
    let mut effective_qos = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
    let mut qos_priority = 0;
    let qos_get_result = unsafe {
        libc::pthread_get_qos_class_np(libc::pthread_self(), &mut effective_qos, &mut qos_priority)
    };
    if qos_get_result != 0 {
        effective_qos = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
        qos_priority = 0;
    }

    let process_info = NSProcessInfo::processInfo();
    let reason = NSString::from_str("NexaWal scan benchmark");
    let activity =
        process_info.beginActivityWithOptions_reason(NSActivityOptions::UserInitiated, &reason);
    let context = BenchmarkWorkerContext {
        thread_name: thread::current().name().unwrap_or("unnamed").to_string(),
        qos_requested: qos_name(requested_qos),
        qos_effective: qos_name(effective_qos),
        qos_priority,
        qos_set_result,
        activity: "user-initiated",
    };
    (
        BenchmarkActivityGuard {
            process_info,
            activity,
        },
        context,
    )
}

#[cfg(not(target_os = "macos"))]
fn configure_worker_context() -> (BenchmarkActivityGuard, BenchmarkWorkerContext) {
    (
        BenchmarkActivityGuard,
        BenchmarkWorkerContext {
            thread_name: thread::current().name().unwrap_or("unnamed").to_string(),
            qos_requested: "platform-default",
            qos_effective: "platform-default",
            qos_priority: 0,
            qos_set_result: 0,
            activity: "not-required",
        },
    )
}

#[cfg(target_os = "macos")]
fn platform_thermal_state() -> &'static str {
    match NSProcessInfo::processInfo().thermalState() {
        NSProcessInfoThermalState::Nominal => "nominal",
        NSProcessInfoThermalState::Fair => "fair",
        NSProcessInfoThermalState::Serious => "serious",
        NSProcessInfoThermalState::Critical => "critical",
        _ => "unknown",
    }
}

#[cfg(not(target_os = "macos"))]
fn platform_thermal_state() -> &'static str {
    "unavailable"
}

#[cfg(target_os = "macos")]
fn platform_low_power_mode() -> Option<bool> {
    Some(NSProcessInfo::processInfo().isLowPowerModeEnabled())
}

#[cfg(not(target_os = "macos"))]
fn platform_low_power_mode() -> Option<bool> {
    None
}

fn append_benchmark_worker_event(path: &std::path::Path, context: &BenchmarkWorkerContext) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let event = json!({
        "timestamp_ms": now_ms(),
        "event": "benchmark_worker",
        "thread_name": context.thread_name,
        "qos_requested": context.qos_requested,
        "qos_effective": context.qos_effective,
        "qos_priority": context.qos_priority,
        "qos_set_result": context.qos_set_result,
        "activity": context.activity,
        "thermal_state": platform_thermal_state(),
        "low_power_mode": platform_low_power_mode(),
    });
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{event}");
    }
}

#[derive(Debug)]
pub struct BenchmarkReport {
    pub results_path: String,
    pub rpc_results_path: String,
    pub summary: String,
}

#[derive(Debug)]
pub struct SyncAuditReport {
    pub results_path: String,
    pub rpc_results_path: String,
    pub summary: String,
}

#[derive(Debug)]
pub struct SyncTortureAuditReport {
    pub results_path: String,
    pub rpc_results_path: String,
    pub summary: String,
    pub status: String,
}

pub fn run_id() -> u64 {
    now_ms() as u64 ^ u64::from(std::process::id())
}

pub fn spawn_worker(
    node_url: String,
    mnemonic: String,
    start_height: u64,
    run_id: u64,
) -> std::io::Result<thread::JoinHandle<BenchmarkReport>> {
    thread::Builder::new()
        .name(BENCHMARK_THREAD_NAME.to_string())
        .spawn(move || run(node_url, mnemonic, start_height, run_id))
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
    range_decode_events: u64,
    range_decode_blocks: u64,
    range_decode_transactions: u64,
    range_decode_ms: u128,
    range_finalize_ms: u128,
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
    range_decode_events: u64,
    range_decode_blocks: u64,
    range_decode_transactions: u64,
    range_decode_ms: u128,
    range_finalize_ms: u128,
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
    let (_activity_guard, worker_context) = configure_worker_context();
    let results_path = paths::scan_benchmark_path().display().to_string();
    let rpc_telemetry_path = paths::scan_benchmark_rpc_path(run_id);
    let _ = fs::remove_file(&rpc_telemetry_path);
    append_benchmark_worker_event(&rpc_telemetry_path, &worker_context);
    unsafe {
        std::env::set_var("WALLETCORE_RPC_TELEMETRY_PATH", &rpc_telemetry_path);
    }
    let nodes = targets_for(&node_url);
    let profiles = benchmark_profiles();
    let (repetitions, profile_window, cooldown) = benchmark_config();
    let mut results = Vec::with_capacity(profiles.len() * nodes.len() * repetitions);
    let mut sample_count = 0usize;

    // The caller has already requested cancellation. Do not benchmark alongside the real wallet:
    // a cleanup timeout is a failed precondition, not permission to start another scanner.
    if !wait_for_idle("main_wallet", CANCEL_WAIT) {
        unsafe {
            std::env::remove_var("WALLETCORE_RPC_TELEMETRY_PATH");
        }
        scan_tuning::clear_profile_override();
        return BenchmarkReport {
            results_path,
            rpc_results_path: rpc_telemetry_path.display().to_string(),
            summary: format!(
                "0 samples · cleanup-timeout: main wallet did not stop within {} seconds",
                CANCEL_WAIT.as_secs()
            ),
        };
    }

    'benchmark: for (node_index, target) in nodes.iter().enumerate() {
        for repetition in 0..repetitions {
            let profile_order = shuffled_profiles(&profiles, run_id, node_index, repetition);
            for (order, profile_name) in profile_order.iter().enumerate() {
                if sample_count > 0 && !cooldown.is_zero() {
                    thread::sleep(cooldown);
                }
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
                sample_count += 1;
                let abort_suite = result.outcome == "cleanup-timeout";
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
                    "range_decode_events": result.range_decode_events,
                    "range_decode_blocks": result.range_decode_blocks,
                    "range_decode_transactions": result.range_decode_transactions,
                    "range_decode_ms": result.range_decode_ms,
                    "range_finalize_ms": result.range_finalize_ms,
                    "benchmark_thread": worker_context.thread_name,
                    "benchmark_qos_requested": worker_context.qos_requested,
                    "benchmark_qos_effective": worker_context.qos_effective,
                    "benchmark_qos_priority": worker_context.qos_priority,
                    "benchmark_qos_set_result": worker_context.qos_set_result,
                    "benchmark_activity": worker_context.activity,
                    "thermal_state": platform_thermal_state(),
                    "low_power_mode": platform_low_power_mode(),
                    "cooldown_secs": cooldown.as_secs(),
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
                if abort_suite {
                    break 'benchmark;
                }
            }
        }
    }

    unsafe {
        std::env::remove_var("WALLETCORE_RPC_TELEMETRY_PATH");
    }
    scan_tuning::clear_profile_override();

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
        "{} samples · worker {}/{} · {}",
        results.len(),
        worker_context.qos_effective,
        worker_context.activity,
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

/// Scan each selected node to completion and compare the resulting wallet state.
/// A supplied target height is a deterministic comparison ceiling; the scan
/// still finishes at the daemon tip so WalletCore can rebuild and persist its
/// transfer ledger instead of returning a partially-cancelled empty ledger.
/// This is intentionally separate from the short throughput benchmark: a speed
/// sample cannot prove that two nodes produced the same transaction history.
pub fn run_sync_audit(
    node_url: String,
    mnemonic: String,
    start_height: u64,
    target_height: Option<u64>,
    run_id: u64,
) -> SyncAuditReport {
    let results_path = paths::sync_audit_path(run_id);
    let rpc_path = paths::sync_audit_rpc_path(run_id);
    let _ = fs::remove_file(&results_path);
    let _ = fs::remove_file(&rpc_path);
    unsafe {
        std::env::set_var("WALLETCORE_RPC_TELEMETRY_PATH", &rpc_path);
    }

    let timeout = audit_timeout();
    let targets = targets_for(&node_url);
    let mut samples = Vec::with_capacity(targets.len());
    for (index, target) in targets.iter().enumerate() {
        let wallet_id = format!("nexawal-audit-{run_id}-{index}");
        samples.push(run_audit_target(
            &wallet_id,
            target,
            &mnemonic,
            start_height,
            target_height,
            timeout,
            &rpc_path,
        ));
    }

    unsafe {
        std::env::remove_var("WALLETCORE_RPC_TELEMETRY_PATH");
    }
    scan_tuning::clear_profile_override();

    let comparison = compare_audit_targets(&samples, target_height);
    let comparison_status = comparison
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let report = json!({
        "schema": 2,
        "run_id": run_id,
        "started_at_ms": now_ms(),
        "start_height": start_height,
        "target_height": target_height,
        "timeout_secs": timeout.as_secs(),
        "targets": samples,
        "comparison": comparison,
    });
    let report_text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into());
    let _ = fs::create_dir_all(paths::data_dir());
    let write_result = fs::write(&results_path, report_text);
    let summary = if let Err(err) = write_result {
        format!(
            "{} target(s) · {} · could not write report: {err}",
            targets.len(),
            comparison_status
        )
    } else {
        format!(
            "{} target(s) · {} · report {} · RPC trace {}",
            targets.len(),
            comparison_status,
            results_path.display(),
            rpc_path.display()
        )
    };

    SyncAuditReport {
        results_path: results_path.display().to_string(),
        rpc_results_path: rpc_path.display().to_string(),
        summary,
    }
}

/// Exercise the lifecycle that is hardest to cover in an in-process unit test:
/// checkpoint a live scan, kill that process without cleanup, import the checkpoint
/// in this process, survive a dead endpoint, switch node implementations, and finish.
/// Two inverse runs ensure both Cuprate and monerod produce the same final ledger.
pub fn run_sync_torture_audit(
    node_url: String,
    mnemonic: String,
    start_height: u64,
    target_height: Option<u64>,
    run_id: u64,
) -> SyncTortureAuditReport {
    let run_started_at_ms = now_ms();
    let results_path = paths::sync_torture_audit_path(run_id);
    let rpc_path = paths::sync_torture_audit_rpc_path(run_id);
    let _ = fs::remove_file(&results_path);
    let _ = fs::remove_file(&rpc_path);
    unsafe {
        std::env::set_var("WALLETCORE_RPC_TELEMETRY_PATH", &rpc_path);
    }

    let nodes = targets_for(&node_url);
    let timeout = audit_timeout();
    let checkpoint_blocks = torture_checkpoint_blocks();
    let failure_node = std::env::var("NEXAWAL_TORTURE_FAILURE_NODE")
        .unwrap_or_else(|_| "http://127.0.0.1:1".to_string());
    let temporary = tempfile::tempdir();
    let mut samples = Vec::new();
    let mut setup_error = None;

    if nodes.len() < 2 {
        setup_error = Some("the torture audit requires two node targets".to_string());
    } else if let Err(error) = &temporary {
        setup_error = Some(format!(
            "could not create isolated audit directory: {error}"
        ));
    } else if target_height.is_some_and(|target| target <= start_height) {
        setup_error = Some("target height must be above start height".to_string());
    }

    if let Ok(temporary) = temporary
        && setup_error.is_none()
    {
        for (index, (source_node, resume_node)) in [(&nodes[0], &nodes[1]), (&nodes[1], &nodes[0])]
            .into_iter()
            .enumerate()
        {
            let sequence_dir = temporary.path().join(format!("sequence-{index}"));
            let _ = fs::create_dir_all(&sequence_dir);
            let cache_path = sequence_dir.join("main_wallet.cache");
            let state_path = sequence_dir.join("checkpoint.json");
            let worker_id = format!("nexawal-torture-worker-{run_id}-{index}");
            let resumed_id = format!("nexawal-torture-resume-{run_id}-{index}");
            let capture = capture_killed_checkpoint(
                &worker_id,
                source_node,
                &mnemonic,
                start_height,
                checkpoint_blocks,
                &cache_path,
                &state_path,
            );
            samples.push(match capture {
                Ok(interruption) => run_resumed_torture_target(
                    &resumed_id,
                    source_node,
                    resume_node,
                    &failure_node,
                    &mnemonic,
                    start_height,
                    target_height,
                    timeout,
                    &cache_path,
                    interruption,
                ),
                Err(error) => json!({
                    "node": resume_node,
                    "node_label": node_label(resume_node),
                    "source_node": source_node,
                    "wallet_id": resumed_id,
                    "start_height": start_height,
                    "target_height": target_height,
                    "outcome": "interruption-failed",
                    "error": error,
                    "transfers": [],
                    "interruption": {"forced_kill": false},
                    "failure_probe": {"observed": false, "state_preserved": false},
                    "checkpoint_history_preserved": false,
                }),
            });
        }
    }

    unsafe {
        std::env::remove_var("WALLETCORE_RPC_TELEMETRY_PATH");
    }
    scan_tuning::clear_profile_override();

    let comparison = if setup_error.is_none() {
        compare_audit_targets(&samples, target_height)
    } else {
        json!({
            "status": "not-comparable",
            "reason": setup_error,
        })
    };
    let status = torture_audit_status(&samples, &comparison).to_string();
    let report = json!({
        "schema": "nexawal-sync-torture-v1",
        "run_id": run_id,
        "ephemeral_wallet": std::env::var("NEXAWAL_TORTURE_EPHEMERAL")
            .ok()
            .is_some_and(|value| matches!(value.trim(), "1" | "true" | "yes")),
        "started_at_ms": run_started_at_ms,
        "elapsed_ms": now_ms().saturating_sub(run_started_at_ms),
        "start_height": start_height,
        "target_height": target_height,
        "checkpoint_blocks": checkpoint_blocks,
        "failure_node": failure_node,
        "targets": samples,
        "comparison": comparison,
        "status": status,
    });
    let report_text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into());
    let _ = fs::create_dir_all(paths::data_dir());
    let write_result = paths::write_bytes(results_path.clone(), report_text.as_bytes());
    let summary = if let Err(error) = write_result {
        format!("sync torture · {status} · could not write report: {error}")
    } else {
        format!(
            "sync torture · {status} · report {} · RPC trace {}",
            results_path.display(),
            rpc_path.display()
        )
    };

    SyncTortureAuditReport {
        results_path: results_path.display().to_string(),
        rpc_results_path: rpc_path.display().to_string(),
        summary,
        status,
    }
}

/// Hidden child-process entry point used by `run_sync_torture_audit`.
pub fn run_sync_torture_worker() -> Result<(), String> {
    let mnemonic = required_torture_env("NEXAWAL_MNEMONIC")?;
    let wallet_id = required_torture_env("NEXAWAL_TORTURE_WORKER_ID")?;
    let node_url = required_torture_env("NEXAWAL_TORTURE_WORKER_NODE")?;
    let cache_path =
        std::path::PathBuf::from(required_torture_env("NEXAWAL_TORTURE_WORKER_CACHE")?);
    let state_path =
        std::path::PathBuf::from(required_torture_env("NEXAWAL_TORTURE_WORKER_STATE")?);
    let start_height = required_torture_env("NEXAWAL_TORTURE_WORKER_START_HEIGHT")?
        .parse::<u64>()
        .map_err(|error| format!("invalid torture worker start height: {error}"))?;

    api::open_from_mnemonic(&wallet_id, &mnemonic, start_height, true)
        .and_then(|_| api::set_gap_limit(&wallet_id, 50))
        .map_err(|error| format!("torture worker open failed: {error}"))?;
    scan_tuning::clear_profile_override();
    scan_tuning::apply();
    api::refresh_async(&wallet_id, &node_url)
        .map_err(|error| format!("torture worker refresh failed to start: {error}"))?;

    let deadline = Instant::now() + torture_checkpoint_timeout();
    let mut last_exported = None;
    let mut last_export_at = Instant::now() - Duration::from_secs(1);
    while Instant::now() < deadline {
        match api::refresh_job(&wallet_id) {
            RefreshJob::Failed(message) => {
                return Err(format!("torture worker scan failed: {message}"));
            }
            RefreshJob::Running | RefreshJob::Idle => {
                if let Ok(status) = api::sync_status(&wallet_id)
                    && last_exported != Some(status.last_scanned)
                    && last_export_at.elapsed() >= Duration::from_millis(500)
                {
                    let cache = api::export_cache(&wallet_id)
                        .map_err(|error| format!("torture worker cache export failed: {error}"))?;
                    if !cache.is_empty() {
                        paths::write_bytes(cache_path.clone(), &cache).map_err(|error| {
                            format!("torture worker cache persistence failed: {error}")
                        })?;
                        let state = json!({
                            "wallet_id": wallet_id,
                            "node": node_url,
                            "pid": std::process::id(),
                            "last_scanned": status.last_scanned,
                            "chain_height": status.chain_height,
                            "restore_height": status.restore_height,
                            "cache_bytes": cache.len(),
                            "written_at_ms": now_ms(),
                            "refresh_job": match api::refresh_job(&wallet_id) {
                                RefreshJob::Running => "running",
                                RefreshJob::Idle => "idle",
                                RefreshJob::Failed(_) => "failed",
                            },
                        });
                        let encoded = serde_json::to_vec_pretty(&state).map_err(|error| {
                            format!("torture worker state encode failed: {error}")
                        })?;
                        paths::write_bytes(state_path.clone(), &encoded).map_err(|error| {
                            format!("torture worker state persistence failed: {error}")
                        })?;
                        last_exported = Some(status.last_scanned);
                        last_export_at = Instant::now();
                    }
                }
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
    Err(format!(
        "torture worker did not produce a killable checkpoint within {} seconds",
        torture_checkpoint_timeout().as_secs()
    ))
}

fn required_torture_env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("missing {name}"))
}

fn capture_killed_checkpoint(
    wallet_id: &str,
    source_node: &str,
    mnemonic: &str,
    start_height: u64,
    checkpoint_blocks: u64,
    cache_path: &std::path::Path,
    state_path: &std::path::Path,
) -> Result<Value, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not locate torture worker executable: {error}"))?;
    let mut child = Command::new(executable)
        .arg("--sync-torture-worker")
        .env("NEXAWAL_MNEMONIC", mnemonic)
        .env("NEXAWAL_TORTURE_WORKER_ID", wallet_id)
        .env("NEXAWAL_TORTURE_WORKER_NODE", source_node)
        .env(
            "NEXAWAL_TORTURE_WORKER_START_HEIGHT",
            start_height.to_string(),
        )
        .env("NEXAWAL_TORTURE_WORKER_CACHE", cache_path)
        .env("NEXAWAL_TORTURE_WORKER_STATE", state_path)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("could not start torture worker: {error}"))?;
    let worker_pid = child.id();
    let kill_height = start_height.saturating_add(checkpoint_blocks);
    let started = Instant::now();
    let deadline = Instant::now() + torture_checkpoint_timeout();

    while Instant::now() < deadline {
        if let Some(exit) = child
            .try_wait()
            .map_err(|error| format!("could not inspect torture worker: {error}"))?
        {
            return Err(format!(
                "torture worker exited before forced termination: {exit}"
            ));
        }
        let state = fs::read(state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        let checkpoint_ready = state
            .as_ref()
            .and_then(|value| value.get("last_scanned"))
            .and_then(Value::as_u64)
            .is_some_and(|height| height >= kill_height)
            && fs::metadata(cache_path)
                .map(|metadata| metadata.len() > 0)
                .unwrap_or(false);
        if checkpoint_ready {
            child
                .kill()
                .map_err(|error| format!("could not forcibly terminate torture worker: {error}"))?;
            let exit = child
                .wait()
                .map_err(|error| format!("could not reap torture worker: {error}"))?;
            let mut state = state.unwrap_or_else(|| json!({}));
            if let Some(object) = state.as_object_mut() {
                object.insert("forced_kill".into(), Value::Bool(true));
                object.insert("worker_pid".into(), json!(worker_pid));
                object.insert("worker_exit".into(), json!(exit.to_string()));
                object.insert("kill_height".into(), json!(kill_height));
                object.insert(
                    "capture_elapsed_ms".into(),
                    json!(started.elapsed().as_millis()),
                );
            }
            return Ok(state);
        }
        thread::sleep(Duration::from_millis(100));
    }

    let _ = child.kill();
    let _ = child.wait();
    Err(format!(
        "no checkpoint reached height {kill_height} within {} seconds",
        torture_checkpoint_timeout().as_secs()
    ))
}

#[allow(clippy::too_many_arguments)]
fn run_resumed_torture_target(
    wallet_id: &str,
    source_node: &str,
    resume_node: &str,
    failure_node: &str,
    mnemonic: &str,
    start_height: u64,
    target_height: Option<u64>,
    timeout: Duration,
    cache_path: &std::path::Path,
    interruption: Value,
) -> Value {
    let started = Instant::now();
    let mut outcome = "open-failed";
    let mut error = None;
    let cache = match fs::read(cache_path) {
        Ok(cache) if !cache.is_empty() => Some(cache),
        Ok(_) => {
            error = Some("forced-kill checkpoint was empty".to_string());
            None
        }
        Err(read_error) => {
            error = Some(format!(
                "could not read forced-kill checkpoint: {read_error}"
            ));
            None
        }
    };

    if let Some(cache) = cache {
        match api::open_from_mnemonic(wallet_id, mnemonic, start_height, true)
            .and_then(|_| api::set_gap_limit(wallet_id, 50))
            .and_then(|_| api::import_cache(wallet_id, &cache))
        {
            Err(open_error) => error = Some(format!("checkpoint import failed: {open_error}")),
            Ok(()) => {
                outcome = "failure-probe";
            }
        }
    }

    let imported = wallet_state_snapshot(wallet_id, start_height, target_height);
    let failure_probe = if outcome == "failure-probe" {
        run_failure_probe(
            wallet_id,
            failure_node,
            &imported,
            start_height,
            target_height,
        )
    } else {
        json!({"observed": false, "state_preserved": false})
    };

    if outcome == "failure-probe" {
        scan_tuning::clear_profile_override();
        scan_tuning::apply();
        match api::refresh_async(wallet_id, resume_node) {
            Err(refresh_error) => {
                outcome = "resume-start-failed";
                error = Some(refresh_error.to_string());
            }
            Ok(()) => {
                outcome = "timeout";
                let deadline = Instant::now() + timeout;
                while Instant::now() < deadline {
                    match api::refresh_job(wallet_id) {
                        RefreshJob::Failed(message) => {
                            outcome = "scan-failed";
                            error = Some(message);
                            break;
                        }
                        RefreshJob::Running => {}
                        RefreshJob::Idle => {
                            if let Ok(status) = api::sync_status(wallet_id)
                                && status.chain_height > start_height
                                && status.last_scanned >= status.chain_height
                            {
                                outcome = "completed";
                                break;
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(250));
                }
                if outcome == "timeout" {
                    let _ = api::refresh_cancel(wallet_id);
                }
                if !wait_for_idle(wallet_id, CANCEL_WAIT) {
                    outcome = "cleanup-timeout";
                    error = Some(format!(
                        "resumed refresh did not stop within {} seconds",
                        CANCEL_WAIT.as_secs()
                    ));
                }
            }
        }
    }

    let mut final_state = wallet_state_snapshot(wallet_id, start_height, target_height);
    let checkpoint_history_preserved = canonical_history_is_subset(&imported, &final_state);
    if let Some(object) = final_state.as_object_mut() {
        object.insert("node".into(), json!(resume_node));
        object.insert("node_label".into(), json!(node_label(resume_node)));
        object.insert("source_node".into(), json!(source_node));
        object.insert("wallet_id".into(), json!(wallet_id));
        object.insert("outcome".into(), json!(outcome));
        object.insert("error".into(), json!(error));
        object.insert("elapsed_ms".into(), json!(started.elapsed().as_millis()));
        object.insert("interruption".into(), interruption);
        object.insert("failure_probe".into(), failure_probe);
        object.insert(
            "checkpoint_history_preserved".into(),
            json!(checkpoint_history_preserved),
        );
    }
    final_state
}

fn run_failure_probe(
    wallet_id: &str,
    failure_node: &str,
    before: &Value,
    start_height: u64,
    target_height: Option<u64>,
) -> Value {
    let started = Instant::now();
    let mut observed = false;
    let mut detail = None;
    match api::refresh_async(wallet_id, failure_node) {
        Err(error) => {
            observed = true;
            detail = Some(error.to_string());
        }
        Ok(()) => {
            let deadline = Instant::now() + torture_failure_timeout();
            while Instant::now() < deadline {
                match api::refresh_job(wallet_id) {
                    RefreshJob::Failed(message) => {
                        observed = true;
                        detail = Some(message);
                        break;
                    }
                    RefreshJob::Idle => {
                        observed = true;
                        detail = Some("refresh returned idle for unreachable endpoint".into());
                        break;
                    }
                    RefreshJob::Running => thread::sleep(POLL_INTERVAL),
                }
            }
        }
    }
    if matches!(api::refresh_job(wallet_id), RefreshJob::Running) {
        let _ = api::refresh_cancel(wallet_id);
    }
    let stopped = wait_for_idle(wallet_id, CANCEL_WAIT);
    let after = wallet_state_snapshot(wallet_id, start_height, target_height);
    let state_preserved = stopped
        && sample_last_scanned(before) == sample_last_scanned(&after)
        && before.get("balance_total_piconero") == after.get("balance_total_piconero")
        && before.get("balance_unlocked_piconero") == after.get("balance_unlocked_piconero")
        && canonical_histories_equal(before, &after)
        && api::export_cache(wallet_id).is_ok_and(|cache| !cache.is_empty());
    json!({
        "node": failure_node,
        "observed": observed,
        "detail": detail,
        "stopped": stopped,
        "state_preserved": state_preserved,
        "elapsed_ms": started.elapsed().as_millis(),
        "before_last_scanned": sample_last_scanned(before),
        "after_last_scanned": sample_last_scanned(&after),
    })
}

fn wallet_state_snapshot(wallet_id: &str, start_height: u64, target_height: Option<u64>) -> Value {
    let sync = api::sync_status(wallet_id).ok();
    let balance = api::get_balance(wallet_id).ok();
    let transfers = api::list_transfers(wallet_id).unwrap_or_default();
    let transfer_values: Vec<Value> = transfers
        .iter()
        .map(|transfer| {
            json!({
                "txid": transfer.txid,
                "direction": transfer.direction,
                "amount_piconero": transfer.amount,
                "fee_piconero": transfer.fee,
                "height": transfer.height,
                "timestamp": transfer.timestamp,
                "confirmations": transfer.confirmations,
                "is_pending": transfer.is_pending,
            })
        })
        .collect();
    let fee_comparison_height = target_height.unwrap_or_else(|| {
        sync.as_ref()
            .map(|status| status.last_scanned)
            .unwrap_or(start_height)
    });
    let missing_fee_txids = missing_confirmed_fee_txids(&transfer_values, fee_comparison_height);
    json!({
        "start_height": start_height,
        "target_height": target_height,
        "sync": sync.map(|status| json!({
            "chain_height": status.chain_height,
            "last_scanned": status.last_scanned,
            "restore_height": status.restore_height,
            "chain_time": status.chain_time,
        })),
        "balance_total_piconero": balance.as_ref().map(|value| value.total_piconero),
        "balance_unlocked_piconero": balance.as_ref().map(|value| value.unlocked_piconero),
        "transfer_count": transfer_values.len(),
        "fees_complete": missing_fee_txids.is_empty(),
        "missing_fee_count": missing_fee_txids.len(),
        "missing_fee_txids": missing_fee_txids,
        "transfers": transfer_values,
    })
}

fn canonical_transfer(value: &Value) -> Value {
    json!({
        "direction": value.get("direction"),
        "amount_piconero": value.get("amount_piconero"),
        "fee_piconero": value.get("fee_piconero"),
        "height": value.get("height"),
        "is_pending": value.get("is_pending"),
    })
}

fn canonical_history(sample: &Value) -> BTreeMap<String, Value> {
    sample
        .get("transfers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|transfer| {
            transfer
                .get("txid")
                .and_then(Value::as_str)
                .map(|txid| (txid.to_string(), canonical_transfer(transfer)))
        })
        .collect()
}

fn canonical_histories_equal(left: &Value, right: &Value) -> bool {
    canonical_history(left) == canonical_history(right)
}

fn canonical_history_is_subset(checkpoint: &Value, final_state: &Value) -> bool {
    let checkpoint = canonical_history(checkpoint);
    let final_state = canonical_history(final_state);
    checkpoint
        .iter()
        .all(|(txid, transfer)| final_state.get(txid) == Some(transfer))
}

fn torture_audit_status<'a>(samples: &[Value], comparison: &'a Value) -> &'a str {
    let comparison_status = comparison
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if comparison_status != "pass" {
        return comparison_status;
    }
    let lifecycle_passed = samples.iter().all(|sample| {
        sample
            .get("interruption")
            .and_then(|value| value.get("forced_kill"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && sample
                .get("failure_probe")
                .and_then(|value| value.get("observed"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            && sample
                .get("failure_probe")
                .and_then(|value| value.get("state_preserved"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            && sample
                .get("checkpoint_history_preserved")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    });
    if lifecycle_passed { "pass" } else { "fail" }
}

fn torture_checkpoint_blocks() -> u64 {
    std::env::var("NEXAWAL_TORTURE_CHECKPOINT_BLOCKS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(20_000)
        .clamp(500, 1_000_000)
}

fn torture_checkpoint_timeout() -> Duration {
    let seconds = std::env::var("NEXAWAL_TORTURE_CHECKPOINT_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(900)
        .clamp(60, 7_200);
    Duration::from_secs(seconds)
}

fn torture_failure_timeout() -> Duration {
    let seconds = std::env::var("NEXAWAL_TORTURE_FAILURE_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(45)
        .clamp(5, 300);
    Duration::from_secs(seconds)
}

fn run_audit_target(
    wallet_id: &str,
    node_url: &str,
    mnemonic: &str,
    start_height: u64,
    target_height: Option<u64>,
    timeout: Duration,
    rpc_path: &std::path::Path,
) -> Value {
    let started_at_ms = now_ms();
    let started = Instant::now();
    let log_path = paths::walletcore_log_path(wallet_id);
    let log_offset = file_len(&log_path);
    let rpc_offset = file_len(rpc_path);
    let mut outcome = "open-failed";
    let mut error = None;

    if target_height.is_some_and(|target| target <= start_height) {
        error = Some("target height must be above start height".to_string());
    } else {
        // The FFI keeps a process-wide last-error slot. Clear a previous
        // target's message before an open attempt so a raw validation error
        // (for example an invalid mnemonic) cannot inherit stale text.
        let _ = api::last_error();
        match api::open_from_mnemonic(wallet_id, mnemonic, start_height, true)
            .and_then(|_| api::set_gap_limit(wallet_id, 50))
        {
            Err(err) => error = Some(err.to_string()),
            Ok(()) => {
                // An audit must use the production default rather than an
                // accidentally inherited diagnostic profile.
                scan_tuning::clear_profile_override();
                scan_tuning::apply();
                match api::refresh_async(wallet_id, node_url) {
                    Err(err) => {
                        outcome = "start-failed";
                        error = Some(err.to_string());
                    }
                    Ok(()) => {
                        outcome = "timeout";
                        let deadline = Instant::now() + timeout;
                        while Instant::now() < deadline {
                            match api::refresh_job(wallet_id) {
                                RefreshJob::Failed(message) => {
                                    outcome = "scan-failed";
                                    error = Some(message);
                                    break;
                                }
                                RefreshJob::Running => {}
                                RefreshJob::Idle => {
                                    if let Ok(status) = api::sync_status(wallet_id) {
                                        let reached_tip = status.chain_height > start_height
                                            && status.last_scanned >= status.chain_height;
                                        if reached_tip {
                                            outcome = "completed";
                                            break;
                                        }
                                    }
                                }
                            }
                            thread::sleep(Duration::from_millis(250));
                        }
                        if outcome == "timeout" {
                            let _ = api::refresh_cancel(wallet_id);
                        }
                        if !wait_for_idle(wallet_id, CANCEL_WAIT) {
                            outcome = "cleanup-timeout";
                            error = Some(format!(
                                "wallet refresh did not stop within {} seconds",
                                CANCEL_WAIT.as_secs()
                            ));
                        }
                    }
                }
            }
        }
    }

    let sync = api::sync_status(wallet_id).ok();
    let balance = api::get_balance(wallet_id).ok();
    let transfers = api::list_transfers(wallet_id).unwrap_or_default();
    let metrics = collect_metrics(&log_path, log_offset, Some(rpc_path), rpc_offset);
    let transfer_values: Vec<Value> = transfers
        .iter()
        .map(|transfer| {
            json!({
                "txid": transfer.txid,
                "direction": transfer.direction,
                "amount_piconero": transfer.amount,
                "fee_piconero": transfer.fee,
                "height": transfer.height,
                "timestamp": transfer.timestamp,
                "confirmations": transfer.confirmations,
                "is_pending": transfer.is_pending,
            })
        })
        .collect();
    let fee_comparison_height = target_height.unwrap_or_else(|| {
        sync.as_ref()
            .map(|status| status.last_scanned)
            .unwrap_or(start_height)
    });
    let missing_fee_txids = missing_confirmed_fee_txids(&transfer_values, fee_comparison_height);

    json!({
        "node": node_url,
        "node_label": node_label(node_url),
        "wallet_id": wallet_id,
        "started_at_ms": started_at_ms,
        "elapsed_ms": started.elapsed().as_millis(),
        "start_height": start_height,
        "target_height": target_height,
        "outcome": outcome,
        "error": error,
        "sync": sync.map(|status| json!({
            "chain_height": status.chain_height,
            "last_scanned": status.last_scanned,
            "restore_height": status.restore_height,
            "chain_time": status.chain_time,
        })),
        "balance_total_piconero": balance.as_ref().map(|value| value.total_piconero),
        "balance_unlocked_piconero": balance.as_ref().map(|value| value.unlocked_piconero),
        "transfer_count": transfer_values.len(),
        "fees_complete": missing_fee_txids.is_empty(),
        "missing_fee_count": missing_fee_txids.len(),
        "missing_fee_txids": missing_fee_txids,
        "transfers": transfer_values,
        "rpc_calls": metrics.rpc_calls,
        "rpc_request_bytes": metrics.rpc_request_bytes,
        "rpc_response_bytes": metrics.rpc_response_bytes,
        "rpc_elapsed_ms": metrics.rpc_elapsed_ms,
        "rpc_errors": metrics.rpc_errors,
        "retries": metrics.retries,
    })
}

fn compare_audit_targets(samples: &[Value], target_height: Option<u64>) -> Value {
    if samples.len() < 2 {
        return json!({
            "status": "not-comparable",
            "reason": "at least two node targets are required",
        });
    }

    let left = &samples[0];
    let right = &samples[1];
    let left_last = sample_last_scanned(left);
    let right_last = sample_last_scanned(right);
    let common_height = target_height.unwrap_or_else(|| left_last.min(right_last));
    let left_transfers = audit_transfer_map(left, common_height);
    let right_transfers = audit_transfer_map(right, common_height);
    let left_missing_fees = missing_confirmed_fee_txids_from_map(&left_transfers);
    let right_missing_fees = missing_confirmed_fee_txids_from_map(&right_transfers);
    let left_ids: std::collections::BTreeSet<_> = left_transfers.keys().cloned().collect();
    let right_ids: std::collections::BTreeSet<_> = right_transfers.keys().cloned().collect();
    let missing_from_right: Vec<_> = left_ids.difference(&right_ids).cloned().collect();
    let missing_from_left: Vec<_> = right_ids.difference(&left_ids).cloned().collect();
    let mut mismatches = Vec::new();
    for txid in left_ids.intersection(&right_ids) {
        let a = &left_transfers[txid];
        let b = &right_transfers[txid];
        let fields = ["direction", "amount_piconero", "fee_piconero", "height"];
        let different: Vec<_> = fields
            .iter()
            .filter(|field| a.get(**field) != b.get(**field))
            .copied()
            .collect();
        if !different.is_empty() {
            mismatches.push(json!({
                "txid": txid,
                "fields": different,
                "left": a,
                "right": b,
            }));
        }
    }

    let complete = [left, right].iter().all(|sample| {
        matches!(
            sample.get("outcome").and_then(Value::as_str),
            Some("completed") | Some("target-reached")
        )
    });
    let balance_equal = target_height.is_some()
        && left
            .get("balance_total_piconero")
            .and_then(Value::as_u64)
            .zip(right.get("balance_total_piconero").and_then(Value::as_u64))
            .is_some_and(|(left_total, right_total)| left_total == right_total)
        && left
            .get("balance_unlocked_piconero")
            .and_then(Value::as_u64)
            .zip(
                right
                    .get("balance_unlocked_piconero")
                    .and_then(Value::as_u64),
            )
            .is_some_and(|(left_unlocked, right_unlocked)| left_unlocked == right_unlocked);
    let history_equal =
        missing_from_right.is_empty() && missing_from_left.is_empty() && mismatches.is_empty();
    let fees_complete = left_missing_fees.is_empty() && right_missing_fees.is_empty();
    let status = if !complete {
        "incomplete"
    } else if !history_equal || !fees_complete || (target_height.is_some() && !balance_equal) {
        "fail"
    } else {
        "pass"
    };

    json!({
        "status": status,
        "common_height": common_height,
        "left_node": left.get("node"),
        "right_node": right.get("node"),
        "history_equal": history_equal,
        "fees_complete": fees_complete,
        "left_missing_fee_count": left_missing_fees.len(),
        "left_missing_fee_txids": left_missing_fees,
        "right_missing_fee_count": right_missing_fees.len(),
        "right_missing_fee_txids": right_missing_fees,
        "balance_equal_at_target": if target_height.is_some() { Some(balance_equal) } else { None::<bool> },
        "left_transfer_count": left_transfers.len(),
        "right_transfer_count": right_transfers.len(),
        "missing_from_right_count": missing_from_right.len(),
        "missing_from_right": missing_from_right.into_iter().take(50).collect::<Vec<_>>(),
        "missing_from_left_count": missing_from_left.len(),
        "missing_from_left": missing_from_left.into_iter().take(50).collect::<Vec<_>>(),
        "mismatch_count": mismatches.len(),
        "mismatches": mismatches.into_iter().take(50).collect::<Vec<_>>(),
    })
}

fn missing_confirmed_fee_txids(transfers: &[Value], max_height: u64) -> Vec<String> {
    transfers
        .iter()
        .filter(|transfer| {
            !transfer
                .get("is_pending")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter(|transfer| {
            transfer
                .get("height")
                .and_then(Value::as_u64)
                .is_some_and(|height| height <= max_height)
        })
        .filter(|transfer| transfer.get("fee_piconero").is_none_or(Value::is_null))
        .filter_map(|transfer| {
            transfer
                .get("txid")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect()
}

fn missing_confirmed_fee_txids_from_map(
    transfers: &std::collections::BTreeMap<String, Value>,
) -> Vec<String> {
    transfers
        .iter()
        .filter(|(_, transfer)| {
            !transfer
                .get("is_pending")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter(|(_, transfer)| transfer.get("height").and_then(Value::as_u64).is_some())
        .filter(|(_, transfer)| transfer.get("fee_piconero").is_none_or(Value::is_null))
        .map(|(txid, _)| txid.clone())
        .collect()
}

fn audit_transfer_map(
    sample: &Value,
    max_height: u64,
) -> std::collections::BTreeMap<String, Value> {
    sample
        .get("transfers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|transfer| {
            transfer
                .get("height")
                .and_then(Value::as_u64)
                .is_none_or(|height| height <= max_height)
        })
        .filter_map(|transfer| {
            transfer
                .get("txid")
                .and_then(Value::as_str)
                .map(|txid| (txid.to_string(), transfer.clone()))
        })
        .collect()
}

fn sample_last_scanned(sample: &Value) -> u64 {
    sample
        .get("sync")
        .and_then(Value::as_object)
        .and_then(|sync| sync.get("last_scanned"))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            sample
                .get("start_height")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        })
}

fn audit_timeout() -> Duration {
    let seconds = std::env::var("NEXAWAL_AUDIT_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(3_600)
        .clamp(60, 86_400);
    Duration::from_secs(seconds)
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
    if !wait_for_idle(wallet_id, CANCEL_WAIT) {
        outcome = "cleanup-timeout";
        error = Some(format!(
            "wallet refresh did not stop within {} seconds; remaining benchmark samples were not started",
            CANCEL_WAIT.as_secs()
        ));
    }

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
        range_decode_events: metrics.range_decode_events,
        range_decode_blocks: metrics.range_decode_blocks,
        range_decode_transactions: metrics.range_decode_transactions,
        range_decode_ms: metrics.range_decode_ms,
        range_finalize_ms: metrics.range_finalize_ms,
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
        range_decode_events: 0,
        range_decode_blocks: 0,
        range_decode_transactions: 0,
        range_decode_ms: 0,
        range_finalize_ms: 0,
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
                Some("range_decode") => {
                    metrics.range_decode_events = metrics.range_decode_events.saturating_add(1);
                    metrics.range_decode_blocks = metrics
                        .range_decode_blocks
                        .saturating_add(value.get("blocks").and_then(Value::as_u64).unwrap_or(0));
                    metrics.range_decode_transactions =
                        metrics.range_decode_transactions.saturating_add(
                            value
                                .get("transactions")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                        );
                    metrics.range_decode_ms = metrics.range_decode_ms.saturating_add(u128::from(
                        value.get("decode_ms").and_then(Value::as_u64).unwrap_or(0),
                    ));
                    metrics.range_finalize_ms =
                        metrics.range_finalize_ms.saturating_add(u128::from(
                            value
                                .get("finalize_ms")
                                .and_then(Value::as_u64)
                                .unwrap_or(0),
                        ));
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

fn benchmark_profiles() -> Vec<&'static str> {
    let Some(raw) = std::env::var("NEXAWAL_BENCHMARK_PROFILES").ok() else {
        return DEFAULT_PROFILE_NAMES.to_vec();
    };

    let selected = raw
        .split(',')
        .map(str::trim)
        .filter_map(|name| {
            PROFILE_NAMES
                .iter()
                .copied()
                .find(|profile| *profile == name)
        })
        .fold(Vec::new(), |mut profiles, profile| {
            if !profiles.contains(&profile) {
                profiles.push(profile);
            }
            profiles
        });

    if selected.is_empty() {
        DEFAULT_PROFILE_NAMES.to_vec()
    } else {
        selected
    }
}

fn benchmark_config() -> (usize, Duration, Duration) {
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
    let cooldown_secs = std::env::var("NEXAWAL_BENCHMARK_COOLDOWN_SECS")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_COOLDOWN_SECS)
        .clamp(0, 60);
    (
        repetitions,
        Duration::from_secs(seconds),
        Duration::from_secs(cooldown_secs),
    )
}

fn shuffled_profiles(
    profiles: &[&'static str],
    run_id: u64,
    node_index: usize,
    repetition: usize,
) -> Vec<&'static str> {
    let mut profiles = profiles.to_vec();
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

fn wait_for_idle(wallet_id: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !matches!(api::refresh_job(wallet_id), RefreshJob::Running) {
            return true;
        }
        thread::sleep(POLL_INTERVAL);
    }
    !matches!(api::refresh_job(wallet_id), RefreshJob::Running)
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

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_benchmark_worker_uses_user_initiated_qos_and_activity() {
        let context = thread::Builder::new()
            .name(BENCHMARK_THREAD_NAME.to_string())
            .spawn(|| {
                let (_activity_guard, context) = configure_worker_context();
                context
            })
            .expect("benchmark worker")
            .join()
            .expect("benchmark worker should not panic");

        assert_eq!(context.thread_name, BENCHMARK_THREAD_NAME);
        assert_eq!(context.qos_set_result, 0);
        assert_eq!(context.qos_requested, "user-initiated");
        assert_eq!(context.qos_effective, "user-initiated");
        assert_eq!(context.activity, "user-initiated");
    }

    #[test]
    fn idle_wait_reports_success_without_sleeping() {
        assert!(wait_for_idle(
            "nexawal-benchmark-never-started",
            Duration::ZERO
        ));
    }

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
            let profiles = shuffled_profiles(&PROFILE_NAMES, 42, 0, repetition);
            assert!(profiles.contains(&"fast"));
            assert!(profiles.contains(&"cuprate"));
            assert!(profiles.contains(&"batch-750"));
            assert!(profiles.contains(&"batch-1000"));
            assert!(profiles.contains(&"batch-150"));
            assert!(profiles.contains(&"batch-25"));
            assert!(profiles.contains(&"batch-50"));
            assert!(profiles.contains(&"batch-75"));
            assert!(profiles.contains(&"batch-100"));
            assert!(profiles.contains(&"batch-125"));
            assert!(profiles.contains(&"serial-75"));
            assert!(profiles.contains(&"parallel-75"));
            assert!(profiles.contains(&"decode-serial-75"));
            assert!(profiles.contains(&"decode-parallel-75"));
            assert!(profiles.contains(&"decode-serial-500"));
            assert!(profiles.contains(&"decode-parallel-500"));
        }
    }

    #[test]
    fn sync_audit_comparison_passes_matching_history() {
        let transfer = json!({
            "txid": "abc",
            "direction": "in",
            "amount_piconero": 42,
            "fee_piconero": 0,
            "height": 100,
        });
        let samples = vec![
            json!({
                "node": "one",
                "outcome": "target-reached",
                "sync": {"last_scanned": 100},
                "balance_total_piconero": 42,
                "balance_unlocked_piconero": 42,
                "transfers": [transfer.clone()],
            }),
            json!({
                "node": "two",
                "outcome": "target-reached",
                "sync": {"last_scanned": 100},
                "balance_total_piconero": 42,
                "balance_unlocked_piconero": 42,
                "transfers": [transfer],
            }),
        ];
        let comparison = compare_audit_targets(&samples, Some(100));
        assert_eq!(
            comparison.get("status").and_then(Value::as_str),
            Some("pass")
        );
    }

    #[test]
    fn sync_audit_comparison_detects_missing_history() {
        let samples = vec![
            json!({
                "node": "one",
                "outcome": "target-reached",
                "sync": {"last_scanned": 100},
                "balance_total_piconero": 42,
                "balance_unlocked_piconero": 42,
                "transfers": [{
                    "txid": "abc",
                    "direction": "in",
                    "amount_piconero": 42,
                    "fee_piconero": 0,
                    "height": 100,
                }],
            }),
            json!({
                "node": "two",
                "outcome": "target-reached",
                "sync": {"last_scanned": 100},
                "balance_total_piconero": 0,
                "balance_unlocked_piconero": 0,
                "transfers": [],
            }),
        ];
        let comparison = compare_audit_targets(&samples, Some(100));
        assert_eq!(
            comparison.get("status").and_then(Value::as_str),
            Some("fail")
        );
        assert_eq!(
            comparison
                .get("missing_from_right_count")
                .and_then(Value::as_u64),
            Some(1)
        );
    }

    #[test]
    fn sync_audit_comparison_fails_confirmed_transfer_without_fee() {
        let transfer = json!({
            "txid": "abc",
            "direction": "in",
            "amount_piconero": 42,
            "fee_piconero": null,
            "height": 100,
            "is_pending": false,
        });
        let samples = vec![
            json!({
                "node": "one",
                "outcome": "target-reached",
                "sync": {"last_scanned": 100},
                "balance_total_piconero": 42,
                "balance_unlocked_piconero": 42,
                "transfers": [transfer.clone()],
            }),
            json!({
                "node": "two",
                "outcome": "target-reached",
                "sync": {"last_scanned": 100},
                "balance_total_piconero": 42,
                "balance_unlocked_piconero": 42,
                "transfers": [transfer],
            }),
        ];
        let comparison = compare_audit_targets(&samples, Some(100));
        assert_eq!(
            comparison.get("status").and_then(Value::as_str),
            Some("fail")
        );
        assert_eq!(
            comparison.get("fees_complete").and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            comparison
                .get("left_missing_fee_count")
                .and_then(Value::as_u64),
            Some(1)
        );
    }

    #[test]
    fn torture_status_requires_kill_failure_preservation_and_history() {
        let passing_sample = json!({
            "interruption": {"forced_kill": true},
            "failure_probe": {"observed": true, "state_preserved": true},
            "checkpoint_history_preserved": true,
        });
        let comparison = json!({"status": "pass"});
        assert_eq!(
            torture_audit_status(
                &[passing_sample.clone(), passing_sample.clone()],
                &comparison
            ),
            "pass"
        );

        let mut failed_sample = passing_sample;
        failed_sample["failure_probe"]["state_preserved"] = Value::Bool(false);
        assert_eq!(
            torture_audit_status(
                &[
                    failed_sample,
                    json!({
                        "interruption": {"forced_kill": true},
                        "failure_probe": {"observed": true, "state_preserved": true},
                        "checkpoint_history_preserved": true,
                    })
                ],
                &comparison
            ),
            "fail"
        );
    }

    #[test]
    fn checkpoint_history_ignores_confirmation_growth_but_not_missing_transactions() {
        let checkpoint = json!({
            "transfers": [{
                "txid": "abc",
                "direction": "in",
                "amount_piconero": 42,
                "fee_piconero": 3,
                "height": 100,
                "confirmations": 1,
                "is_pending": false,
            }]
        });
        let final_state = json!({
            "transfers": [{
                "txid": "abc",
                "direction": "in",
                "amount_piconero": 42,
                "fee_piconero": 3,
                "height": 100,
                "confirmations": 99,
                "is_pending": false,
            }]
        });
        assert!(canonical_history_is_subset(&checkpoint, &final_state));
        assert!(!canonical_history_is_subset(
            &checkpoint,
            &json!({"transfers": []})
        ));
    }
}
