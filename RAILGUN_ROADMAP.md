# Zeus + Railgun Integration Roadmap

**Status**: Early research & partial implementation (zkAddress generation).  
**Goal**: Full native Rust implementation of Railgun privacy features inside Zeus (egui desktop wallet using alloy-rs). Enable "Privacy Mode" for shielding tokens, private transfers, privacy-preserving swaps, and gasless (broadcaster-paid) private transactions via the Waku P2P network.  
**Timeline**: Long-term multi-month project. Track here + Hermes `todo` tool + git commits.  
**Maintainer notes**: Update this file after every significant milestone. Use conventional commits (e.g. `feat(railgun): add waku fee cache`).

## What We Said / Project Context (Summary)

- Zeus is a seedless/self-custodial EVM wallet in Rust (eframe/egui GUI + alloy for blockchain).
- Railgun (https://railgun.org/) provides on-chain privacy via zk-SNARKs: users "shield" public tokens into private "notes", perform private actions (transfers, swaps) without revealing amounts/addresses on-chain, then unshield.
- Railgun **only** provides a TypeScript SDK (`@railgun-community/*` packages: engine, wallet, shared-models, plus Waku broadcaster client).
- To integrate:
  - Re-implement (or carefully port/adapt) core Railgun **engine** logic in Rust (key derivation, note management, Merkle trees, scanning, private tx building).
  - Implement a full **Waku Broadcaster client** in Rust (hardest part). This connects to the privacy-preserving Waku P2P gossip network to:
    - Discover broadcasters.
    - Receive live fee quotes (token fees instead of native gas).
    - Post private transaction requests (encrypted) to a broadcaster.
    - Receive execution results (tx hash or error, also encrypted).
  - Broadcasters pay the gas for your private tx in exchange for a token fee (keeps your privacy; you never touch the native token publicly for private actions).
- Cloned reference repos (for reverse-engineering message formats, encryption, topics, logic):
  - `/home/cion/Railgun/waku-broadcaster-client` (TS Waku client + fee/transact logic).
  - `/home/cion/Railgun/wallet`
  - `/home/cion/Railgun/engine`
  - `/home/cion/Railgun/poseidon-hash-wasm`
  - `/home/cion/Railgun/logos-delivery-rust-bindings` (Rust waku; published alternative `waku-bindings = "0.6.0"` recommended for crates.io).
- **Current progress (as of this doc)**: 
  - Basic zk-Railgun address (0zk... bech32m) generation implemented in `crates/zeus-railgun/src/address.rs`.
  - Uses: BabyJubJub (custom impl + curve25519-dalek), light-poseidon, HMAC-SHA512 derivation (specific paths m/44'/1984'..., m/420'/1984'...), blake2, etc.
  - Test shows close match to "Railway" wallet address (user-provided example seed produces similar but not byte-identical 0zk address — acceptable for now).
  - No Waku, no notes, no shielding, no engine, no UI yet.
- Dependencies already in zeus-railgun: alloy-*, secure-types, light-poseidon, ark-*, curve25519-dalek, chacha20poly1305, bech32, etc.
- Zeus structure: root binary crate (src/main.rs + gui/core/utils) + workspace crates (zeus-eth for alloy/revm, zeus-wallet, zeus-bip32, zeus-*-theme/ui/widgets, **zeus-railgun**).
- Plan: New dedicated crate `crates/zeus-waku-broadcaster` (modular). Core privacy logic stays/grows in `zeus-railgun`. Integrate into zeus-wallet + GUI later.
- Challenges: 
  - No official Rust SDK.
  - Complex ZK (Poseidon Merkle trees, Groth16 proofs for shield/unshield/transact/relay-adapt).
  - Waku protocol specifics + custom Railgun message encryption/signing (ECDH shared keys, AES-GCM responses, broadcaster signatures).
  - Event scanning + local private state (commitments/nullifiers).
  - Secure key handling (viewing + spending keys) compatible with existing Zeus derivation.
  - Proof artifacts / circuit reimplementation.
  - Multi-chain (Ethereum, Polygon, Arbitrum, etc.), POI (Proof of Innocence) lists, versioned txid (V2_PoseidonMerkle).
- Success criteria: User can shield → see private balance → private swap/transfer via broadcaster (gas paid in token) → unshield, all without leaking to public chain in the privacy path. Full feature parity with official Railway/Railgun wallets where reasonable.

## Railgun High-Level Architecture (for Rust Port)

1. **Addresses / Keys** (partial):
   - 0zk... bech32m encoded (version + master pubkey + xor'd network + viewing pubkey).
   - Spend key (for signing private actions), Viewing key (for decrypting incoming notes).
   - Derivation from seed (specific hardened paths, clamping for ed25519/babyjub).

2. **Notes & Private Balances**:
   - Shield creates a "note" (commitment = Poseidon hash of (pubkey, amount, token, random)).
   - Notes live in on-chain Merkle tree (Poseidon).
   - To spend: generate nullifier, inclusion proof, ZK proof that you own the note without revealing it.
   - Local engine maintains: commitment tree, nullifier set, decrypted notes (using viewing key).

3. **Transactions**:
   - Shield (public ERC20/ETH → private note).
   - Unshield (private → public).
   - Private transfer (note → note(s)).
   - Private swap (via RelayAdapt contract? for Uniswap/etc integration without leaking).
   - All private actions go through Railgun smart contracts (shield, relay, etc.).

4. **Waku Broadcaster Layer** (critical for "full privacy" + gas abstraction):
   - Waku (libp2p-based P2P, sharded pubsub) replaces direct RPC to broadcaster.
   - Topics (per chain): `/railgun/v2/{type}-{id}-fees/json`, `-transact/json`, `-transact-response/json`.
   - Fee flow: Broadcasters announce signed fee schedules (token → fee rate, expiration, version, railgunAddress). Client verifies sig using broadcaster's viewing pubkey, caches best rates.
   - Transact flow:
     - Client picks broadcaster + fee quote.
     - Builds private tx data (nullifiers, commitments, proofs, calldata).
     - Encrypts payload (random responseKey + ECDH sharedKey from random privkey + broadcaster pubkey).
     - Sends `{method: "transact", params: {pubkey, encryptedData}}` on transact topic.
     - Broadcaster decrypts, executes (pays gas), responds encrypted (AES-GCM with responseKey) on response topic.
     - Client decrypts matching responseKey.
   - Also historical polling, peer counts, health, DNS/ENR discovery, specific cluster/shard (currently cluster 5 / shard 1).

5. **Engine / Prover**:
   - Merkle trees (Poseidon), note decryption/encryption, nullifier computation.
   - Proof generation (artifacts + snarkjs in TS; need Rust equivalent: arkworks-groth16 + circuit impl or wasm bridge).
   - POI (optional compliance lists), artifact management.
   - TXID versions.

6. **On-chain**:
   - Alloy calls to Railgun contracts (addresses per chain known from official).
   - Event logs for note scanning (no full node required; use public RPC + filters or logs).

7. **UI/UX in Zeus**:
   - Toggle Privacy Mode.
   - Shield/Unshield flows (with fee quote preview).
   - Private balances (decrypted notes).
   - Private send/swap forms that use broadcaster.
   - Settings: broadcaster config, trusted signers, POI lists.

Dependencies to add/evaluate:
- `waku-bindings = "0.6.0"` (or local fork).
- Full arkworks stack if doing native ZK (ark-groth16, ark-bn254, etc. — already partial).
- Perhaps `circom-compat` or custom prover.
- tokio (already in Zeus), serde, etc.
- For encryption: match TS (likely secp256k1 or babyjub ECDH + aes-gcm/chacha).

## Full Phased Roadmap

### Phase 0: Foundations, Research, Documentation (Current)
**Goal**: Solid base + shared understanding. No code breakage.
**Tasks**:
- [x] Summarize goals + write this roadmap (this file).
- [ ] Deep-dive all cloned TS repos: extract exact message schemas, encryption code, signature verification, constants, test vectors.
- [ ] Document Railgun contract ABIs / addresses (per chain), event topics for scanning.
- [ ] Research exact crypto for encryption/signing in broadcaster (from wallet/engine utils: `encryptDataWithSharedKey`, `verifyBroadcasterSignature`).
- [ ] Decide crate layout: `zeus-waku-broadcaster` (new) vs. submodules in `zeus-railgun`.
- [ ] Add Railgun section to root README + any AGENTS.md notes.
- [ ] Setup test vectors (use same mnemonic as user's Railway test).
- [ ] Evaluate `waku-bindings` API (connect, pubsub subscribe/publish, light node, discovery). Compare to logos-delivery.
- **Deliverables**: Updated RAILGUN_ROADMAP.md, initial notes/memory entries, research doc or comments in code.
- **Verification**: Another dev (or future you) can read this + run existing address tests.
- **Blockers/Risks**: Missing shared-models source; Waku Rust bindings maturity.

**Estimated**: 1-2 weeks (research heavy).

### Phase 1: Waku Broadcaster Client (Highest Priority - Hardest)
**Goal**: Standalone working Rust client that can discover broadcasters, receive valid fee quotes, select best, and round-trip an (even dummy) transact message/response.
**Why first**: Unblocks everything else; most unique to "full privacy covering gas"; lots of protocol reverse-eng.
**New Crate**: `crates/zeus-waku-broadcaster`
  - Add to workspace `Cargo.toml`.
  - Public API mirroring TS where possible: `WakuBroadcasterClient::start(chain, options)`, `findBestBroadcaster`, `BroadcasterTransaction::create(...).send()`.
**Key Subtasks**:
1. Basic crate skeleton + deps (`waku-bindings`, tokio, serde, anyhow, hex, etc.).
2. Waku core: init LightNode equivalent, peer discovery (DNS/ENR + direct), connection management, health.
3. Topics & observers: subscribe to fees + transact-response per chain; content topic formatting; dedup cache.
4. Fee handling:
   - Parse `BroadcasterFeeMessageData`.
   - Signature verification (port `verifyBroadcasterSignature` + address data extraction).
   - Expiration/timestamp logic (handle Waku timestamp bugs).
   - Version checks.
   - `BroadcasterFeeCache` equivalent (per-token fees, authorized fees, variance checks).
5. Broadcaster search/selection: `findBestBroadcaster(chain, token, useRelayAdapt)`.
6. Transact:
   - Encryption flow (responseKey, sharedKey derivation, encryptData, pubkey inclusion).
   - Message construction.
   - Send on transact topic.
   - Listen for + decrypt responses using responseKey (symmetric).
   - Polling/retry/timeout logic.
7. Config: BroadcasterOptions (trustedFeeSigner, timeouts, peers, shards, version range).
8. Tests: unit tests mirroring the TS `__tests__` (fee cache, handle messages, broadcaster tx, live peer if possible). Use same fixtures.
9. Debug logging + status callbacks.
10. Integration test: start, wait for real broadcasters on a testnet/mainnet (with real RPC? no, pure Waku), print fees.
**Deliverables**:
- Working `zeus-waku-broadcaster` crate that can be used independently.
- Example binary or test that prints live fee quotes for ETH mainnet or Goerli/Sepolia.
- Docs in crate.
**Verification**: Matches TS client behavior on same Waku network. Can receive real signed fee messages and select a broadcaster.
**Dependencies**: Phase 0 research on encryption details. May need to implement custom crypto (k256 or jubjub ECDH?).
**Risks**: Waku bindings API differences from @waku/sdk; live network flakiness; exact sig/encrypt algorithms.
**Milestone**: "Waku client can discover and quote fees for a token."

**Estimated**: 4-8 weeks (biggest chunk).

### Phase 2: Complete Keys, Addresses, Basic Note Model
**Goal**: Production-grade address + key management. Basic note struct + encryption/decryption.
**In `zeus-railgun`**:
- Fix/refine address generation + full test suite against official vectors (if available) or more Railway examples.
- Extract SpendKey, ViewingKey, NullifyingKey properly (secure).
- Support chain-specific + index derivation.
- Note struct: (token, amount, pubkey, random, commitment, nullifier).
- Encryption/decryption of notes (Railgun uses specific scheme, often Poseidon + symmetric).
- Export/import viewing keys safely.
- Integration with existing `zeus-wallet` (derive_seed etc.).
**Deliverables**: Updated address + new `keys.rs`, `note.rs`. Tests pass for address + roundtrip note encrypt/decrypt.
**Verification**: Generate addresses match user's examples + new test vectors; notes roundtrip.

**Estimated**: 2-3 weeks (builds on existing).

### Phase 3: On-Chain Shield / Unshield + Contract Layer
**Goal**: Public <-> Private token movement using Railgun contracts.
**Tasks**:
- Add Railgun contract ABIs (shield, unshield, relay, etc.) — copy/adapt from engine/abi.
- Functions: `shield(erc20, amount, railgunAddress, ...)` → returns commitment.
- Unshield using private note proof (but proof gen later).
- Event parsing for `Shield`, `Unshield`, `Transact` events.
- Use alloy-provider to call + listen (integrate with zeus-eth).
- For broadcaster path: build the "adapt" calldata if needed.
**Deliverables**: `contracts.rs` or module, shield/unshield builders that output calldata + events.
**Verification**: Can call shield on a testnet with public funds → see event. (Unshield needs Phase 4/5).

**Estimated**: 2-3 weeks.

### Phase 4: Engine Core — Scanning, Merkle Trees, Private State
**Goal**: Local private balance from on-chain data. Ability to "see" your shielded notes.
**Tasks** (port from engine/merkletree + note + database):
- Poseidon Merkle tree implementation (or use/extend light-poseidon + custom tree).
- Commitment insertion, root calculation, membership proofs.
- Nullifier set (to prevent double-spend).
- Scanner: listen to Railgun contract events (via alloy logs), decrypt notes for our viewing key, store in local DB (sled already dep).
- Balance calculator: sum unspent notes per token.
- POI list handling (fetch/validate if required).
- Historical sync + incremental update.
**Deliverables**: `engine/` module or subcrate logic, `scanner.rs`, tree impl, local store for notes.
**Verification**: Shield some tokens (Phase 3), run scanner, see decrypted balance in tests. Merkle proof verifies.

**Estimated**: 4-6 weeks (state management heavy).

### Phase 5: ZK Proofs + Private Transact / Swap Logic
**Goal**: Actually build and prove private transactions (the core privacy primitive).
**Tasks**:
- Research exact circuits (from engine/prover).
- Options for proving in Rust:
  a. Re-implement circuits in arkworks (accurate but long).
  b. Bundle proving keys + use ark-groth16 verifier + external or embedded prover (or wasmtime for wasm prover).
  c. Hybrid: call out to a small node process or prebuilt binary for proof gen (not ideal).
- Implement: generate proof for private transfer (nullifiers + new commitments + ownership).
- Build full `BroadcasterRawParamsTransact` or equivalent.
- Support for "RelayAdapt" (for private DEX swaps).
- TXID version handling.
- Combine with Waku (from Phase 1): send real private tx via broadcaster.
**Deliverables**: Proof generator (at least verifier + one action), private tx builder, end-to-end private transfer test (mock or testnet via broadcaster).
**Verification**: Shield → private transfer (via broadcaster) → balance updates correctly on both sides. Tx appears on-chain but private.

**Estimated**: 6-10+ weeks (the ZK mountain). Start with verifier + simple cases; full proving may be iterative.

### Phase 6: Wallet Integration & Core Features
**Goal**: Railgun usable from Zeus core (not just lib).
- Add `RailgunWallet` wrapper in `zeus-wallet` or `zeus-railgun`.
- Secure storage of viewing/spend keys (using existing encryption).
- Private balance queries, note selection for spends.
- Integration points for tx building in `src/core/tx/`.

**Estimated**: 2-3 weeks.

### Phase 7: GUI & User Experience
**Goal**: Polished, safe UI for privacy features.
- New panels: Privacy dashboard, Shield form (token select + amount + preview fee), Private send, Private swap (Uniswap via adapt?).
- Visuals: Private balances distinct (perhaps theme), warning banners ("Privacy Mode Active").
- Settings: Enable Railgun, Waku status (peer count, connected broadcasters), trusted fee signer, network selection.
- Use zeus-widgets/theme/ui-components.
- QR for 0zk addresses, copy, etc.
- Fee quote display, broadcaster reliability indicators.
- Error handling for "no broadcaster available", expired quotes.
**Deliverables**: Working UI flows in the egui app. End-to-end demo: shield → private tx → unshield.
**Verification**: Manual testing on testnet + screenshots / video in repo.

**Estimated**: 3-4 weeks.

### Phase 8: Advanced Features, Polish, Multi-Chain, Testing
- Full swap support (private → private via DEX).
- POI lists + compliance.
- Multiple indices / "accounts" in privacy.
- Gas estimation + fallback (direct pay gas vs broadcaster).
- Performance: tree updates, proof caching, parallel scanning.
- Multi-chain support + chain switching in Waku client.
- Comprehensive tests (unit + integration with real Waku + testnet contracts). E2E with Zeus app.
- Security: constant-time where possible, zeroize, audit notes.
- Docs, examples, migration guides.
- Optimization: reduce deps, binary size.
- Optional: native artifacts for proofs.

**Estimated**: Ongoing + 4-6 weeks.

### Phase 9: Release & Maintenance
- Feature flag `railgun` in Zeus.
- Release notes, blog? 
- Monitor for Railgun protocol upgrades (new txid versions, new Waku shards, contract changes).
- Community: contribute back any general Rust crates if useful (e.g. poseidon railgun variant).

## Progress Tracking & Process

- **This file** is the source of truth for phases. Mark items with [x] or dates when done. Add "Completed: YYYY-MM-DD" notes.
- **Hermes tools**: Use `todo` for session tasks, `memory` for durable facts, `session_search` for history. After big phases, consider `skill_manage` to capture workflow.
- **Code**:
  - Keep `zeus-railgun` focused on privacy primitives.
  - Waku in its own crate.
  - Never mutate past conversation context (per AGENTS.md).
  - E2E test with real paths (not just mocks).
- **Testing discipline**: No change-detector tests. Use real vectors from Railgun repos. Test against live Waku when possible.
- **Commits**: Conventional. Scope: `railgun-waku:`, `railgun-engine:`, `feat(zeus): privacy mode ui`.
- **Blockers**: Log in this file under "Open Issues".
- **Review**: Before merging big PRs, run full test suite (`scripts/run_tests.sh` if exists).

## Open Issues / Unknowns (update regularly)
- Exact Rust equivalent for broadcaster encryption (need to port `encryptDataWithSharedKey` and sig verify precisely).
- ZK proving strategy (biggest unknown — may require community help or accepting wasm bridge).
- Official Railgun Rust efforts? (search periodically).
- Performance of Waku in Rust bindings on desktop.
- How Zeus "seedless" derivation maps perfectly to Railgun paths (current custom m/ paths in address.rs).
- Artifact licensing / size for ZK keys.
- Full list of supported chains + contract addresses.

## Resources
- Official: https://railgun.org/
- Dev docs: https://docs.railgun.org/developer-guide
- Cloned sources (local only).
- Waku: https://waku.org/
- Current address test seed (in `address.rs` tests).

---

**Next immediate actions after this doc**:
1. Mark Phase 0 research tasks.
2. Begin Phase 1 crate setup + Waku bindings experiment (as you suggested — start here).
3. Extract encryption + message schemas from TS into Rust-friendly structs first.
4. Regular updates to this file + memory/todo.

Let's build this step by step. Never give up on the right (private, modular, correct) solution.

*Document generated with assistance from Brainiac (Hermes Agent). Update frequently.*

---

## Waku Implementation Strategy Update (Critical - Added Later)

After investigation of the cloned repos:

**The problem with Rust bindings**
- `logos-delivery-rust-bindings` (and its `waku-sys`) is **not** a pure Rust implementation.
- It is FFI bindings to a **Nim** implementation (`logos-messaging-nim` / nwaku / libwaku).
- The build process vendors and compiles a lot of native code (zstd, NAT traversal, miniupnp, libbacktrace, etc.).
- This is why forking + updating dependencies hits a wall very quickly.

**The good news**
- `/home/cion/Railgun/logos-delivery-js` is the **official pure JavaScript/TypeScript** implementation of the Waku v2 protocol.
- It is what the official `@railgun-community/waku-broadcaster-client` actually uses under the hood.
- It has **no Nim**, no native dependencies in the packages we care about.
- It is built on modern JS libp2p (`@chainsafe/libp2p-gossipsub`, noise, etc.).
- Key exports for us: `createLightNode`, light push, filter, store, discovery (ENR + DNS), content topic encoding.

**Feasibility of writing Waku from scratch in Rust**

- **Full Waku v2 protocol**: No. Not practical. It would require re-implementing a large portion of a libp2p node + many Waku-specific protocols (sharding, relay, lightpush, filter, store, metadata, RLN, etc.). This is years of work by dedicated teams.

- **Minimal subset needed for Railgun broadcasters**: More realistic.
  - What we actually need (from studying the TS client):
    - Connect as a light client
    - Subscribe to specific content topics (`/railgun/v2/...-fees/json`, `-transact/json`, `-transact-response/json`)
    - Send via LightPush
    - Receive via Filter/Relay observers
    - Basic peer discovery (DNS ENR tree + direct peers)
    - Historical message retrieval (Store)
    - Peer count / health information
  - Many of the building blocks already exist in the Rust ecosystem (`libp2p` crate has GossipSub, Noise, TCP, DNS, multiaddr, ENR, etc.).
  - Still a **significant** multi-month project with protocol fidelity risks and ongoing maintenance.

**Recommended path forward (pragmatic)**

**Use a Node.js sidecar process** (strongly recommended for the near term):

1. Create a small, dedicated Node script (or use the existing broadcaster client logic) that only handles Waku networking.
2. It uses the pure, well-maintained JS Waku from `logos-delivery-js`.
3. Communication with the Rust `zeus-waku-broadcaster` crate happens over a narrow, well-defined channel:
   - JSON lines over stdin/stdout (simplest)
   - Or a localhost HTTP server (slightly more overhead but easier to debug)
4. Rust owns all the Railgun domain logic (fee parsing + verification, broadcaster selection, encryption with sharedKey/responseKey, transaction construction).
5. The sidecar only does the "dumb pipe" Waku operations.

**Advantages**:
- We get working, correct Waku behavior immediately.
- No native compilation nightmares.
- We can still write the majority of the logic in Rust.
- Easy to test and iterate.
- If a good pure-Rust Waku light client appears later, we can swap the transport layer.

**Disadvantages**:
- Requires Node.js to be present (acceptable for a desktop wallet; we can bundle or auto-install a minimal runtime).
- Slightly more process management.

Alternative longer-term options:
- Implement the minimal subset directly on `rust-libp2p`.
- Contribute to or wait for community Rust Waku efforts.
- Hybrid (sidecar now, pure Rust later).

We should update Phase 1 of this roadmap to reflect the sidecar strategy as the primary short/medium-term approach.


## Waku Sidecar Architecture (Adopted - Pure JS + Rust IPC)

**Decision (June 2026)**: Use Node.js sidecar for Waku.

- **Node sidecar** (`crates/zeus-waku-broadcaster/js-sidecar/`):
  - Uses pure `@waku/sdk` from logos-delivery-js ecosystem (no Nim, no native deps).
  - Only responsibility: connect, subscribe to Railgun content topics, publish, report peers.
  - Communication: line-delimited JSON over stdin/stdout.

- **Rust side** (`crates/zeus-waku-broadcaster/src/sidecar.rs` + high-level client):
  - Spawns the sidecar using `tokio::process`.
  - Sends commands (start, subscribe, publish, status).
  - Receives events (messages, peer updates).
  - All Railgun logic (fee parsing, signature verification, encryption, transact, broadcaster selection) stays in Rust.

**Benefits**:
- Immediate correct Waku behavior.
- No native zstd / Nim hell.
- Most code remains in Rust (your preference).
- Easy to test the sidecar standalone.

**How to run**:
1. cd crates/zeus-waku-broadcaster/js-sidecar
2. npm install
3. (Rust will spawn `node src/index.js` relative path)

**Protocol** (defined in sidecar.rs + JS):
- Commands have `"id"` and `"cmd"`.
- Events have `"type"`.
- Payloads are base64.

This replaces the previous plan of using `waku-bindings` directly.

Next: Implement listening for fee messages in Rust, using the sidecar event stream.


---

## Milestone Update (2026-06-26)

**Waku Sidecar Networking — SUCCESS**

- 9 stable mesh/pubsub peers achieved on Railgun cluster 5 / shard 1 (Ethereum mainnet).
- Direct dialing of known relays now works (`waku.dial`).
- Subscribe to `/railgun/v2/0-1-fees/json` succeeds reliably.
- Sidecar receives Waku messages and forwards them as events.
- Rust `WakuSidecarClient` + typed JSON protocol is solid.
- `examples/waku_sidecar_test` now attempts to parse incoming messages using new `SignedBroadcasterFeeMessage` / `BroadcasterFeeMessageData` models.

**Next focus (Phase 1 continuation)**:
- Receive + fully parse real fee quotes in Rust.
- Implement signature verification (using broadcaster's viewing public key).
- Build `BroadcasterFeeCache`.
- Implement broadcaster selection.
- Move from "we have peers" → "we can quote fees and select a broadcaster".

This marks the end of the pure networking bootstrap phase. We are now doing real Railgun protocol work in Rust.

**Updated task priority**: `railgun-fee-quotes` is now the active implementation task.


---

## 2026-06-26 Update — Focus on Reliable Fee Reception (Option A)

Decision: Before building higher-level transact / selection logic, make sure we can actually **receive real fee messages** reliably.

Actions taken:
- Added `handleQueryHistorical` + `query_historical` command to the sidecar.
- Sidecar now supports `waku.store.queryGenerator` with time range (defaults to last 4h).
- Rust client exposes `client.query_historical(...)`.
- Example now automatically issues a historical query right after subscribing to the fees topic.
- Messages from historical queries are delivered the same way as live ones (with `source: "historical"`).
- Fee parsing (SignedBroadcasterFeeMessage + BroadcasterFeeMessageData) is already wired in the example.

Next test run should pull historical fee announcements even if no live ones are published during the session.

This is the current active focus for Phase 1.


### 2026-06-26 Follow-up Run
- Networking improved: bootstrap dials now succeed, reached 8 mesh peers quickly.
- Historical query was issued too early (right after subscribe) → "No peers available to query".
- **Fixes applied**:
  - JS sidecar: `handleQueryHistorical` now retries up to 3 times (with 8-12s delays) when Store reports no peers.
  - Example: Added explicit "wait for peers" phase (up to 90s, breaks early on mesh>=3 or any message).
  - After waiting, issues historical query.
  - Added periodic re-query of historical every ~3 minutes while message_count < 3 (to catch infrequent fee announcements via Store).
- This directly addresses the "wait a bit before querying" observation.
- Still no fee messages in this run (query failed early due to timing), but foundation for reliable reception is now stronger.
- Next run should have much better chance of seeing historical + live fee data.


### 2026-06-26 Latest Run Analysis + Fixes
**Run summary**:
- Bootstrap dials now succeed reliably.
- Reaches 9 mesh/pubsub peers very quickly.
- Wait for peers in example triggered correctly (mesh~4 then sustained).
- Historical query still returns "No peers available to query" on all 3 attempts.
- Even with 9 peers and retries, Store protocol finds 0 usable peers.
- No live fee messages observed.

**Root cause identified**:
Mesh/pubsub peers (for gossip/filter/lightpush) ≠ Store peers.
The @waku store peerManager is not finding any peers advertising the Store protocol for the Railgun shard/content topic.

**Changes applied**:
- Sidecar `handleStart`: Added dedicated long wait (up to 3 minutes) specifically for `['store']` protocol after general connectivity.
- `handleQueryHistorical`: Before every attempt, explicitly calls `waitForRemotePeer(waku, ['store'], 20000)`.
- Example: Increased patient peer wait to 180s + requires sustained mesh >=5 for 20s before first historical query (to give discovery time).
- Periodic historical re-query logic remains.

These changes make the client much more patient with Store discovery.

**Next run expectation**:
The "started" message may take longer (because of the 3min store wait).
After that, the example will wait up to 3min for good mesh.
Then historical query should have better chance because sidecar waited for store peers.

If it still fails with "no peers available", we will need to:
- Explicitly configure/hardcode known store peers (Railgun has `storePeers` in config).
- Or find which specific peers in the fleet provide Store for fees.
- Or accept that historical Store is unreliable for fees and focus on live + other mechanisms.


### 2026-06-26 - Pinpointing Store Peer Issue (3 action points executed)

User confirmed even with long waits + retries: still 0 store peers.

Implemented the 3 points:

1. **Hardcode + force known store peers**
   - Using the same 3 Railgun relays as store candidates (matching what TS client does with `storePeers`).
   - In every historical query attempt: ask peerManager for store peers.
   - If any, **force** `peerId` in `queryGenerator(options)` (bypasses peerManager selection).
   - Fallback: force one of the general connected peers (diagnostic).

2. **Better diagnostics**
   - New helpers: `getStorePeers()` using `waku.store.peerManager.getPeers({ protocol: Protocols.Store })`
   - `getConnectedPeerIds()`
   - Status now includes `storePeers: N`
   - Logs:
     - "Store peers after dedicated wait: X"
     - "(attempt N) Current store peers from peerManager: Y"
     - "After waitForRemotePeer store: Z"
     - When forcing a peerId (from store or fallback connected)
   - Subscribed to `store:connect` events (logs "🟢 Store peer connected event received")
   - Example now prints `store=N` in every status line.

3. **Explicit store-oriented dialing / ingress**
   - Already dialing the 3 known peers at start (waku.dial).
   - Added dedicated 3-minute `waitForRemotePeer(waku, ['store'], ...)` during start.
   - Re-wait for store before each query attempt.
   - Forcing the peerId directly in queries (the main new lever).

These changes should give us much clearer signal on whether:
- peerManager ever sees store peers (even 1)
- Forcing a peerId bypasses the "no peers" error
- Store connect events ever fire
- The forced peers actually support the protocol or return data/errors

Next run will be very informative for root cause.


### 2026-06-26 Analysis of Diagnostic Run (after implementing the 3 pinpoint actions)

**Run results (with initial diagnostic code):**
- 3 known relays dialed successfully.
- Reached 8 mesh peers.
- storePeers reported by peerManager: **always 0** (even after long dedicated waits).
- No `store:connect` events ever fired.
- "Error getting store peers: Protocols is not defined" (our diagnostic bug).
- When forcing a connected peerId into query: new error "Both pubsubTopic and contentTopics must be set together for content-filtered queries" (instead of the previous "No peers available").
- 0 historical messages, 0 live fee messages.

**What the 3 actions revealed:**
1. Hardcoding/Forcing: The peerManager never surfaces any Store peers. Forcing a general peer changes the error mode (good sign that the "no peers" was the selection layer).
2. Diagnostics: peerManager.getPeers({protocol: 'store'}) == 0. storePeers in status = 0. Connected peers exist but none are Store-capable for this topic.
3. Explicit dialing: The 3 relays we dial do not provide Store (or are not selected by the peer manager for Store on cluster 5/shard 1).

**Immediate code fixes applied after this run:**
- Fixed Protocols require for CommonJS (`wakuInterfaces.Protocols`).
- Added `pubsubTopic` + `contentTopics` to queryOpts when forcing peerId (to satisfy validation).
- Much better logging: now prints actual peer IDs for both connected and store attempts.
- Status now reliably shows store count.

**Current working hypothesis:**
The Railgun Waku fleet's "relay-a / relay-b / client-edge" nodes (as currently dialed over wss) do not expose the Store protocol to light nodes for the fees topic, or the @waku/sdk peerManager for Store is not discovering them properly with our current config.

The official TS client likely has extra `storePeers` configuration or uses different ingress nodes / TCP for Store.

Next step: Re-run with the fixes above. If we still see 0 store peers + actual peer IDs logged, we will:
- Try the TCP variants (port 30304) that appear in some Railgun tests.
- Add explicit `additionalDirectPeers` or manual store peer injection if the API allows.
- Consider falling back to pure live Filter + accepting that historical Store for fees may not be reliable in this setup.


### 2026-06-26 Latest Run + Config Alignment Fixes

**Run observations**:
- 6 bootstrap peers listed (3 WSS + 3 TCP).
- WSS dials succeed, TCP fail (expected).
- "waitForPeers(store) completed" logged.
- But `getPeers({protocol: 'store'})` still 0.
- No store:connect event this time (user noted "worse").
- "Protocols is not defined" still in logs (import issue).
- 0 historical, 0 live fee messages.
- Peer count stable at 7-8.

**Root causes identified from log + TS code review**:
1. The `store: { peers: [...] }` option was **not present** in the createLightNode call during this run (critical mismatch with official client).
2. Protocols import was broken (ESM + require mix).
3. peerManager.getPeers for Store returns 0 even when waitForPeers succeeds. This is common if the peers don't advertise the Store protocol or the peerManager hasn't negotiated it.
4. The 3 relays we use may primarily be Filter/LightPush ingress; Store may require different peers or explicit connection establishment.

**Fixes applied in this iteration**:
- Switched to proper ESM import for `@waku/interfaces` (Protocols now defined).
- Added `store: { peers: bootstrapPeers }` to createLightNode (matches exactly what the official Railgun waku-broadcaster-client does in peer-discovery-core-base.ts).
- Updated startup log to confirm store config.
- Made getStorePeers error logging less noisy.
- TCP variants kept for experimentation but expected to fail on WSS nodes.

**Next run expectations**:
- "Store configured with explicit peers for historical queries"
- No more "Protocols is not defined"
- Better chance that peerManager will report store peers > 0 after the waits.
- If still 0, we will add manual `waku.dial()` for store + try query with forced peerId and capture the actual query result/error (instead of just "no peers").


### 2026-06-27 Breakthrough — Historical Store Queries Now Working

**Major milestone achieved:**
- First real Railgun fee messages received via Store!
- 6+ parsed `BroadcasterFeeMessageData` entries (version 8.2.3, different broadcasters, token fees 4–5).
- Explicit `store: { peers: [...] }` + matching `waitForPeers([filter, lightpush, store])` was the key (aligned with official TS client).
- Queries succeed even when `peerManager.getPeers({protocol: 'store'})` returns 0 (we force a connected peerId and it works).

**Remaining small issues addressed in this session:**
- Default historical window reduced from 6h → 5 minutes (prevents huge dumps).
- Added hard cap (50 messages per query attempt) + only retry on 0 messages.
- Rust example now prints only first 8 detailed messages + periodic summaries.
- Added graceful shutdown (SIGINT/SIGTERM + EPIPE handling) so Ctrl+C no longer crashes the sidecar.
- Better logging around "Store configured with explicit peers".

**PeerManager observation (still open):**
- `getPeers({ protocol: 'store' })` consistently returns 0 even after successful queries and `waitForPeers`.
- Possible reasons:
  - The `store: { peers }` option at creation time makes the Store subsystem use the peers directly without populating the peerManager's "known store peers" list.
  - Many nodes on the Railgun fleet may not fully advertise the Store protocol in the way the peerManager API expects.
  - Forcing a peerId from the connected list bypasses this and works.

  We can treat "peerManager store count" as a secondary diagnostic only. The important thing (successful queries + real fee data) now works.

**Next immediate actions (per user):**
- Rerun the cleaned test.
- Once stable, move on to proper Rust-side fee handling (cache, signature verification, findBestBroadcaster, etc.).
- Keep the sidecar lean.


### 2026-06-27 Regression + Fix (Timing of 'started')

**Problem in this run**:
- Sidecar was doing long store waits *before* sending the 'started' event.
- Rust timed out after 60s waiting for 'started', then issued query too early.
- The running binary still had the old 6h window code.
- Result: 0 messages.

**Fixes applied**:
- Sidecar now emits 'started' + starts peer reporter **immediately** after `waku.start()` + explicit dials.
- Long `waitForPeers` + store waits now happen *after* 'started' is sent (non-blocking for the client).
- Rust example:
  - Increased 'started' timeout to 180s.
  - Always sends explicit last-5-minute window for the initial historical query.
  - All log strings updated from "last 6h" to "last 5 minutes".
- This should make the first query happen at the right time again while keeping the reliable Store config.

**Recommendation for next run**:
- `cargo run -p zeus-waku-broadcaster --example waku_sidecar_test`
- You should see 'started' much faster now.
- First historical query will request only the last 5 minutes.


### 2026-06-27 Revert to Working Historical Query Timing

User requested revert to the exact timing/behavior where historical Store queries were successfully delivering real fee messages (the run with 6+ parsed BroadcasterFeeMessageData).

Reverted changes:
- Sidecar now performs the store waits (waitForPeers + dedicated store wait) **before** emitting the 'started' event.
  - This is the timing from the successful run.
- Shortened max waits (90s per phase instead of 120-240s) to reduce the "I have to wait forever" feeling.
- Rust example:
  - Keeps patient wait for 'started' (up to 180s).
  - Uses explicit last-5-minute window for the first historical query.
  - 30s peer wait after subscribe (reasonable now that 'started' implies store readiness).
- Kept all the good cleanups from before: 5min default, message caps, graceful shutdown, EPIPE handling, better logging.

The goal is to get back to the state where we saw:
  ✅ Fee from historical | railgun=0zk... | version=8.2.3 | tokens=X

Once we confirm it works again, we can polish further (e.g. send a separate "store_ready" event, or make waits non-blocking but with better signaling).


### 2026-06-27 Peer Discovery Improvements (before continuing higher layers)

User requested one more round of peer discovery hardening because historical queries are still flaky (works only sometimes, even with good mesh).

**Changes applied**:
- Proper use of `enrTree()` helper from @waku/discovery (matching official client pattern).
- New `ensureStrongConnectivity()` function that re-dials all known Railgun relays aggressively.
- Called multiple times: after initial dials, before sending 'started', and right before every historical query.
- Major upgrade to `handleQueryHistorical`:
  - Collects candidates from goodStorePeers (remembered successes) + store peers + all connected peers.
  - Shuffles and tries up to 6 different peers in sequence for a single query.
  - Once a peer delivers messages, it is added to `goodStorePeers` and preferred in future.
- Periodic (every 45s) background call to `ensureStrongConnectivity()`.
- Rust test example now:
  - Waits up to 180s for 'started'.
  - Waits up to 90s or until mesh >= 6 before first historical query.
  - Uses 6h window for snapshot testing (higher chance of catching fee data).

These changes make the "force peerId" strategy much more robust and give discovery more chances to surface usable Store peers.

Next step after testing this version: decide whether to continue iterating on discovery or lock this as "good enough foundation" and move real fee logic into Rust.

### 2026-06-27 Quick Fix - enrTree misuse
The latest run crashed with "enrTree is not a function".
Root cause: `enrTree` exported from @waku/discovery is an *object* containing predefined tree URLs (for Waku's own SANDBOX/TEST), not a wrapper function.
We were calling `enrTree(url)` which doesn't exist.

Fixed by reverting only the discovery creation line back to the previous working form:
wakuDnsDiscovery(enrTrees)   // raw enrtree:// strings

All the other peer discovery improvements remain active:
- ensureStrongConnectivity() with repeated dials of known relays
- goodStorePeers memory
- Trying up to 6 different candidate peers during historical queries
- Patient waits in the Rust test example

This should restore connectivity while keeping the new robustness.
