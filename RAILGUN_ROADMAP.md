# Zeus + Railgun Integration Roadmap

**Current Status (2026-07-01)**

Core Railgun privacy engine is functionally complete:
- `RailgunEngine` owns scanner, keys, prover client, and waku broadcaster client.
- Full high-level APIs for shield / unshield (with real ZK proofs).
- Sidecars are embedded at build time and automatically extracted + `npm install`ed at runtime.
- All 20 tests pass.

## Architecture (Locked)

- **zeus-railgun-shared** — Common types only (RailgunKeys, 0zk addresses, BabyJub, poseidon, Chain).
- **zeus-railgun** — Full privacy engine:
  - Note encryption, scanner + Poseidon Merkle (redb, thread-safe).
  - Shield / Unshield builders + `RailgunEngine`.
  - Real proof generation + transact calldata.
  - Owns both sidecar clients.
- **zeus-waku-broadcaster** — Gas abstraction (depends on shared only).
- Sidecars (JS) are dumb pipes. Rust owns all logic.

**Key Principle**: Rust owns the protocol. Sidecars are thin execution layers.

## Major Milestones Completed

- 0zk address generation + key derivation
- Note model + blinded keys + AES-GCM encryption
- Thread-safe `RailgunScanner` + unified redb + Poseidon Merkle tree
- Shield + Unshield builders (change notes, multi-note selection, blinded derivation)
- `RailgunEngine` high-level wrapper that owns clients
- Real BabyJub public key + correct `boundParamsHash` in witnesses
- `build_*_proof_request` + `snark_proof_from_sidecar` for both shield and unshield
- Real Groth16 proofs via sidecar (no dummies in production paths)
- Sidecar embedding system (build.rs + smart extraction + version hash + auto `npm install --production`)
- Node.js detection with clear error messages
- Fee quote integration (`get_best_fee_quote`, auto selection)
- High-level public APIs:
  - `shield(&self, token, value, memo) -> Vec<u8>` (calldata, self-broadcast)
  - `unshield(&self, to, token, amount) -> Vec<u8>` (calldata)
  - `unshield_via_broadcaster(&mut self, to, token, amount) -> WakuTransactResponse`
- 20 passing tests

## Current Focus (Prioritized)

1. **Complete the broadcaster transact flow**
   - Properly wire + test `unshield_via_broadcaster` end-to-end (publish encrypted message, wait for response, return tx hash).
   - Handle real Railgun contract addresses per chain.
   - Derive sensible `minGasPrice` from fee quotes.

2. **GUI / Zeus integration**
   - Expose privacy mode in egui.
   - Connect to `zeus-eth` wallet signing/broadcasting.
   - Show private balances, shield/unshield UI flows.

3. **Production & polish**
   - Live scanner watching (Waku events or polling).
   - Better error handling / fallbacks (no quotes → self-broadcast).
   - Per-chain Railgun contract addresses + tree IDs.
   - End-to-end tests (shield → unshield → balance).

## What's Next (Later)

- Private transfers / swaps inside Railgun
- Cross-contract calls (relay adapt)
- Full privacy mode in Zeus (default shielded UX)
- Native prover (optional, for performance)

## Pitfalls to Avoid

- Do not duplicate key/address logic (always use shared crate).
- Keep sidecars dumb — never move protocol decisions into JS.
- Engine must stay the single high-level entry point.
- Always test both self-broadcast and broadcaster paths.

## References

- Official: https://railgun.org/ + https://docs.railgun.org/developer-guide
- Cloned: `/home/cion/Railgun/{waku-broadcaster-client, wallet, engine}`
- Key code:
  - `crates/zeus-railgun/src/engine.rs`
  - `crates/zeus-railgun/src/builders.rs`
  - `crates/zeus-railgun/src/sidecar_assets.rs`
  - `crates/zeus-waku-broadcaster/src/`

Update this file after every major integration milestone.


## Broadcaster Transact Path Audit & Fixes (2026-07-02)

Audit of `unshield_via_broadcaster` and supporting code:

**What was missing / broken:**

- Railgun contract address was hardcoded to `0x0000...` when calling `waku_client.transact(...)`. The "to" must be the RailgunSmartWallet proxy.
- `min_gas_price` was hardcoded to 1 everywhere (both for BoundParams and for the broadcaster's overall_batch_min_gas_price).
- `viewing_public_key` was not populated in `find_best_broadcaster` (only in the "all quotes" path). This broke ECDH encryption for the common "best quote" flow.
- Scanner creation required a known Railgun contract address → tests on Polygon (137) etc. were broken.
- No helper on `RailgunEngine` to get the contract address.
- No derivation of gas price from the actual broadcaster quote.
- Limited chain support in `railgun_address()`.

**Fixes applied:**

- Added `RailgunEngine::railgun_contract_address()` (delegates to `contracts::railgun_address`).
- Fixed `find_best_broadcaster` to derive + attach `viewing_public_key` (using `RailgunAddress` from shared).
- `unshield_via_broadcaster` now:
  - Uses real `railgun_contract_address()` (errors clearly on unsupported chains).
  - Derives `min_gas_price` from the quote's `fee_per_unit_gas` (with safe fallback).
  - Passes correct `overall_batch_min_gas_price` to the broadcaster.
  - Improved error messages.
- Added Polygon (137) support (placeholder address) + comments.
- Made scanner creation tolerant of unknown chains (uses ZERO) so dev/tests on any chain work. Broadcaster path still enforces real address when needed.
- Updated hardcoded 1 in plain `unshield()` to a more reasonable default.
- All 20 tests still pass.

The core "quote → prepare → real proof → calldata → encrypted Waku transact" path is now wired correctly.

Remaining for "complete" broadcaster experience (next after this):
- Better `minGasPrice` derivation (e.g. from live gas price + broadcaster fee).
- Automatic `apply_unshield` after successful response + on-chain confirmation.
- Real viewing key population from live fee messages (currently derived from the railgun address in the fee).
- Support for more chains + verified contract addresses.
