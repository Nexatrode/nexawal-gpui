//! `monero:` payment URI parse. Spend/view key params are ignored.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentUri {
    pub address: String,
    pub amount_xmr: Option<String>,
}

pub fn parse(raw: &str) -> Option<PaymentUri> {
    let trimmed = raw.trim();
    let rest = trimmed
        .strip_prefix("monero:")
        .or_else(|| trimmed.strip_prefix("MONERO:"))?;
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let (address_raw, query) = match rest.split_once('?') {
        Some((addr, q)) => (addr, Some(q)),
        None => (rest, None),
    };
    let address = address_raw.trim_matches('/').trim().to_string();
    if address.is_empty() {
        return None;
    }
    let mut amount_xmr = None;
    if let Some(query) = query {
        for pair in query.split('&') {
            let mut it = pair.splitn(2, '=');
            let name = it.next().unwrap_or("").to_ascii_lowercase();
            let value = it.next().unwrap_or("");
            if matches!(
                name.as_str(),
                "spend_key" | "view_key" | "spendkey" | "viewkey"
            ) {
                continue;
            }
            if matches!(name.as_str(), "amount" | "tx_amount")
                && !value.is_empty()
                && amount_xmr.is_none()
            {
                amount_xmr = Some(percent_decode(value));
            }
        }
    }
    Some(PaymentUri { address, amount_xmr })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn looks_like_address(value: &str) -> bool {
    let s = value.trim();
    let len = s.len();
    (len == 95 || len == 106) && (s.starts_with('4') || s.starts_with('8'))
}

pub fn build(address: &str, amount_xmr: Option<&str>, description: Option<&str>) -> String {
    let addr = address.trim();
    let mut params = Vec::new();
    if let Some(amt) = amount_xmr.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(format!("tx_amount={}", percent_encode(amt)));
    }
    if let Some(desc) = description.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(format!("tx_description={}", percent_encode(desc)));
    }
    if params.is_empty() {
        format!("monero:{addr}")
    } else {
        format!("monero:{addr}?{}", params.join("&"))
    }
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uri_with_amount() {
        let uri = parse("monero:4abc?amount=1.25").unwrap();
        assert_eq!(uri.address, "4abc");
        assert_eq!(uri.amount_xmr.as_deref(), Some("1.25"));
    }

    #[test]
    fn ignores_view_key() {
        let uri = parse("monero:8xyz?view_key=secret&amount=2").unwrap();
        assert_eq!(uri.address, "8xyz");
        assert_eq!(uri.amount_xmr.as_deref(), Some("2"));
    }

    #[test]
    fn build_uri_with_amount_and_description() {
        let uri = build("4abc", Some("1.25"), Some("thanks"));
        assert_eq!(uri, "monero:4abc?tx_amount=1.25&tx_description=thanks");
    }
}
