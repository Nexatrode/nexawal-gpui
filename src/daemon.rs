//! Daemon helpers: fast restore height from `/get_info`, matching iOS/Android.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProbe {
    pub height: u64,
    pub target_height: u64,
    pub latency_ms: u128,
}

pub fn fetch_suggested_restore_height(node_url: &str, proxy: Option<&str>, require_i2p: bool) -> Result<u64, String> {
    let probe = probe(node_url, proxy, require_i2p)?;
    let tip = probe.target_height.max(probe.height);
    Ok(if tip > 10 { tip - 10 } else { 0 })
}

pub fn probe(node_url: &str, proxy: Option<&str>, require_i2p: bool) -> Result<NodeProbe, String> {
    let base = node_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("node URL is empty".into());
    }
    let parsed = reqwest::Url::parse(base).map_err(|_| "Invalid daemon URL")?;
    if require_i2p || parsed.host_str().is_some_and(|host| host.ends_with(".i2p")) {
        crate::network::validate_i2p_configuration(base, proxy)?;
    }
    let url = format!("{base}/get_info");
    let started = std::time::Instant::now();
    let mut builder = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(5))
        .redirects(0).try_proxy_from_env(false);
    if let Some(proxy) = proxy.filter(|s| !s.trim().is_empty()) {
        let proxy_url = crate::network::proxy_url(proxy);
        builder = builder.proxy(ureq::Proxy::new(&proxy_url).map_err(|_| "Invalid I2P HTTP proxy")?);
    }
    let json = builder
        .build()
        .get(&url)
        .call()
        .map_err(|err| err.to_string())?
        .into_string()
        .map_err(|err| err.to_string())?;
    let target = json_u64(&json, "target_height").unwrap_or(0);
    let height = json_u64(&json, "height").unwrap_or(0);
    if target == 0 && height == 0 {
        return Err("node did not report a height".into());
    }
    Ok(NodeProbe {
        height,
        target_height: target,
        latency_ms: started.elapsed().as_millis(),
    })
}

fn json_u64(json: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let idx = json.find(&needle)?;
    let colon = json[idx + needle.len()..].find(':')? + idx + needle.len();
    let rest = json[colon + 1..].trim_start();
    let end = rest
        .find(|ch: char| ch == ',' || ch == '}' || ch.is_whitespace())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_get_info_heights() {
        let json = r#"{"height":100,"target_height":120}"#;
        assert_eq!(json_u64(json, "target_height"), Some(120));
        assert_eq!(json_u64(json, "height"), Some(100));
    }

    #[test]
    fn probe_type_is_stable_for_ui_reporting() {
        let probe = NodeProbe {
            height: 100,
            target_height: 120,
            latency_ms: 7,
        };
        assert_eq!(probe.target_height.max(probe.height), 120);
        assert_eq!(probe.latency_ms, 7);
    }
}
