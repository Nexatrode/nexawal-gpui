//! First-seen and send-time fiat snapshots for history, matching iOS/Android.

use std::collections::{HashMap, HashSet};

use crate::fiat::{self, Rate};
use crate::paths;

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub currency: String,
    pub fiat_per_xmr: f64,
    pub recorded_at_ms: u64,
    pub kind: String,
}

#[derive(Clone, Debug, Default)]
pub struct Store {
    snapshots: HashMap<String, Snapshot>,
    observed: HashSet<String>,
}

impl Store {
    pub fn load() -> Self {
        let snapshots = parse_snapshots(&read(paths::fiat_snapshots_path()));
        let mut observed = parse_observed(&read(paths::fiat_observed_path()));
        if observed.is_empty() {
            observed = snapshots.keys().cloned().collect();
        }
        Self {
            snapshots,
            observed,
        }
    }

    pub fn get(&self, txid: &str) -> Option<&Snapshot> {
        self.snapshots.get(txid.trim())
    }

    pub fn record_send(&mut self, txid: &str, rate: Option<&Rate>) {
        self.record(txid, rate, "send");
    }

    pub fn record_new_transfers<'a>(
        &mut self,
        transfers: impl IntoIterator<Item = (&'a str, Option<u64>)>,
        rate: Option<&Rate>,
        opted_in_at_ms: u64,
    ) {
        let now = fiat::now_ms();
        let mut observed_changed = false;
        let mut snapshots_changed = false;
        for (txid, timestamp_seconds) in transfers {
            let trimmed = txid.trim();
            if trimmed.is_empty() || self.observed.contains(trimmed) {
                continue;
            }
            self.observed.insert(trimmed.to_string());
            observed_changed = true;
            if !fiat::should_record_seen_snapshot(timestamp_seconds, opted_in_at_ms) {
                continue;
            }
            if self.snapshots.contains_key(trimmed) {
                continue;
            }
            let Some(rate) = rate.filter(|r| fiat::is_fresh(r.fetched_at_ms, now)) else {
                continue;
            };
            self.snapshots.insert(
                trimmed.to_string(),
                Snapshot {
                    currency: rate.currency.clone(),
                    fiat_per_xmr: rate.fiat_per_xmr,
                    recorded_at_ms: now,
                    kind: "seen".into(),
                },
            );
            snapshots_changed = true;
        }
        if observed_changed {
            save_observed(&self.observed);
        }
        if snapshots_changed {
            save_snapshots(&self.snapshots);
        }
    }

    fn record(&mut self, txid: &str, rate: Option<&Rate>, kind: &str) {
        let trimmed = txid.trim();
        if trimmed.is_empty() {
            return;
        }
        self.observed.insert(trimmed.to_string());
        save_observed(&self.observed);
        let now = fiat::now_ms();
        let Some(rate) = rate.filter(|r| fiat::is_fresh(r.fetched_at_ms, now)) else {
            return;
        };
        if self.snapshots.contains_key(trimmed) {
            return;
        }
        self.snapshots.insert(
            trimmed.to_string(),
            Snapshot {
                currency: rate.currency.clone(),
                fiat_per_xmr: rate.fiat_per_xmr,
                recorded_at_ms: now,
                kind: kind.to_string(),
            },
        );
        save_snapshots(&self.snapshots);
    }
}

fn read(path: std::path::PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn parse_snapshots(raw: &str) -> HashMap<String, Snapshot> {
    let mut map = HashMap::new();
    for line in raw.lines() {
        let mut parts = line.splitn(5, '|');
        let Some(txid) = parts.next().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(currency) = parts.next().map(str::trim) else {
            continue;
        };
        let Some(fiat_per_xmr) = parts.next().and_then(|s| s.trim().parse().ok()) else {
            continue;
        };
        let recorded_at_ms = parts
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let kind = parts.next().unwrap_or("seen").trim().to_string();
        map.insert(
            txid.to_string(),
            Snapshot {
                currency: currency.to_string(),
                fiat_per_xmr,
                recorded_at_ms,
                kind,
            },
        );
    }
    map
}

fn parse_observed(raw: &str) -> HashSet<String> {
    raw.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn save_snapshots(map: &HashMap<String, Snapshot>) {
    let mut body = String::new();
    for (txid, snap) in map {
        body.push_str(&format!(
            "{}|{}|{}|{}|{}\n",
            txid.replace('|', ""),
            snap.currency,
            snap.fiat_per_xmr,
            snap.recorded_at_ms,
            snap.kind
        ));
    }
    let _ = paths::write_bytes(paths::fiat_snapshots_path(), body.as_bytes());
}

fn save_observed(set: &HashSet<String>) {
    let mut body = String::new();
    for txid in set {
        body.push_str(txid);
        body.push('\n');
    }
    let _ = paths::write_bytes(paths::fiat_observed_path(), body.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_line() {
        let raw = "abc|USD|356.85|1|send\n";
        let map = parse_snapshots(raw);
        assert_eq!(map["abc"].currency, "USD");
        assert_eq!(map["abc"].kind, "send");
    }
}
