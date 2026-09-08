//! Prepare → persist → relay, matching iOS/Android crash recovery for signed sends.

use monerowalletcore::api::{self, SendResult};
use std::io;

use crate::paths;

const WALLET_ID: &str = "main_wallet";

/// Native work may finish while the UI is locked, but may never reopen it or let a
/// replacement wallet take over the single native slot before that work completes.
#[derive(Default, Debug, PartialEq, Eq)]
pub enum SessionOperation {
    #[default]
    Idle,
    Active,
    LockedActive,
}

impl SessionOperation {
    pub fn is_busy(&self) -> bool { *self != Self::Idle }
    pub fn start(&mut self) {
        assert!(!self.is_busy());
        *self = Self::Active;
    }
    /// Returns whether native cleanup is safe now. Presentation must lock in either case.
    pub fn lock(&mut self) -> bool {
        if self.is_busy() {
            *self = Self::LockedActive;
            false
        } else {
            true
        }
    }
    /// A true result requires cleanup instead of publishing any operation result.
    pub fn finish(&mut self) -> bool {
        std::mem::take(self) == Self::LockedActive
    }
}

/// Form/session invalidation: late RPC completions cannot authorize a changed send.
#[derive(Default)]
pub struct PreviewEpoch(u64);
pub fn preview_amount_matches(preview: Option<u64>, current: u64) -> bool {
    preview == Some(current)
}
impl PreviewEpoch {
    pub fn invalidate(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
    pub fn token(&self) -> u64 {
        self.0
    }
    pub fn accepts(&self, token: u64) -> bool {
        self.0 == token
    }
}

fn with_approved_fee<T>(
    fee: u64,
    approved: u64,
    operation: impl FnOnce() -> api::Result<T>,
) -> api::Result<T> {
    if fee > approved {
        return Err(api::Error { code: -10, message: "Network fee increased. Preview and approve the fee again; nothing was saved or broadcast.".into() });
    }
    operation()
}

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
    approved_max_fee: u64,
) -> api::Result<(String, u64, u64)> {
    if let Some(recovered) = recover_pending(node_url)? {
        return Ok((recovered.txid, recovered.amount, recovered.fee));
    }
    let json =
        api::prepare_send_filtered(WALLET_ID, node_url, to_address, amount, from_subaddress)?;
    let prepared = api::parse_prepared(&json)?;
    let sent = with_approved_fee(prepared.fee, approved_max_fee, || {
        persist_and_relay(node_url, &json, prepared.fee)
    })?;
    Ok((sent.txid, prepared.amount, sent.fee))
}

pub fn send_max(
    node_url: &str,
    to_address: &str,
    from_subaddress: Option<u32>,
    approved_max_fee: u64,
) -> api::Result<(String, u64, u64)> {
    if let Some(recovered) = recover_pending(node_url)? {
        return Ok((recovered.txid, recovered.amount, recovered.fee));
    }
    let json = api::prepare_sweep_filtered(WALLET_ID, node_url, to_address, from_subaddress)?;
    let prepared = api::parse_prepared(&json)?;
    let sent = with_approved_fee(prepared.fee, approved_max_fee, || {
        persist_and_relay(node_url, &json, prepared.fee)
    })?;
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
    fn lock_during_preview_send_or_recovery_defers_cleanup_not_privacy() {
        for _operation in ["preview", "send", "pending recovery"] {
            let mut state = super::SessionOperation::default();
            state.start();
            assert!(!state.lock(), "must not reset a wallet owned by native work");
            assert!(state.is_busy(), "opening/replacing stays blocked while locked");
            assert!(!state.lock(), "repeated lock must preserve pending cleanup");
            assert!(state.finish(), "late success/error must clean up, not show wallet data");
            assert!(!state.is_busy(), "explicit authenticated reopen is now allowed");
            state.start();
            assert!(!state.finish(), "normal foreground operation may publish its result");
            assert!(state.lock(), "idle lock can clean up immediately");
        }
    }

    #[test]
    fn increased_fee_cannot_persist_or_broadcast() {
        for (fee, approved, allowed) in [
            (11, 10, false),
            (10, 10, true),
            (9, 10, true),
            (u64::MAX, u64::MAX - 1, false),
        ] {
            let calls = Cell::new(0);
            let result = super::with_approved_fee(fee, approved, || {
                calls.set(calls.get() + 1);
                Ok(())
            });
            assert_eq!(result.is_ok(), allowed);
            assert_eq!(calls.get(), usize::from(allowed));
        }
    }

    #[test]
    fn changed_fiat_conversion_requires_new_preview() {
        assert!(super::preview_amount_matches(Some(100), 100));
        assert!(!super::preview_amount_matches(Some(100), 101));
        assert!(!super::preview_amount_matches(None, 100));
    }

    #[test]
    fn editing_or_replacing_session_invalidates_preview() {
        let mut epoch = super::PreviewEpoch::default();
        let old = epoch.token();
        assert!(epoch.accepts(old));
        epoch.invalidate();
        assert!(!epoch.accepts(old));
        let new = epoch.token();
        epoch.invalidate();
        assert!(!epoch.accepts(new));
    }

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
