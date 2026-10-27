//! Clearnet / I2P / hybrid routing, matching iOS/Android NetworkRouting.

pub const DEFAULT_I2P_RPC_ADDRESS: &str = "7lfi73h743qm663hv4x5q6ccgc4jc5ncl65dfvpe665m6pawwdxa.b32.i2p";

pub fn validate_i2p_configuration(node: &str, proxy: Option<&str>) -> Result<(), String> {
    let node = reqwest::Url::parse(&normalize_url(node)).map_err(|_| "Invalid I2P RPC address")?;
    if node.scheme() != "http" || !node.host_str().is_some_and(|host| host.ends_with(".i2p"))
        || node.port() == Some(0) || node.query().is_some() || node.fragment().is_some() {
        return Err("Use an HTTP I2P RPC address ending in .i2p, not a clearnet URL or P2P :0 address".into());
    }
    let proxy = proxy.filter(|proxy| !proxy.trim().is_empty()).ok_or("An I2P HTTP proxy is required")?;
    let proxy = reqwest::Url::parse(&proxy_url(proxy)).map_err(|_| "Invalid I2P HTTP proxy")?;
    if proxy.scheme() != "http" || proxy.host_str().is_none() || proxy.port_or_known_default().is_none()
        || proxy.port() == Some(0) || !proxy.username().is_empty() || proxy.password().is_some()
        || proxy.path() != "/" || proxy.query().is_some() || proxy.fragment().is_some() {
        return Err("Use an HTTP proxy such as 127.0.0.1:4444 (not SOCKS or HTTPS)".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Clearnet,
    I2p,
    Hybrid,
}

impl Policy {
    pub fn from_raw(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "i2p" => Self::I2p,
            "hybrid" => Self::Hybrid,
            _ => Self::Clearnet,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clearnet => "clearnet",
            Self::I2p => "i2p",
            Self::Hybrid => "hybrid",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Clearnet => Self::I2p,
            Self::I2p => Self::Hybrid,
            Self::Hybrid => Self::Clearnet,
        }
    }

    pub fn label(self) -> gpui::SharedString {
        crate::l10n::t(match self {
            Self::Clearnet => "Clearnet only",
            Self::I2p => "I2P only",
            Self::Hybrid => "Scan clearnet, broadcast I2P",
        })
    }
}

pub fn explicit_node_url(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(trimmed)
    } else {
        None
    }
}

pub fn normalize_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(explicit) = explicit_node_url(trimmed) {
        return explicit.to_string();
    }
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("http://{trimmed}")
    }
}

pub fn scan_node_url(policy: Policy, clearnet: &str, i2p_rpc: &str) -> String {
    match policy {
        Policy::Clearnet | Policy::Hybrid => clearnet.trim().to_string(),
        Policy::I2p => normalize_url(i2p_rpc),
    }
}

pub fn broadcast_node_url(policy: Policy, clearnet: &str, i2p_rpc: &str) -> String {
    match policy {
        Policy::Clearnet => clearnet.trim().to_string(),
        Policy::I2p | Policy::Hybrid => normalize_url(i2p_rpc),
    }
}

pub fn should_use_i2p_http_proxy(
    policy: Policy,
    proxy_configured: bool,
    for_broadcast: bool,
) -> bool {
    if !proxy_configured {
        return false;
    }
    match policy {
        Policy::Clearnet => false,
        Policy::I2p => true,
        Policy::Hybrid => for_broadcast,
    }
}

pub fn proxy_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.to_ascii_lowercase().starts_with("http://")
        || trimmed.to_ascii_lowercase().starts_with("https://")
    {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEARNET: &str = "https://rpc.nexawal.com";
    const I2P: &str = "cvxtgqjorfif6i5x5fenys6fj7hzddbgavpyutps6gphywnlklqa.b32.i2p:18081";

    #[test]
    fn i2p_configuration_rejects_missing_or_invalid_proxy_and_clearnet_endpoint() {
        for proxy in [None, Some(""), Some("socks5://127.0.0.1:4447"), Some("https://127.0.0.1:4444"), Some("127.0.0.1:0")] {
            assert!(validate_i2p_configuration(DEFAULT_I2P_RPC_ADDRESS, proxy).is_err());
        }
        for node in ["", CLEARNET, "http://127.0.0.1:18092", "http://test.i2p.evil.example", "test.i2p:0"] {
            assert!(validate_i2p_configuration(node, Some("127.0.0.1:4444")).is_err());
        }
        assert!(validate_i2p_configuration(DEFAULT_I2P_RPC_ADDRESS, Some("127.0.0.1:4444")).is_ok());
    }

    #[test]
    fn scan_uses_clearnet_except_i2p_only() {
        assert_eq!(scan_node_url(Policy::Clearnet, CLEARNET, I2P), CLEARNET);
        assert_eq!(scan_node_url(Policy::Hybrid, CLEARNET, I2P), CLEARNET);
        assert_eq!(
            scan_node_url(Policy::I2p, CLEARNET, I2P),
            normalize_url(I2P)
        );
    }

    #[test]
    fn broadcast_uses_i2p_for_i2p_and_hybrid() {
        let expected = normalize_url(I2P);
        assert_eq!(broadcast_node_url(Policy::I2p, CLEARNET, I2P), expected);
        assert_eq!(broadcast_node_url(Policy::Hybrid, CLEARNET, I2P), expected);
        assert_eq!(
            broadcast_node_url(Policy::Clearnet, CLEARNET, I2P),
            CLEARNET
        );
    }

    #[test]
    fn proxy_policy() {
        assert!(!should_use_i2p_http_proxy(Policy::I2p, false, true));
        assert!(!should_use_i2p_http_proxy(Policy::Clearnet, true, true));
        assert!(should_use_i2p_http_proxy(Policy::I2p, true, false));
        assert!(should_use_i2p_http_proxy(Policy::Hybrid, true, true));
        assert!(!should_use_i2p_http_proxy(Policy::Hybrid, true, false));
    }
}
