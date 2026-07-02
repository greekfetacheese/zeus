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


## Realistic minGasPrice (2026-07-02)

Implemented proper gas price estimation for BoundParams and broadcaster `overall_batch_min_gas_price`:

- Added `RailgunEngine::estimate_min_gas_price(&self, client: &RpcClient) -> Result<U256>`
- Ethereum mainnet: `get_next_base_fee()` (from zeus-eth) + `get_max_priority_fee_per_gas()` + ~12% buffer
- Other chains: `get_gas_price()` + `get_max_priority_fee_per_gas()` + buffer
- Updated high-level APIs:
  - `shield(client, ...)`
  - `unshield(client, ...)`
  - `unshield_via_broadcaster(client, ...)`
- All use the realistic value instead of hardcoded 1 or quote-fee mis-use.
- `suggested_min_gas_price` now returns a safe default (callers in GUI should use the estimate method with live client).
- 20/20 tests still pass.


## Kohaku Railgun Review & Protocol Alignment (Started 2026-07-02)

**Goal before GUI integration**: Validate our `RailgunScanner`, Merkle tree, note model, commitment handling, nullifier tracking, and private balance logic against a more complete independent Rust implementation of the Railgun protocol.

Kohaku (Ethereum Foundation / kohaku roadmap privacy tooling) takes a slightly different approach for broadcasting (third-party paymaster instead of Waku broadcaster), but the on-chain event processing, UTXO model, Merkle trees, note decryption, and nullification logic should be authoritative.

**Key Kohaku modules for Merkle / Notes / Commitments / Syncing (reference these):**

- `indexer/utxo_indexer.rs` — Central state: `utxo_trees: BTreeMap<u32, UtxoMerkleTree>`, list of `IndexedAccount`, sync logic, event dispatch.
- `indexer/indexed_account.rs` — Per-account state (`IndexedAccountState { notes: Vec<UtxoNote>, synced_block }`). Handles:
  - `handle_shield_event`
  - `handle_transact_event`
  - `handle_nullified_event` (removes matching note by nullifier from the notes vec — no separate spent set)
- `indexer/syncer/` (chained.rs, rpc.rs, subsquid.rs, mod.rs) — Event sources and normalization.
- `merkle_tree/`
  - `utxo_tree.rs` + `merkle_tree.rs` (TREE_DEPTH = 16, Poseidon hash, `UtxoMerkleTree`)
  - `verifier.rs`
- `note/utxo.rs` — `UtxoNote` struct (tree_number, leaf_index, value, asset, nullifier, blinded_commitment, note_public_key, etc.). Decryption + nullifier computation.
- `note/mod.rs` — `Note` + `EncryptableNote` traits.
- `provider.rs` — High-level `RailgunProvider`:
  - `balance(address) -> Vec<BalanceEntry>` (grouped by `AssetId` / token)
  - `notes(address) -> Vec<NoteEntry>`
  - `unspent(...)` (filters via the indexer)
- `database/` — persistence of trees + account state.

**Current differences observed (to investigate):**

- Our scanner keeps **all** `owned_notes` + separate `spent_nullifiers: HashSet`. Kohaku **removes** notes from the list on nullification events.
- Our `private_balance()` and `unspent_note_count()` are currently global (sum all tokens). Kohaku returns per-`AssetId` (TokenType).
- How commitments are reconstructed and how `blinded_commitment` vs on-chain commitment is used.
- Tree numbering / multiple trees per chain.
- Nullifier derivation details.

**References added:**
- Kohaku repo: `/home/cion/Railgun/kohaku`
- Focus crate: `/home/cion/Railgun/kohaku/crates/railgun`
- Specific files listed above.

**Next step after review**: Decide on scanner redesign (if needed) so that `private_balance` and note selection for unshield are per-token and match the protocol exactly. Then proceed to GUI.

Update this section after the review is complete.


## Kohaku Crypto References (2026-07-02)

For Poseidon, note commitments, nullifiers, blinded keys and Merkle:
- `/home/cion/Railgun/kohaku/crates/crypto` — High level wrappers:
  - `src/poseidon.rs` (wraps poseidon-rust)
  - `src/merkle_tree/` (UtxoMerkleTree using Poseidon)
  - `src/babyjubjub.rs`, pedersen, etc.
- `/home/cion/Railgun/kohaku/crates/poseidon-rust` — The actual BN254 Circom-compatible Poseidon implementation (t2..t14 params, full optimized permutation).
  - Uses explicit Circom-generated round constants and MDS matrices.
  - `poseidon_hash(&[Fr]) -> Fr`

Our current implementation uses `light-poseidon` + `new_circom(arity)`. The input ordering for:
- nullifier = poseidon([nullifying_key, leaf_index])
- npk = poseidon([master_public_key, random])
- commitment = poseidon([npk, token_hash, value])
appears to match Kohaku's `note/utxo.rs`.

We will verify byte-for-byte equivalence for critical cases. If mismatch is found, we will port/fork the poseidon-rust + crypto logic into `zeus-railgun-shared`.

Added to references for all future crypto validation.


## Scanner Redesign for Spent Notes + Per-Token Balances (2026-07-02)

Adopted Kohaku patterns:

- `owned_notes: Vec<OwnedNote>` now represents **only unspent notes** (no separate filtering at query time).
- New methods on `RailgunScanner` (and exposed on `RailgunEngine`):
  - `private_balances() -> HashMap<TokenData, U256>`
  - `private_balance_for(&TokenData) -> U256`
  - `unspent_notes() -> Vec<OwnedNote>`
  - `unspent_notes_for(&TokenData) -> Vec<OwnedNote>`
  - `mark_nullified(nullifier)` + `mark_nullified_many` (removes from list + legacy spent set)
- `add_own_shielded_note` now avoids duplicates.
- Old `private_balance()` / `unspent_note_count()` kept for compatibility but now total across tokens.
- TokenData and TokenType now derive Hash (so they can be map keys).

This matches Kohaku's `IndexedAccount.unspent()` (just the notes vec) and `provider.balance()` (per AssetId).

Next for full alignment:
- Wire Nullified event decoding in `sync_from_block` to call mark_nullified.
- When building unshield, call mark after note selection (optimistic).
- Consider dropping the spent_nullifiers HashSet entirely after persistence migration.

Poseidon/crypto alignment review ongoing (inputs match; will verify with side-by-side if needed).

Updated before proceeding to GUI integration.


## Kohaku Alignment Polish: Nullified handling + Sync strategy (2026-07-02)

Implemented:

1. **Nullified event wired in sync_from_block**
   - Now calls `scanner.mark_nullified(nullifier)` (removes note from `owned_notes`).
   - Previously only inserted into the legacy spent set.
   - Matches Kohaku `IndexedAccount::handle_nullified_event`.

2. **Post-tx sync is the recommended balance update path (no aggressive optimistic marking)**
   - High-level `shield()`, `unshield()`, `unshield_via_broadcaster()` do **not** auto-mark notes.
   - Improved `engine.sync(&mut self, client)` with 128-block reorg buffer.
   - `apply_unshield()` / `apply_shield()` still exist as *optional immediate* helpers.
   - Docs strongly recommend running `sync()` after confirmation so on-chain Nullified events are the source of truth.
   - Prevents showing incorrect (too-low) balance if a tx fails/reverts.

3 & 4 (optional, evaluated)
   - Detailed data model comment added in `RailgunScannerInner`.
   - `spent_nullifiers` kept for now (persistence compat). TODO to drop after migration.
   - No per-TokenData internal grouping yet (linear scan is fine; realistic wallets have few notes).
   - Will revisit only if we see perf issues with large note counts.

Updated scanner docs, engine high-level docs, apply comments, and sync method.
