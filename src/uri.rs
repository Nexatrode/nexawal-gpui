//! `monero:` payment URI parse. Spend/view key params are ignored.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentUri {
    pub address: String,
    pub amount_xmr: Option<String>,
    pub description: Option<String>,
    pub recipient_name: Option<String>,
}

pub fn parse(raw: &str) -> Option<PaymentUri> {
    let trimmed = raw.trim();
    let (scheme, rest) = trimmed.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("monero") {
        return None;
    }
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
    let mut description = None;
    let mut recipient_name = None;
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
            if matches!(name.as_str(), "amount" | "tx_amount") {
                if value.is_empty() {
                    continue;
                }
                let decoded = percent_decode(value, false);
                if !is_valid_amount(&decoded) {
                    return None;
                }
                match &amount_xmr {
                    Some(existing) if existing == &decoded => {}
                    Some(_) => return None, // conflicting amounts
                    None => amount_xmr = Some(decoded),
                }
            } else if matches!(name.as_str(), "tx_description" | "message")
                && !value.is_empty()
                && description.is_none()
            {
                description = Some(percent_decode(value, true));
            } else if name == "recipient_name" && !value.is_empty() && recipient_name.is_none() {
                recipient_name = Some(percent_decode(value, true));
            }
        }
    }
    Some(PaymentUri {
        address,
        amount_xmr,
        description,
        recipient_name,
    })
}

/// Accept plain decimal XMR amounts used in payment URIs.
/// Rejects scientific notation, signs, empty values, and non-numeric junk.
fn is_valid_amount(value: &str) -> bool {
    let s = value.trim();
    if s.is_empty() || s.starts_with('+') || s.starts_with('-') {
        return false;
    }
    let mut seen_dot = false;
    let mut digits = 0usize;
    for (i, ch) in s.chars().enumerate() {
        match ch {
            '0'..='9' => digits += 1,
            '.' if !seen_dot => {
                if i == 0 {
                    return false;
                }
                seen_dot = true;
            }
            _ => return false,
        }
    }
    digits > 0
}

fn percent_decode(value: &str, plus_as_space: bool) -> String {
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
        out.push(if plus_as_space && bytes[i] == b'+' {
            b' '
        } else {
            bytes[i]
        });
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

    const PRIMARY: &str = "4B33mFPMq6mKi7Eiyd5XuyKRVMGVZz1Rqb9ZTyGApXW5d1aT7UBDZ89ewmnWFkzJ5wPd2SFbn313vCT8a4E2Qf4KQH4pNey";

    #[test]
    fn address_extracted() {
        let parsed = parse(&format!("monero:{PRIMARY}")).unwrap();
        assert_eq!(parsed.address, PRIMARY);
        assert_eq!(parsed.amount_xmr, None);
    }

    #[test]
    fn amount_extracted() {
        let parsed = parse(&format!("monero:{PRIMARY}?tx_amount=1.5")).unwrap();
        assert_eq!(parsed.address, PRIMARY);
        assert_eq!(parsed.amount_xmr.as_deref(), Some("1.5"));
    }

    #[test]
    fn identical_amount_duplicates_are_accepted() {
        let parsed =
            parse(&format!("monero:{PRIMARY}?amount=1.5&tx_amount=1.5")).unwrap();
        assert_eq!(parsed.amount_xmr.as_deref(), Some("1.5"));
    }

    #[test]
    fn conflicting_amounts_are_rejected() {
        assert!(parse(&format!("monero:{PRIMARY}?amount=1.5&tx_amount=2.0")).is_none());
    }

    #[test]
    fn invalid_amounts_are_rejected() {
        assert!(parse(&format!("monero:{PRIMARY}?tx_amount=+1.5")).is_none());
        assert!(parse(&format!("monero:{PRIMARY}?tx_amount=abc")).is_none());
        assert!(parse(&format!("monero:{PRIMARY}?tx_amount=1e3")).is_none());
    }

    #[test]
    fn mixed_case_scheme_and_metadata_are_supported() {
        let parsed = parse(&format!(
            "MonErO://{PRIMARY}?TX_AMOUNT=1.5&recipient_name=Coffee+Shop&message=two%20drinks%20%26%20tip"
        ))
        .unwrap();
        assert_eq!(parsed.address, PRIMARY);
        assert_eq!(parsed.amount_xmr.as_deref(), Some("1.5"));
        assert_eq!(parsed.recipient_name.as_deref(), Some("Coffee Shop"));
        assert_eq!(parsed.description.as_deref(), Some("two drinks & tip"));
    }

    #[test]
    fn percent_encoded_plus_amount_is_rejected() {
        assert!(parse(&format!("monero:{PRIMARY}?tx_amount=%2B1.5")).is_none());
    }

    #[test]
    fn spend_and_view_keys_ignored_as_send_targets() {
        let uri = format!(
            "monero:{PRIMARY}?spend_key=deadbeefdeadbeef&view_key=cafebabecafebabe&tx_amount=1.0"
        );
        let parsed = parse(&uri).unwrap();
        assert_eq!(parsed.address, PRIMARY);
        assert_eq!(parsed.amount_xmr.as_deref(), Some("1.0"));
        assert_ne!(parsed.address, "deadbeefdeadbeef");
        assert_ne!(parsed.address, "cafebabecafebabe");
    }

    #[test]
    fn build_round_trips_amount_and_description() {
        let built = build(PRIMARY, Some("0.25"), Some("coffee & tip"));
        let parsed = parse(&built).unwrap();
        assert_eq!(parsed.address, PRIMARY);
        assert_eq!(parsed.amount_xmr.as_deref(), Some("0.25"));
        assert_eq!(parsed.description.as_deref(), Some("coffee & tip"));
    }

    #[test]
    fn looks_like_address_shape_gate() {
        assert!(looks_like_address(PRIMARY));
        assert!(!looks_like_address("not-an-address"));
    }
}
