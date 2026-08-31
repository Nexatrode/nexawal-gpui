//! Prepare → persist → relay, matching iOS/Android crash recovery for signed sends.

use monerowalletcore::api::{self, SendResult};
use std::io;

use crate::paths;

const WALLET_ID: &str = "main_wallet";

#[derive(Debug, Clone)]
pub struct RecoveredSend {
    pub txid: String,
    pub amount: u64,
    pub fee: u64,
}

pub fn recover_pending(node_url: &str) -> api::Result<Option<RecoveredSend>> {
    let Some(json) = paths::load_pending_send().map_err(pending_journal_error)? else {
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
) -> api::Result<(String, u64, u64)> {
    if let Some(recovered) = recover_pending(node_url)? {
        return Ok((recovered.txid, recovered.amount, recovered.fee));
    }
    let json =
        api::prepare_send_filtered(WALLET_ID, node_url, to_address, amount, from_subaddress)?;
    let prepared = api::parse_prepared(&json)?;
    let sent = persist_and_relay(node_url, &json, prepared.fee)?;
    Ok((sent.txid, prepared.amount, sent.fee))
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
    let relay = persist_before_relay(
        || paths::save_pending_send(json),
        || api::relay_prepared(WALLET_ID, node_url, json),
    )?;
    paths::clear_pending_send();
    Ok(SendResult {
        txid: relay.txid,
        fee,
    })
}

fn persist_before_relay<T>(
    persist: impl FnOnce() -> io::Result<()>,
    relay: impl FnOnce() -> api::Result<T>,
) -> api::Result<T> {
    persist().map_err(pending_journal_error)?;
    relay()
}

fn pending_journal_error(error: io::Error) -> api::Error {
    api::Error {
        code: -17,
        message: format!(
            "pending send recovery data could not be read or saved; new sends are blocked: {error}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, io};

    use super::persist_before_relay;

    #[test]
    fn relay_is_never_attempted_when_journal_persistence_fails() {
        let relayed = Cell::new(false);
        let result = persist_before_relay(
            || Err(io::Error::other("disk full")),
            || {
                relayed.set(true);
                Ok(())
            },
        );

        assert!(result.is_err());
        assert!(!relayed.get());
        assert!(
            result
                .unwrap_err()
                .message
                .contains("new sends are blocked")
        );
    }
}
