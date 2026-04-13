# Kaspa Multisig Control Room

`kaspawallet-egui` is a native Rust desktop UI for the Go `kaspawallet` multisig flow.

## Architecture

- `eframe` / `egui` provides the desktop UI.
- `build.rs` compiles a local Go bridge from `cmd/kaspawallet-gui-backend`.
- The UI spawns that bridge as a local child process on startup.
- The bridge uses the real Go wallet code for:
  - multisig bootstrap / recovery
  - wallet summary + fingerprint
  - local daemon lifecycle
  - balance + address discovery
  - create unsigned spend bundles
  - offline signing
  - broadcast
  - transaction parsing

## Run

```bash
cargo run --manifest-path tools/kaspawallet-egui/Cargo.toml
```

The Go bridge is built automatically by the Rust build script.

## Multisig workflow reflected in the UI

1. Bootstrap: create or recover the local cosigner file, exchange `kpub...` strings, verify the shared fingerprint.
2. Receive: start the wallet daemon, sync balances, and only generate receive addresses from the canonical cosigner (`cosignerIndex = 0`).
3. Spend: create unsigned transaction bundle -> sign current hex -> sign again on another cosigner -> broadcast once fully signed.
4. Inspect: paste any bundle hex and review fees, outputs, and signature progress offline.
