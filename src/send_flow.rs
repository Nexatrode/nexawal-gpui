//! Prepare → persist → relay, matching iOS/Android crash recovery for signed sends.

use monerowalletcore::api::{self, SendResult};

use crate::paths;

const WALLET_ID: &str = "main_wallet";

#[derive(Debug, Clone)]
pub struct RecoveredSend {
    pub txid: String,
    pub amount: u64,
    pub fee: u64,
}

pub fn recover_pending(node_url: &str) -> api::Result<Option<RecoveredSend>> {
    let Some(json) = paths::load_pending_send() else {
        return Ok(None);
    };
    let prepared = api::parse_prepared(&json)?;
    match api::relay_prepared(WALLET_ID, node_url, &json) {
        Ok(relay) => {
            paths::clear_pending_send();
            Ok(Some(RecoveredSend {
                txid: relay.txid,
                amount: prepared.amount,
                fee: prepared.fee,
            }))
        }
        Err(err) => Err(err),
    }
}

pub fn send_exact(
    node_url: &str,
    to_address: &str,
    amount: u64,
    from_subaddress: Option<u32>,
) -> api::Result<SendResult> {
    if let Some(recovered) = recover_pending(node_url)? {
        return Ok(SendResult {
            txid: recovered.txid,
            fee: recovered.fee,
        });
    }
    let json = api::prepare_send_filtered(WALLET_ID, node_url, to_address, amount, from_subaddress)?;
    let prepared = api::parse_prepared(&json)?;
    persist_and_relay(node_url, &json, prepared.fee)
}

pub fn send_max(
    node_url: &str,
    to_address: &str,
    from_subaddress: Option<u32>,
) -> api::Result<(String, u64, u64)> {
    if let Some(recovered) = recover_pending(node_url)? {
        return Ok((recovered.txid, recovered.amount, recovered.fee));
    }
    let json = api::prepare_sweep_filtered(WALLET_ID, node_url, to_address, from_subaddress)?;
    let prepared = api::parse_prepared(&json)?;
    let sent = persist_and_relay(node_url, &json, prepared.fee)?;
    Ok((sent.txid, prepared.amount, sent.fee))
}

fn persist_and_relay(node_url: &str, json: &str, fee: u64) -> api::Result<SendResult> {
    let _ = paths::save_pending_send(json);
    let relay = api::relay_prepared(WALLET_ID, node_url, json)?;
    paths::clear_pending_send();
    Ok(SendResult {
        txid: relay.txid,
        fee,
    })
}
