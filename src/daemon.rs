//! Daemon helpers: fast restore height from `/get_info`, matching iOS/Android.

pub fn fetch_suggested_restore_height(node_url: &str, proxy: Option<&str>) -> Result<u64, String> {
    let base = node_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("node URL is empty".into());
    }
    let url = format!("{base}/get_info");
    let mut builder = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(5));
    if let Some(proxy) = proxy.filter(|s| !s.trim().is_empty()) {
        let proxy_url = crate::network::proxy_url(proxy);
        if let Ok(px) = ureq::Proxy::new(&proxy_url) {
            builder = builder.proxy(px);
        }
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
    let tip = if target == 0 { height } else { target };
    if tip == 0 {
        return Err("node did not report a height".into());
    }
    Ok(if tip > 10 { tip - 10 } else { 0 })
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
}
