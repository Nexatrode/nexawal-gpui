# Paged transaction history

Wallet shows 10 recent records and View all / pending shortcuts. Transactions has its own sidebar
entry, whole-ledger txid search, direction/pending filters and optional UTC date ranges.

The list uses GPUI uniform_list, 50-record native pages and a four-page/200-record cache. Rows and
placeholders have the same height. A ledger revision prevents page gaps/duplication during sync.
Reload anchors by txid, falling back to page zero if the transaction no longer matches. Details
are looked up by txid, not the old preview index. CSV export includes every matching page.

No network requests are caused by browsing history. Core retains its complete ledger; this change
bounds UI allocations and FFI payloads rather than discarding wallet records.

Rows and details derive confirmations from the current sync height, so a height-only update does
not leave cached counts frozen or require another page fetch. Search / From / Through use three
independent native focus handles.

The published Cargo pin is deliberately unchanged until the matching WalletCore release exists.
To build/test this local implementation from this repository:

    cargo test --offline --features ui-tests --config 'patch."https://github.com/cacaosteve/MoneroWalletCoreFFI.git".walletcore.path="../MoneroWalletCoreFFI/monero-oxide-output"'
    cargo run --release --offline --config 'patch."https://github.com/cacaosteve/MoneroWalletCoreFFI.git".walletcore.path="../MoneroWalletCoreFFI/monero-oxide-output"'

Do not commit the local-path Cargo.lock resolution. Release WalletCore and update Cargo.toml /
Cargo.lock to its final source commit before shipping. The feature requires the new history API.

71 tests and the release build passed, including bounded-page retention, calendar date validation,
live confirmation derivation, and native focus isolation. The opt-in ui-tests feature enables
headless GPUI test support; it is not enabled in normal release builds. Core additionally
tests full exports/native query compatibility and 10,000-record paging. Do a hands-on desktop
keyboard/scrollbar/window-resize pass before release; no live wallet was opened for this change.
