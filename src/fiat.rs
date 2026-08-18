//! Opt-in XMR fiat estimates. Same sources as iOS/Android: Kraken + Frankfurter.

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
    rate.filter(|r| is_fresh(r.fetched_at_ms, now))
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

pub fn parse_kraken_last_trade(json: &str) -> Option<f64> {
    let result_idx = json.find("\"result\"")?;
    let c_idx = json[result_idx..].find("\"c\"")? + result_idx;
    let bracket = json[c_idx..].find('[')? + c_idx;
    let quote1 = json[bracket + 1..].find('"')? + bracket + 1;
    let quote2 = json[quote1 + 1..].find('"')? + quote1 + 1;
    json[quote1 + 1..quote2].parse().ok()
}

pub fn parse_frankfurter_rate(json: &str, symbol: &str) -> Option<f64> {
    let code = symbol.to_ascii_uppercase();
    if code == "USD" {
        return Some(1.0);
    }
    let rates_idx = json.find("\"rates\"")?;
    let key = format!("\"{code}\"");
    let key_idx = json[rates_idx..].find(&key)? + rates_idx;
    let colon = json[key_idx + key.len()..].find(':')? + key_idx + key.len();
    let rest = json[colon + 1..].trim_start();
    let end = rest
        .find(|ch: char| ch == ',' || ch == '}' || ch.is_whitespace())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

pub fn fetch_rate(currency: &str) -> Result<Rate, String> {
    let code = currency.trim().to_ascii_uppercase();
    if !is_supported(&code) {
        return Err(format!("unsupported currency {code}"));
    }
    let now = now_ms();
    if code == "EUR" {
        let last = fetch_kraken("XMREUR")?;
        return Ok(Rate {
            currency: code,
            fiat_per_xmr: last,
            fetched_at_ms: now,
            source: "kraken".into(),
        });
    }
    let usd = fetch_kraken("XMRUSD")?;
    if code == "USD" {
        return Ok(Rate {
            currency: code,
            fiat_per_xmr: usd,
            fetched_at_ms: now,
            source: "kraken".into(),
        });
    }
    let fx = fetch_frankfurter(&code)?;
    Ok(Rate {
        currency: code,
        fiat_per_xmr: usd * fx,
        fetched_at_ms: now,
        source: "kraken+frankfurter".into(),
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(8))
        .build()
}

fn fetch_kraken(pair: &str) -> Result<f64, String> {
    let url = format!("https://api.kraken.com/0/public/Ticker?pair={pair}");
    let json = http_get(&url)?;
    parse_kraken_last_trade(&json).ok_or_else(|| "Kraken response missing last trade".into())
}

fn fetch_frankfurter(symbol: &str) -> Result<f64, String> {
    let url = format!("https://api.frankfurter.dev/v1/latest?base=USD&symbols={symbol}");
    let json = http_get(&url)?;
    parse_frankfurter_rate(&json, symbol)
        .ok_or_else(|| format!("Frankfurter response missing {symbol}"))
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
    fn kraken_parse() {
        let json = r#"{"result":{"XXMRZUSD":{"c":["356.85","1"]}}}"#;
        assert_eq!(parse_kraken_last_trade(json), Some(356.85));
    }

    #[test]
    fn frankfurter_parse() {
        let json = r#"{"rates":{"GBP":0.78}}"#;
        assert_eq!(parse_frankfurter_rate(json, "GBP"), Some(0.78));
    }

    #[test]
    fn piconero_from_fiat_rounds_down() {
        let rate = Rate {
            currency: "USD".into(),
            fiat_per_xmr: 100.0,
            fetched_at_ms: 1,
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
    fn seen_snapshot_after_opt_in() {
        assert!(should_record_seen_snapshot(Some(2_000), 1_500_000));
        assert!(!should_record_seen_snapshot(Some(1), 1_500_000));
        assert!(!should_record_seen_snapshot(None, 1));
        assert!(!should_record_seen_snapshot(Some(2_000), 0));
    }
}
