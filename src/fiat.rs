//! Opt-in XMR fiat estimates fetched from the NexaWal rates API over clearnet HTTPS.

use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_AGE_MS: u64 = 30 * 60 * 1000;
pub const REFRESH_INTERVAL_MS: u64 = 15 * 60 * 1000;

pub const SUPPORTED: [&str; 30] = [
    "USD", "EUR", "GBP", "JPY", "CNY", "AUD", "CAD", "CHF", "HKD", "SGD", "NZD", "SEK", "NOK",
    "DKK", "PLN", "CZK", "HUF", "RON", "TRY", "BRL", "MXN", "INR", "KRW", "IDR", "THB", "PHP",
    "MYR", "ZAR", "ILS", "ISK",
];

#[derive(Clone, Debug)]
pub struct Rate {
    pub currency: String,
    pub fiat_per_xmr: f64,
    pub fetched_at_ms: u64,
    pub expires_at_ms: u64,
    pub source: String,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn is_supported(code: &str) -> bool {
    SUPPORTED.iter().any(|c| *c == code)
}

pub fn next_currency(current: &str) -> &'static str {
    let idx = SUPPORTED.iter().position(|c| *c == current).unwrap_or(0);
    SUPPORTED[(idx + 1) % SUPPORTED.len()]
}

pub fn prev_currency(current: &str) -> &'static str {
    let idx = SUPPORTED.iter().position(|c| *c == current).unwrap_or(0);
    SUPPORTED[(idx + SUPPORTED.len() - 1) % SUPPORTED.len()]
}

pub fn is_fresh(fetched_at_ms: u64, now: u64) -> bool {
    now >= fetched_at_ms && now.saturating_sub(fetched_at_ms) < MAX_AGE_MS
}

pub fn live<'a>(rate: Option<&'a Rate>, now: u64) -> Option<&'a Rate> {
    rate.filter(|r| is_fresh(r.fetched_at_ms, now) && now < r.expires_at_ms)
}

pub fn decimal_places(currency: &str) -> usize {
    if matches!(currency, "JPY" | "KRW" | "HUF" | "ISK") {
        0
    } else {
        2
    }
}

/// Convert a typed fiat amount to piconero. Rounds **down** so send never exceeds the typed value.
pub fn piconero_from_fiat(fiat_text: &str, rate: &Rate) -> Option<u64> {
    if rate.fiat_per_xmr <= 0.0 {
        return None;
    }
    let fiat = parse_decimal(fiat_text)?;
    if fiat < 0.0 {
        return None;
    }
    if fiat == 0.0 {
        return Some(0);
    }
    let pico = (fiat / rate.fiat_per_xmr * 1_000_000_000_000.0).floor();
    if pico < 0.0 || pico > u64::MAX as f64 {
        return None;
    }
    Some(pico as u64)
}

pub fn format_fiat_for_input(piconero: u64, rate: &Rate) -> String {
    let places = decimal_places(&rate.currency);
    let amount = (piconero as f64 / 1_000_000_000_000.0) * rate.fiat_per_xmr;
    format_plain(amount, places)
}

pub fn format_xmr_approx(piconero: u64) -> String {
    format!("≈ {} XMR", crate::amount::format_for_input(piconero))
}

pub fn recorded_approx(piconero: u64, fiat_per_xmr: f64, currency: &str) -> String {
    format_approx_amount(piconero, fiat_per_xmr, currency)
}

pub fn should_record_seen_snapshot(tx_timestamp_seconds: Option<u64>, opted_in_at_ms: u64) -> bool {
    if opted_in_at_ms == 0 {
        return false;
    }
    let Some(ts) = tx_timestamp_seconds.filter(|t| *t > 0) else {
        return false;
    };
    ts.saturating_mul(1000) >= opted_in_at_ms
}

pub fn format_approx(piconero: u64, rate: &Rate) -> String {
    format_approx_amount(piconero, rate.fiat_per_xmr, &rate.currency)
}

fn format_approx_amount(piconero: u64, fiat_per_xmr: f64, currency: &str) -> String {
    let amount = (piconero as f64 / 1_000_000_000_000.0) * fiat_per_xmr;
    let places = decimal_places(currency);
    let number = format_plain(amount.abs(), places);
    let grouped = group_thousands(&number, places);
    let signed = if amount < 0.0 {
        format!("-{grouped}")
    } else {
        grouped
    };
    match symbol(currency) {
        Some(sym) => format!("≈ {sym}{signed}"),
        None => format!("≈ {signed} {currency}"),
    }
}

fn parse_decimal(raw: &str) -> Option<f64> {
    let trimmed = raw.trim().replace(',', ".");
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse().ok()
}

fn format_plain(amount: f64, places: usize) -> String {
    if places == 0 {
        format!("{:.0}", amount)
    } else {
        format!("{amount:.places$}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

pub fn live_approx(piconero: u64, rate: Option<&Rate>) -> Option<String> {
    live(rate, now_ms()).map(|r| format_approx(piconero, r))
}

fn symbol(currency: &str) -> Option<&'static str> {
    Some(match currency {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" | "CNY" => "¥",
        "KRW" => "₩",
        "INR" => "₹",
        "AUD" => "A$",
        "CAD" => "C$",
        "HKD" => "HK$",
        "SGD" => "S$",
        "NZD" => "NZ$",
        "BRL" => "R$",
        "MXN" => "MX$",
        _ => return None,
    })
}

fn group_thousands(number: &str, places: usize) -> String {
    let (whole, frac) = match number.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (number, None),
    };
    let mut out = String::new();
    for (i, ch) in whole.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let grouped: String = out.chars().rev().collect();
    if places == 0 {
        grouped
    } else {
        format!("{grouped}.{}", frac.unwrap_or("00"))
    }
}

pub fn fetch_rate(currency: &str) -> Result<Rate, String> {
    let code = currency.trim().to_ascii_uppercase();
    if !is_supported(&code) {
        return Err(format!("unsupported currency {code}"));
    }
    let json = http_get(&format!("https://rates.nexatrode.com/v1/rates/XMR/{code}"))?;
    parse_nexatrode_rate(&json, &code, now_ms())
}

pub fn parse_nexatrode_rate(json: &str, currency: &str, now: u64) -> Result<Rate, String> {
    let code = currency.trim().to_ascii_uppercase();
    if !is_supported(&code) {
        return Err(format!("unsupported currency {code}"));
    }
    let value: serde_json::Value = serde_json::from_str(json).map_err(|err| err.to_string())?;
    let base = value.get("base").and_then(serde_json::Value::as_str);
    let quote = value.get("quote").and_then(serde_json::Value::as_str);
    let price = value
        .get("fiat_per_xmr")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse::<f64>().ok());
    let checked_at = value
        .get("checked_at_ms")
        .and_then(serde_json::Value::as_u64);
    let expires_at = value
        .get("expires_at_ms")
        .and_then(serde_json::Value::as_u64);

    let (Some(price), Some(checked_at), Some(expires_at)) = (price, checked_at, expires_at) else {
        return Err("rates response is missing required fields".into());
    };
    if base != Some("XMR") || quote != Some(code.as_str()) {
        return Err("rates response currency does not match request".into());
    }
    if !price.is_finite() || price <= 0.0 {
        return Err("rates response contains an invalid price".into());
    }
    if checked_at == 0
        || checked_at > now
        || expires_at <= now
        || expires_at.saturating_sub(checked_at) > MAX_AGE_MS
        || !is_fresh(checked_at, now)
    {
        return Err("rates response is expired or outside the accepted age".into());
    }

    Ok(Rate {
        currency: code,
        fiat_per_xmr: price,
        fetched_at_ms: checked_at,
        expires_at_ms: expires_at,
        source: "nexatrode".into(),
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(8))
        .build()
}

fn http_get(url: &str) -> Result<String, String> {
    agent()
        .get(url)
        .call()
        .map_err(|err| err.to_string())?
        .into_string()
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nexatrode_response_is_validated_and_uses_server_timestamp() {
        let now = 1_700_000_000_000;
        let json = format!(
            r#"{{"base":"XMR","quote":"USD","fiat_per_xmr":"547.38","checked_at_ms":{},"expires_at_ms":{}}}"#,
            now - 10_000,
            now + 1_790_000
        );
        let rate = parse_nexatrode_rate(&json, "USD", now).unwrap();
        assert_eq!(rate.currency, "USD");
        assert_eq!(rate.fiat_per_xmr, 547.38);
        assert_eq!(rate.fetched_at_ms, now - 10_000);
        assert_eq!(rate.expires_at_ms, now + 1_790_000);
        assert_eq!(rate.source, "nexatrode");
    }

    #[test]
    fn nexatrode_response_rejects_mismatch_expired_future_and_invalid_values() {
        let now = 1_700_000_000_000;
        let response = |quote: &str, price: &str, checked: u64, expires: u64| {
            format!(
                r#"{{"base":"XMR","quote":"{quote}","fiat_per_xmr":"{price}","checked_at_ms":{checked},"expires_at_ms":{expires}}}"#
            )
        };
        assert!(parse_nexatrode_rate(&response("EUR", "2", now - 1, now + 1), "USD", now).is_err());
        assert!(
            parse_nexatrode_rate(
                &response("USD", "2", now - MAX_AGE_MS - 1, now + 1),
                "USD",
                now
            )
            .is_err()
        );
        assert!(parse_nexatrode_rate(&response("USD", "2", now + 1, now + 2), "USD", now).is_err());
        assert!(
            parse_nexatrode_rate(&response("USD", "NaN", now - 1, now + 1), "USD", now).is_err()
        );
    }

    #[test]
    fn piconero_from_fiat_rounds_down() {
        let rate = Rate {
            currency: "USD".into(),
            fiat_per_xmr: 100.0,
            fetched_at_ms: 1,
            expires_at_ms: 1 + MAX_AGE_MS,
            source: "test".into(),
        };
        assert_eq!(piconero_from_fiat("50", &rate), Some(500_000_000_000));
        assert_eq!(
            piconero_from_fiat("50.000000000001", &rate),
            Some(500_000_000_000)
        );
        assert_eq!(piconero_from_fiat("0", &rate), Some(0));
        assert_eq!(piconero_from_fiat("", &rate), None);
        assert_eq!(piconero_from_fiat("abc", &rate), None);
    }

    #[test]
    fn live_quote_expires_at_server_deadline_not_local_maximum() {
        let rate = Rate { currency: "USD".into(), fiat_per_xmr: 100.0,
            fetched_at_ms: 1_000_000, expires_at_ms: 1_001_000, source: "nexatrode".into() };
        assert!(live(Some(&rate), 1_000_999).is_some());
        assert!(live(Some(&rate), 1_001_000).is_none());
        assert!(live(Some(&rate), 999_999).is_none());
        let extended = Rate { expires_at_ms: u64::MAX, ..rate };
        assert!(live(Some(&extended), 1_000_000 + MAX_AGE_MS).is_none());
    }

    #[test]
    fn seen_snapshot_after_opt_in() {
        assert!(should_record_seen_snapshot(Some(2_000), 1_500_000));
        assert!(!should_record_seen_snapshot(Some(1), 1_500_000));
        assert!(!should_record_seen_snapshot(None, 1));
        assert!(!should_record_seen_snapshot(Some(2_000), 0));
    }
}
