//! XMR amount parse/format, matching NexaWalLogic.XmrAmount.

pub const PICONERO_PER_XMR: u64 = 1_000_000_000_000;

pub fn parse_piconero(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let norm = trimmed.replace(',', ".");
    let mut parts = norm.splitn(2, '.');
    let whole_raw = parts.next().unwrap_or("");
    let frac_raw = parts.next().unwrap_or("");
    let whole_str = if whole_raw.is_empty() { "0" } else { whole_raw };
    if !whole_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !frac_raw.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if frac_raw.len() > 12 {
        return None;
    }
    let whole: u64 = whole_str.parse().ok()?;
    let frac_padded = format!("{frac_raw:0<12}");
    let frac: u64 = if frac_padded.is_empty() {
        0
    } else {
        frac_padded.parse().ok()?
    };
    let scaled = whole.checked_mul(PICONERO_PER_XMR)?;
    scaled.checked_add(frac)
}

pub fn format_for_input(piconero: u64) -> String {
    let whole = piconero / PICONERO_PER_XMR;
    let frac = piconero % PICONERO_PER_XMR;
    if frac == 0 {
        return whole.to_string();
    }
    let mut frac_str = format!("{frac:012}");
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{whole}.{frac_str}")
}

pub fn format_xmr(piconero: u64) -> String {
    let whole = piconero / PICONERO_PER_XMR;
    let frac = (piconero % PICONERO_PER_XMR) / 1_000_000;
    format!("{whole}.{frac:06} XMR")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_xmr() {
        assert_eq!(parse_piconero("1.0"), Some(1_000_000_000_000));
        assert_eq!(parse_piconero("1"), Some(1_000_000_000_000));
    }

    #[test]
    fn one_piconero() {
        assert_eq!(parse_piconero("0.000000000001"), Some(1));
    }

    #[test]
    fn format_for_input_matches_logic() {
        assert_eq!(format_for_input(1_000_000_000_000), "1");
        assert_eq!(format_for_input(500_000_000_000), "0.5");
        assert_eq!(format_for_input(1), "0.000000000001");
    }

    #[test]
    fn overflow_rejected() {
        assert_eq!(parse_piconero("18446745"), None);
        assert_eq!(parse_piconero("18446744073710.0"), None);
        assert_eq!(parse_piconero("999999999999999"), None);
    }

    #[test]
    fn invalid_rejected() {
        assert_eq!(parse_piconero(""), None);
        assert_eq!(parse_piconero("abc"), None);
        assert_eq!(parse_piconero("1.2.3"), None);
        assert_eq!(parse_piconero("0.0000000000001"), None);
    }

    #[test]
    fn parse_and_format_round_trip() {
        let pico = parse_piconero("1.5").unwrap();
        assert_eq!(pico, 1_500_000_000_000);
        assert_eq!(format_for_input(pico), "1.5");
    }
}
