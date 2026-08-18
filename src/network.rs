//! Clearnet / I2P / hybrid routing, matching iOS/Android NetworkRouting.

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

    pub fn label(self) -> &'static str {
        match self {
            Self::Clearnet => "Clearnet only",
            Self::I2p => "I2P only",
            Self::Hybrid => "Scan clearnet, broadcast I2P",
        }
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

pub fn apply_http_proxy(proxy: Option<&str>) {
    unsafe {
        if let Some(raw) = proxy.filter(|s| !s.trim().is_empty()) {
            let url = proxy_url(raw);
            std::env::set_var("HTTP_PROXY", &url);
            std::env::set_var("http_proxy", &url);
            std::env::set_var("ALL_PROXY", &url);
            std::env::set_var("all_proxy", &url);
            std::env::remove_var("NO_PROXY");
            std::env::remove_var("no_proxy");
        } else {
            std::env::remove_var("HTTP_PROXY");
            std::env::remove_var("http_proxy");
            std::env::remove_var("ALL_PROXY");
            std::env::remove_var("all_proxy");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEARNET: &str = "https://rpc.nexatrode.com";
    const I2P: &str = "cvxtgqjorfif6i5x5fenys6fj7hzddbgavpyutps6gphywnlklqa.b32.i2p:18081";

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
