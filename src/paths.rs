use std::fs;
use std::path::PathBuf;

pub const CURRENT_TERMS_VERSION: u32 = 1;
pub const DEFAULT_NODE: &str = "https://rpc.nexatrode.com";

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nexawal")
}

pub fn cache_path() -> PathBuf {
    data_dir().join("main_wallet.cache")
}

pub fn terms_path() -> PathBuf {
    data_dir().join("terms_version")
}

pub fn node_path() -> PathBuf {
    data_dir().join("node_url")
}

pub fn pending_send_path() -> PathBuf {
    data_dir().join("pending_send.json")
}

pub fn wallet_slot_path() -> PathBuf {
    data_dir().join("wallet_slot")
}

pub fn restore_height_path() -> PathBuf {
    data_dir().join("restore_height")
}

pub fn fiat_enabled_path() -> PathBuf {
    data_dir().join("fiat_estimates_enabled")
}

pub fn fiat_currency_path() -> PathBuf {
    data_dir().join("fiat_currency")
}

pub fn fiat_rate_path() -> PathBuf {
    data_dir().join("fiat_rate")
}

pub fn receive_book_path() -> PathBuf {
    data_dir().join("receive_subaddresses")
}

pub fn device_auth_path() -> PathBuf {
    data_dir().join("require_device_auth")
}

pub fn network_policy_path() -> PathBuf {
    data_dir().join("network_policy")
}

pub fn i2p_rpc_path() -> PathBuf {
    data_dir().join("i2p_rpc")
}

pub fn i2p_proxy_path() -> PathBuf {
    data_dir().join("i2p_proxy")
}

pub fn fiat_opted_in_path() -> PathBuf {
    data_dir().join("fiat_estimates_enabled_at")
}

pub fn fiat_snapshots_path() -> PathBuf {
    data_dir().join("fiat_tx_snapshots")
}

pub fn fiat_observed_path() -> PathBuf {
    data_dir().join("fiat_tx_observed")
}

pub fn sync_details_expanded_path() -> PathBuf {
    data_dir().join("ui_sync_details_expanded")
}

pub fn scan_benchmark_path() -> PathBuf {
    data_dir().join("scan_benchmarks.jsonl")
}

pub fn scan_benchmark_rpc_path(run_id: u64) -> PathBuf {
    data_dir().join(format!("scan_benchmark_rpc_{run_id}.jsonl"))
}

/// WalletCore's per-wallet diagnostic log location for a mainnet benchmark wallet.
pub fn walletcore_log_path(wallet_id: &str) -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WalletCaches")
        .join("mainnet")
        .join(format!("{wallet_id}.walletcore.log"))
}

pub fn append_scan_benchmark(line: &str) -> std::io::Result<()> {
    use std::io::Write;

    fs::create_dir_all(data_dir())?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(scan_benchmark_path())?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")
}

pub fn load_cache() -> Option<Vec<u8>> {
    fs::read(cache_path())
        .ok()
        .filter(|bytes| !bytes.is_empty())
}

pub fn save_cache(bytes: &[u8]) -> std::io::Result<()> {
    write_bytes(cache_path(), bytes)
}

pub fn load_terms_version() -> u32 {
    fs::read_to_string(terms_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

pub fn terms_need_accept() -> bool {
    load_terms_version() < CURRENT_TERMS_VERSION
}

pub fn accept_terms() -> std::io::Result<()> {
    write_bytes(terms_path(), CURRENT_TERMS_VERSION.to_string().as_bytes())
}

pub fn load_node_url() -> String {
    fs::read_to_string(node_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_NODE.to_string())
}

pub fn save_node_url(url: &str) -> std::io::Result<()> {
    write_bytes(node_path(), url.trim().as_bytes())
}

pub fn load_pending_send() -> Option<String> {
    fs::read_to_string(pending_send_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn save_pending_send(json: &str) -> std::io::Result<()> {
    write_bytes(pending_send_path(), json.as_bytes())
}

pub fn clear_pending_send() {
    let _ = fs::remove_file(pending_send_path());
}

pub fn mark_wallet_stored(restore_height: u64) -> std::io::Result<()> {
    write_bytes(restore_height_path(), restore_height.to_string().as_bytes())?;
    write_bytes(wallet_slot_path(), b"1")
}

pub fn load_restore_height() -> u64 {
    fs::read_to_string(restore_height_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

pub fn clear_wallet_slot() {
    let _ = fs::remove_file(wallet_slot_path());
    let _ = fs::remove_file(restore_height_path());
}

pub fn load_fiat_enabled() -> bool {
    fs::read_to_string(fiat_enabled_path())
        .ok()
        .map(|s| matches!(s.trim(), "1" | "true"))
        .unwrap_or(false)
}

pub fn save_fiat_enabled(enabled: bool) -> std::io::Result<()> {
    write_bytes(fiat_enabled_path(), if enabled { b"1" } else { b"0" })
}

pub fn load_fiat_currency() -> String {
    fs::read_to_string(fiat_currency_path())
        .ok()
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| crate::fiat::is_supported(s))
        .unwrap_or_else(|| "USD".to_string())
}

pub fn save_fiat_currency(code: &str) -> std::io::Result<()> {
    write_bytes(fiat_currency_path(), code.trim().as_bytes())
}

pub fn load_fiat_rate() -> Option<crate::fiat::Rate> {
    let raw = fs::read_to_string(fiat_rate_path()).ok()?;
    let mut lines = raw.lines();
    let currency = lines.next()?.trim().to_string();
    let fiat_per_xmr: f64 = lines.next()?.trim().parse().ok()?;
    let fetched_at_ms: u64 = lines.next()?.trim().parse().ok()?;
    let source = lines.next().unwrap_or("kraken").trim().to_string();
    if !crate::fiat::is_supported(&currency) || fiat_per_xmr <= 0.0 {
        return None;
    }
    Some(crate::fiat::Rate {
        currency,
        fiat_per_xmr,
        fetched_at_ms,
        source,
    })
}

pub fn save_fiat_rate(rate: &crate::fiat::Rate) -> std::io::Result<()> {
    let body = format!(
        "{}\n{}\n{}\n{}\n",
        rate.currency, rate.fiat_per_xmr, rate.fetched_at_ms, rate.source
    );
    write_bytes(fiat_rate_path(), body.as_bytes())
}

pub fn clear_fiat_rate() {
    let _ = fs::remove_file(fiat_rate_path());
}

pub fn load_device_auth_preference() -> Option<bool> {
    fs::read_to_string(device_auth_path())
        .ok()
        .and_then(|value| parse_bool_preference(&value))
}

pub fn save_device_auth(enabled: bool) -> std::io::Result<()> {
    write_bytes(device_auth_path(), if enabled { b"1" } else { b"0" })
}

fn parse_bool_preference(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

pub fn load_network_policy() -> crate::network::Policy {
    crate::network::Policy::from_raw(&fs::read_to_string(network_policy_path()).unwrap_or_default())
}

pub fn save_network_policy(policy: crate::network::Policy) -> std::io::Result<()> {
    write_bytes(network_policy_path(), policy.as_str().as_bytes())
}

pub fn load_i2p_rpc() -> String {
    fs::read_to_string(i2p_rpc_path())
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub fn save_i2p_rpc(value: &str) -> std::io::Result<()> {
    write_bytes(i2p_rpc_path(), value.trim().as_bytes())
}

pub fn load_i2p_proxy() -> String {
    fs::read_to_string(i2p_proxy_path())
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

pub fn save_i2p_proxy(value: &str) -> std::io::Result<()> {
    write_bytes(i2p_proxy_path(), value.trim().as_bytes())
}

pub fn load_sync_details_expanded() -> bool {
    fs::read_to_string(sync_details_expanded_path())
        .ok()
        .map(|s| matches!(s.trim(), "1" | "true"))
        .unwrap_or(false)
}

pub fn save_sync_details_expanded(expanded: bool) -> std::io::Result<()> {
    write_bytes(
        sync_details_expanded_path(),
        if expanded { b"1" } else { b"0" },
    )
}

pub fn load_fiat_opted_in_at() -> u64 {
    fs::read_to_string(fiat_opted_in_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

pub fn ensure_fiat_opted_in_at() -> u64 {
    let stored = load_fiat_opted_in_at();
    if stored > 0 {
        return stored;
    }
    if !load_fiat_enabled() {
        return 0;
    }
    let now = crate::fiat::now_ms();
    let _ = write_bytes(fiat_opted_in_path(), now.to_string().as_bytes());
    now
}

pub(crate) fn write_bytes(path: PathBuf, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::parse_bool_preference;

    #[test]
    fn device_auth_preference_is_tristate() {
        assert_eq!(parse_bool_preference("1"), Some(true));
        assert_eq!(parse_bool_preference("TRUE\n"), Some(true));
        assert_eq!(parse_bool_preference("0"), Some(false));
        assert_eq!(parse_bool_preference("false"), Some(false));
        assert_eq!(parse_bool_preference(""), None);
        assert_eq!(parse_bool_preference("invalid"), None);
    }
}
