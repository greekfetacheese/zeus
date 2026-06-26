# zeus-waku-broadcaster

Rust port of the Railgun Waku Broadcaster client (`@railgun-community/waku-broadcaster-client`).

This enables Zeus to:
- Connect to the Waku P2P network for Railgun.
- Discover Railgun Broadcasters via fee announcements.
- Receive and validate fee quotes (pay gas in ERC20 instead of native ETH).
- Select the best broadcaster for a token/chain.
- Send encrypted private transaction requests and receive encrypted responses.

**This is the most complex piece of the Railgun integration** (see top-level `RAILGUN_ROADMAP.md`, Phase 1).

## Architecture (2026 decision)

Because the Rust `waku-bindings` depend on an old Nim-based FFI (`waku-sys` + zstd, etc.) that is very hard to update, we use a **Node.js sidecar** for the networking layer only.

- **js-sidecar/**: Small Node process using the **pure JavaScript** `@waku/sdk` from `logos-delivery-js` (no Nim, clean libp2p).
  - Responsibilities: start light node, subscribe to Railgun topics, publish, report peer counts.
- **Rust (this crate)**: Owns **all** Railgun logic.
  - `src/sidecar.rs`: `WakuSidecarClient` — spawns the Node process and talks to it over line-delimited JSON (stdin/stdout).
  - Fee parsing, signature verification, encryption (sharedKey + responseKey), broadcaster selection, transact message building — all in Rust.

This gives us reliable Waku behavior immediately while keeping the project mostly Rust.

## Current Status

- Sidecar protocol + basic JS implementation done.
- Rust `WakuSidecarClient` implemented and compiles.
- Topics defined (`/railgun/v2/{type}-{id}-{fees|transact|transact-response}/json`).
- High-level skeleton for `WakuBroadcasterClient`, `BroadcasterFeeCache`, `BroadcasterTransaction`.

Next milestones:
- Wire the sidecar into `start()` and receive real fee messages.
- Port fee message handling + verification from TS.
- Implement encryption + transact flow.

## How to use (when ready)

```rust
use zeus_waku_broadcaster::{WakuSidecarClient, Chain};

let mut client = WakuSidecarClient::new();
let rx = client.start_sidecar("js-sidecar/src/index.js").await?;

client.start_waku(Chain::ETHEREUM_MAINNET, None).await?;

// Subscribe to fee + transact topics for the chain
let fees_topic = format!("/railgun/v2/0-1-fees/json");
client.subscribe(vec![fees_topic]).await?;

// Later: listen on rx for SidecarMessage::Message
```

## Setup (required once)

```bash
cd crates/zeus-waku-broadcaster/js-sidecar
npm install
```

The Rust code will spawn `node js-sidecar/src/index.js` (adjust path as needed when integrating into Zeus).

## References

- TS broadcaster client: `/home/cion/Railgun/waku-broadcaster-client`
- Pure JS Waku: `/home/cion/Railgun/logos-delivery-js`
- Full plan: `RAILGUN_ROADMAP.md` (top level of Zeus)

See the roadmap for the full phased plan.

## Running the sidecar test (recommended first step)

1. Install the JS dependencies (one time):
   ```bash
   cd crates/zeus-waku-broadcaster/js-sidecar
   npm install
   ```

2. From the Zeus workspace root, run the example:
   ```bash
   cargo run -p zeus-waku-broadcaster --example waku_sidecar_test
   ```

   Or with more logs:
   ```bash
   RUST_LOG=info cargo run -p zeus-waku-broadcaster --example waku_sidecar_test
   ```

The test will:
- Spawn the Node.js Waku sidecar
- Connect to the Railgun Waku network
- Subscribe to fee announcements on Ethereum mainnet
- Print any messages it receives for ~90 seconds

If you see `📨 MESSAGE` lines, the sidecar + Waku connectivity is working!
