//! Optional public explorer observations. This module has no wallet/core dependency.
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub const HELP: &str = "Optional. Each lookup shares your IP address and this transaction ID with explorer.nexawal.com over clearnet HTTPS, even in I2P mode. No background requests. Wallet data is unchanged.";
pub const REPORTED: &str = "Public explorer data only; your wallet verifies payments.";
pub const DISABLED: &str = "Enable Explorer details in Settings to use this feature.";
pub const STALE: &str = "Node data may be stale or incomplete.";

#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    Missing,
    Unavailable,
    Cancelled,
}
impl Failure {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Missing => "Not found on this node. This does not prove a payment is missing.",
            _ => "Explorer unavailable. Your wallet is unaffected. Try again.",
        }
    }
}

#[derive(Debug)]
pub struct Observation {
    pub status: String,
    pub fee: Option<u64>,
    pub height: Option<u64>,
    pub block_hash: Option<String>,
    pub block_timestamp: Option<u64>,
    pub confirmations: u64,
    pub node_ready: bool,
    pub observed_at: u64,
}
pub fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn parse(bytes: &[u8], txid: &str, now: u64) -> Result<Observation, Failure> {
    let invalid = Failure::Unavailable;
    if bytes.len() > 16_384 || !valid_id(txid) {
        return Err(invalid);
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| invalid.clone())?;
    let int = |name: &str| v[name].as_u64().ok_or(invalid.clone());
    let optional_int = |name: &str| {
        if v.get(name).is_some_and(Value::is_null) {
            Ok(None)
        } else {
            int(name).map(Some)
        }
    };
    let optional_string = |name: &str| {
        if v.get(name).is_some_and(Value::is_null) {
            Ok(None)
        } else {
            v[name]
                .as_str()
                .map(|s| Some(s.to_string()))
                .ok_or(invalid.clone())
        }
    };
    if int("api_version")? != 1
        || v["network"] != "mainnet"
        || v["txid"] != txid
        || !v["tip_hash"].as_str().is_some_and(valid_id)
    {
        return Err(invalid);
    }
    let chain = int("chain_height")?;
    let observed_at = int("observed_at")?;
    if chain == 0
        || observed_at == 0
        || observed_at > now.saturating_add(120)
        || now > observed_at.saturating_add(300)
    {
        return Err(invalid);
    }
    let fee = optional_string("fee_piconero")?
        .map(|f| {
            if f.is_empty() || f.len() > 20 || !f.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid.clone());
            }
            f.parse::<u64>().map_err(|_| invalid.clone())
        })
        .transpose()?;
    let height = optional_int("block_height")?;
    let block_hash = optional_string("block_hash")?;
    let block_timestamp = optional_int("block_timestamp")?;
    let confirmations = int("confirmations")?;
    let status = v["status"].as_str().ok_or(invalid.clone())?;
    match status {
        "pending"
            if height.is_none()
                && block_hash.is_none()
                && block_timestamp.is_none()
                && confirmations == 0 =>
        {
            ()
        }
        "confirmed"
            if height.is_some_and(|h| h < chain && confirmations == chain - h)
                && block_hash.as_deref().is_some_and(valid_id)
                && block_timestamp.is_some_and(|t| t > 0 && t <= now.saturating_add(600)) =>
        {
            ()
        }
        _ => return Err(invalid),
    }
    Ok(Observation {
        status: status.into(),
        fee,
        height,
        block_hash,
        block_timestamp,
        confirmations,
        node_ready: v["node_ready"].as_bool().ok_or(invalid)?,
        observed_at,
    })
}

#[derive(Default)]
pub struct Lookup {
    pub generation: u64,
    pub txid: String,
    pub visible: bool,
    pub loading: bool,
    pub result: Option<Result<Observation, Failure>>,
    pub cancel: Arc<AtomicBool>,
}
impl Lookup {
    pub fn clear(&mut self) {
        self.cancel.store(true, Ordering::Release);
        let generation = self.generation.wrapping_add(1);
        *self = Self::default();
        self.generation = generation;
    }
}
impl Drop for Lookup {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

pub fn fetch(txid: &str, cancel: Arc<AtomicBool>) -> Result<Observation, Failure> {
    if !valid_id(txid) || cancel.load(Ordering::Acquire) {
        return Err(Failure::Cancelled);
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Failure::Unavailable)?;
    runtime.block_on(async {
        let work = async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(12))
                .build()
                .map_err(|_| Failure::Unavailable)?;
            let mut response = client
                .get(format!(
                    "https://explorer.nexawal.com/api/v1/transactions/{txid}"
                ))
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(|_| Failure::Unavailable)?;
            let status = response.status();
            if ![reqwest::StatusCode::OK, reqwest::StatusCode::NOT_FOUND].contains(&status)
                || response.content_length().is_some_and(|n| n > 16_384)
                || response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.split(';').next())
                    != Some(Some("application/json"))
            {
                return Err(Failure::Unavailable);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| Failure::Unavailable)? {
                if bytes.len() + chunk.len() > 16_384 {
                    return Err(Failure::Unavailable);
                }
                bytes.extend_from_slice(&chunk);
            }
            if status == reqwest::StatusCode::NOT_FOUND {
                let error: Value =
                    serde_json::from_slice(&bytes).map_err(|_| Failure::Unavailable)?;
                if error["api_version"] != 1 || error["error"]["code"] != "not_found" {
                    return Err(Failure::Unavailable);
                }
                return Err(Failure::Missing);
            }
            parse(&bytes, txid, crate::fiat::now_ms() / 1000)
        };
        tokio::pin!(work);
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(Failure::Cancelled);
            }
            tokio::select! {
                result = &mut work => return result,
                _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => (),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn body() -> Value {
        serde_json::json!({"api_version":1,"network":"mainnet","txid":"a".repeat(64),"status":"confirmed","fee_piconero":"18446744073709551615","block_height":8,"block_hash":"a".repeat(64),"block_timestamp":900,"confirmations":2,"chain_height":10,"tip_hash":"a".repeat(64),"node_ready":true,"observed_at":1000})
    }
    #[test]
    fn exact_fee_missing_fee_and_pending_are_not_fabricated() {
        let id = "a".repeat(64);
        let data = parse(&serde_json::to_vec(&body()).unwrap(), &id, 1000).unwrap();
        assert_eq!(data.fee, Some(u64::MAX));
        assert_eq!(data.confirmations, 2);
        let mut v = body();
        for key in [
            "block_height",
            "block_hash",
            "block_timestamp",
            "fee_piconero",
        ] {
            v[key] = Value::Null;
        }
        v["status"] = "pending".into();
        v["confirmations"] = 0.into();
        v["node_ready"] = false.into();
        let data = parse(&serde_json::to_vec(&v).unwrap(), &id, 1000).unwrap();
        assert!(data.fee.is_none() && data.height.is_none() && !data.node_ready);
    }
    #[test]
    fn wrong_identity_schema_network_numbers_and_freshness_fail() {
        for (key, value) in [
            ("txid", Value::String("b".repeat(64))),
            ("api_version", 2.into()),
            ("network", "stagenet".into()),
            ("confirmations", 1.into()),
            ("confirmations", serde_json::json!(2.5)),
            ("block_height", 10.into()),
            ("fee_piconero", "18446744073709551616".into()),
            ("fee_piconero", serde_json::json!(1.5)),
            ("observed_at", 699.into()),
            ("observed_at", 1121.into()),
            ("node_ready", "true".into()),
        ] {
            let mut v = body();
            v[key] = value;
            assert!(
                parse(&serde_json::to_vec(&v).unwrap(), &"a".repeat(64), 1000).is_err(),
                "{key}"
            );
        }
        assert!(parse(&vec![b' '; 16_385], &"a".repeat(64), 1000).is_err());
    }
    #[test]
    fn closing_or_disabling_invalidates_worker_and_cancels_before_network() {
        let mut state = Lookup::default();
        let token = state.cancel.clone();
        let generation = state.generation;
        state.clear();
        assert!(token.load(Ordering::Acquire));
        assert_ne!(generation, state.generation);
        assert!(!state.visible && state.result.is_none());
        assert!(matches!(
            fetch(&"a".repeat(64), token),
            Err(Failure::Cancelled)
        ));
    }
}
