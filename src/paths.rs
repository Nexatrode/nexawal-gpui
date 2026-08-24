use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

pub fn theme_path() -> PathBuf {
    data_dir().join("theme")
}

pub fn window_placement_path() -> PathBuf {
    data_dir().join("window_placement")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPlacement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl WindowPlacement {
    fn parse(raw: &str) -> Option<Self> {
        let mut lines = raw.lines();
        if lines.next()?.trim() != "1" {
            return None;
        }
        let maximized = match lines.next()?.trim() {
            "windowed" => false,
            "maximized" => true,
            _ => return None,
        };
        let placement = Self {
            x: lines.next()?.trim().parse().ok()?,
            y: lines.next()?.trim().parse().ok()?,
            width: lines.next()?.trim().parse().ok()?,
            height: lines.next()?.trim().parse().ok()?,
            maximized,
        };
        placement.is_sane().then_some(placement)
    }

    fn is_sane(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .into_iter()
            .all(f32::is_finite)
            && (520.0..=10_000.0).contains(&self.width)
            && (520.0..=10_000.0).contains(&self.height)
            && (-100_000.0..=100_000.0).contains(&self.x)
            && (-100_000.0..=100_000.0).contains(&self.y)
    }

    fn encode(self) -> String {
        format!(
            "1\n{}\n{}\n{}\n{}\n{}\n",
            if self.maximized {
                "maximized"
            } else {
                "windowed"
            },
            self.x,
            self.y,
            self.width,
            self.height
        )
    }
}

pub fn load_window_placement() -> Option<WindowPlacement> {
    WindowPlacement::parse(&fs::read_to_string(window_placement_path()).ok()?)
}

pub fn save_window_placement(placement: WindowPlacement) -> std::io::Result<()> {
    if !placement.is_sane() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid window placement",
        ));
    }
    write_bytes(window_placement_path(), placement.encode().as_bytes())
}

pub fn load_theme() -> String {
    fs::read_to_string(theme_path())
        .ok()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "classic".to_string())
}

pub fn save_theme(theme: &str) -> std::io::Result<()> {
    write_bytes(theme_path(), theme.trim().as_bytes())
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

pub fn sync_audit_path(run_id: u64) -> PathBuf {
    data_dir().join(format!("sync_audit_{run_id}.json"))
}

pub fn sync_audit_rpc_path(run_id: u64) -> PathBuf {
    data_dir().join(format!("sync_audit_rpc_{run_id}.jsonl"))
}

pub fn sync_torture_audit_path(run_id: u64) -> PathBuf {
    data_dir().join(format!("sync_torture_audit_{run_id}.json"))
}

pub fn sync_torture_audit_rpc_path(run_id: u64) -> PathBuf {
    data_dir().join(format!("sync_torture_audit_rpc_{run_id}.jsonl"))
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

pub fn load_cache() -> std::io::Result<Option<Vec<u8>>> {
    match fs::read(cache_path()) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn save_cache(bytes: &[u8]) -> std::io::Result<()> {
    write_bytes(cache_path(), bytes)
}

/// Moves a rejected cache out of the active slot while retaining it for diagnosis.
/// A hard link provides collision-safe, no-overwrite naming on every supported desktop
/// platform. If the process stops between linking and unlinking, the next launch merely
/// sees the same bytes in both places and can quarantine the active slot again.
pub fn quarantine_rejected_cache() -> std::io::Result<Option<PathBuf>> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    quarantine_rejected_file_at(&cache_path(), timestamp)
}

fn quarantine_rejected_file_at(
    target: &Path,
    timestamp_milliseconds: u128,
) -> std::io::Result<Option<PathBuf>> {
    match target.try_exists() {
        Ok(false) => return Ok(None),
        Ok(true) => {}
        Err(error) => return Err(error),
    }

    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = target.file_name().unwrap_or_default().to_string_lossy();
    let mut attempt = 0_u64;
    loop {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{attempt}")
        };
        let candidate = parent.join(format!(
            "{file_name}.rejected-{timestamp_milliseconds}{suffix}"
        ));
        match fs::hard_link(target, &candidate) {
            Ok(()) => {
                if let Err(error) = fs::remove_file(target) {
                    let _ = fs::remove_file(&candidate);
                    return Err(error);
                }
                #[cfg(unix)]
                fs::File::open(parent)?.sync_all()?;
                return Ok(Some(candidate));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt = attempt.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
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

pub fn save_restore_height(height: u64) -> std::io::Result<()> {
    write_bytes(restore_height_path(), height.to_string().as_bytes())
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
    use std::io::Write;

    write_bytes_with(path, |file| file.write_all(bytes))
}

fn write_bytes_with(
    path: PathBuf,
    writer: impl FnOnce(&mut fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    // Write beside the destination so persist() is an atomic rename on the same
    // filesystem. A crash can leave the previous complete file or the new complete
    // file, never a truncated wallet cache/preferences file.
    let mut temporary = tempfile::Builder::new()
        .prefix(".nexawal-write-")
        .tempfile_in(parent)?;
    writer(temporary.as_file_mut())?;
    temporary.as_file_mut().sync_all()?;
    temporary.persist(&path).map_err(|error| error.error)?;

    // Also make the directory entry durable where the platform supports fsync on
    // directories. Windows does not expose directories as ordinary files.
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Error, ErrorKind, Write};

    use super::{
        WindowPlacement, parse_bool_preference, quarantine_rejected_file_at, write_bytes,
        write_bytes_with,
    };

    #[test]
    fn device_auth_preference_is_tristate() {
        assert_eq!(parse_bool_preference("1"), Some(true));
        assert_eq!(parse_bool_preference("TRUE\n"), Some(true));
        assert_eq!(parse_bool_preference("0"), Some(false));
        assert_eq!(parse_bool_preference("false"), Some(false));
        assert_eq!(parse_bool_preference(""), None);
        assert_eq!(parse_bool_preference("invalid"), None);
    }

    #[test]
    fn atomic_write_replaces_the_complete_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wallet.cache");
        write_bytes(path.clone(), b"first complete cache").unwrap();
        write_bytes(path.clone(), b"second complete cache").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"second complete cache");
    }

    #[test]
    fn interrupted_atomic_write_leaves_previous_cache_complete() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wallet.cache");
        write_bytes(path.clone(), b"previous complete cache").unwrap();

        let error = write_bytes_with(path.clone(), |file| {
            file.write_all(b"partial replacement")?;
            Err(Error::new(ErrorKind::Other, "simulated interruption"))
        })
        .unwrap_err();

        assert_eq!(error.kind(), ErrorKind::Other);
        assert_eq!(fs::read(path).unwrap(), b"previous complete cache");
    }

    #[test]
    fn rejected_caches_leave_the_active_slot_and_never_overwrite_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("wallet.cache");
        fs::write(&path, b"rejected one").unwrap();
        let first = quarantine_rejected_file_at(&path, 1234).unwrap().unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read(&first).unwrap(), b"rejected one");

        fs::write(&path, b"rejected two").unwrap();
        let second = quarantine_rejected_file_at(&path, 1234).unwrap().unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&second).unwrap(), b"rejected two");
        assert_eq!(quarantine_rejected_file_at(&path, 1234).unwrap(), None);
    }

    #[test]
    fn window_placement_round_trips_and_rejects_bad_sizes() {
        let placement = WindowPlacement {
            x: 120.5,
            y: -20.0,
            width: 1100.0,
            height: 760.0,
            maximized: true,
        };
        assert_eq!(WindowPlacement::parse(&placement.encode()), Some(placement));
        assert!(WindowPlacement::parse("1\nwindowed\n0\n0\n100\n100\n").is_none());
        assert!(WindowPlacement::parse("2\nwindowed\n0\n0\n1100\n760\n").is_none());
    }
}
