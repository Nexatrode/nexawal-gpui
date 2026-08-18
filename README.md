# nexawal-gpui

Desktop [nexawal](https://github.com/Nexatrode/nexawal) for **macOS, Windows, and Linux**, built with [GPUI](https://gpui.rs) (the UI crate from [Zed](https://zed.dev)).

This is a **new client**, not a port of the SwiftUI or Compose apps. Scan, restore, history, and send stay in the shared Rust wallet core.

- Desktop app: this repository
- iOS / Mac Catalyst app: [nexawal](https://github.com/Nexatrode/nexawal)
- Android app: [nexawal-android](https://github.com/Nexatrode/nexawal-android)
- Website: [nexatrode.com](https://nexatrode.com)

| Repo | Role |
| --- | --- |
| [nexawal](https://github.com/Nexatrode/nexawal) | iOS / Mac Catalyst (SwiftUI) |
| [nexawal-android](https://github.com/Nexatrode/nexawal-android) | Android (Compose) + walletcore |
| [nexawal-gpui](https://github.com/Nexatrode/nexawal-gpui) | macOS / Windows / Linux desktop (this repo) |

The Catalyst app can stay as the App Store / iPhone+iPad Mac companion. This binary is a native desktop app on all three OSes (Metal / DirectX / Wayland+X11).

## Status

Walletcore is linked as a Rust crate (`rlib`) from a pinned revision of the
source-only `cargo-walletcore` branch in
[MoneroWalletCoreFFI](https://github.com/cacaosteve/MoneroWalletCoreFFI). This
keeps the dependency fetch small and lets this repository build from a standalone
clone. The restore screen has seed and restore-height fields; **Open & sync** is
a separate step so paste does not jump into the wallet. Env vars prefill the
fields and do not auto-open.

On macOS the menu bar is `nexawal | Edit | Wallet | Window` (Zed-style: Hide, Paste, Minimize, Quit) so you can Hide or Cmd-Tab back to the running app.

Scan cache is stored at `~/Library/Application Support/nexawal/main_wallet.cache` on macOS (XDG data dir on Linux). The seed is **never written to the app data directory**. After the first restore, it is saved in the native per-user secure store: macOS Keychain, Windows Credential Manager, or Linux Secret Service. The next launch shows **Open existing wallet** and reuses the same scan cache and wallet UI without asking for the seed again.

Keyring v3 does not enable a native backend by default. This repository enables each platform backend explicitly; builds before this change used a temporary in-memory backend and require entering the seed one final time to migrate it into persistent secure storage.

## Run

Needs [Rust](https://rustup.rs/) 1.97.1 (see `rust-toolchain.toml`). First build fetches the Zed git pin, compiles GPUI, and compiles walletcore (slow).

On macOS 26, GPUI’s default Metal AOT compile needs Apple’s Metal toolchain (`xcodebuild -downloadComponent MetalToolchain`). This crate enables Zed’s `runtime_shaders` feature so `cargo run` works without that download.

```bash
cargo run
```

The development profile optimizes the complete WalletCore dependency graph, so
plain `cargo run` scans close to release speed. Use `cargo run --release` for
comparable throughput measurements across frontends.

Optional env:

```bash
export NEXAWAL_MNEMONIC="word1 word2 ... word25"
export NEXAWAL_RESTORE_HEIGHT=0
export NEXAWAL_NODE_URL=https://rpc.nexatrode.com
cargo run
```

Release:

```bash
cargo build --release
```

The release executable has the NexaWal icon embedded on Windows. On macOS,
`cargo run` installs the native padded, rounded icon in the Dock and Cmd-Tab
switcher. To create a normal signed-ready `.app` bundle with `nexawal.icns`:

```bash
cargo install cargo-bundle
cargo bundle --release
```

After changing the source artwork, regenerate the macOS PNG and `.icns` on a
Mac with `scripts/generate-macos-icon.sh`. The full-bleed source remains in use
for Windows and Linux.

Linux desktop integration is under `packaging/linux`. Its desktop filename and
icon name match the GPUI window app ID (`com.nexatrode.nexawal`), so Wayland and
X11 desktops can associate the installed launcher icon with the running window.
For a per-user installation after building:

```bash
install -Dm755 target/release/nexawal ~/.local/bin/nexawal
install -Dm644 packaging/linux/com.nexatrode.nexawal.desktop ~/.local/share/applications/com.nexatrode.nexawal.desktop
install -Dm644 packaging/linux/com.nexatrode.nexawal.metainfo.xml ~/.local/share/metainfo/com.nexatrode.nexawal.metainfo.xml
install -Dm644 packaging/linux/hicolor/256x256/apps/com.nexatrode.nexawal.png ~/.local/share/icons/hicolor/256x256/apps/com.nexatrode.nexawal.png
install -Dm644 packaging/linux/hicolor/512x512/apps/com.nexatrode.nexawal.png ~/.local/share/icons/hicolor/512x512/apps/com.nexatrode.nexawal.png
```

If you already have Zed at `~/github/zed` on the same machine, you can switch the `gpui` / `gpui_platform` deps in `Cargo.toml` to path crates for a faster compile.

## Platforms

- **macOS** — Metal + `font-kit`; runtime Dock icon and `.icns` bundle icon
- **Windows** — Win32 + DirectX with an embedded multi-resolution `.ico` and Windows Credential Manager for the stored wallet. Install MSVC + Windows SDK as in [Building Zed for Windows](https://github.com/zed-industries/zed/blob/main/docs/src/development/windows.md).
- **Linux** — Wayland and/or X11 (both features enabled), with hicolor and desktop-entry assets. Stored wallets use the desktop Secret Service provided by GNOME Keyring or a compatible KWallet integration.
