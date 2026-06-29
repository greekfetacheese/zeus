# Zeus + Railgun Integration Roadmap

**Status (2026-06-29)**:
- Note model + blinded keys + nullifiers complete.
- **Contract ABIs verified & corrected** against actual RailgunLogic.sol + Globals.sol (pulled from Etherscan) + deployments/implementation.ts.
  Correct events:
  - Shield(treeNumber, startPosition, CommitmentPreimage[], ShieldCiphertext[], fees[])
  - Transact(treeNumber, startPosition, hash[] (leaves), CommitmentCiphertext[])
  - Unshield + Nullified(uint16, bytes32[])
  Removed legacy GeneratedCommitmentBatch / CommitmentBatch (not present in current contracts).
- PoseidonMerkleTree implemented.
- RailgunScanner updated to correctly decode real events, compute leaves for Shield (using our poseidon), insert direct hashes from Transact, track nullifiers.
- References added: /home/cion/Railgun/Railgun contracts and /home/cion/Railgun/deployments
- **Disk persistence for PoseidonMerkleTree implemented using redb** (stable pure-Rust DB).
  - `PoseidonMerkleTree::open(path, tree_id)`, `load(db, tree_id)`, `save(db, tree_id)`
  - `from_leaves` / `leaves()` helpers for roundtrips
  - `RailgunScanner` now has `load_merkle_tree(&db, tree_id)` and `save_merkle_tree(&db, tree_id)`
  - All tests passing (including persistence roundtrips)
- **Unified redb persistence helper added**:
  - Single `redb::Database` for both Merkle tree + scanner state (nullifiers + owned notes + last block)
  - New methods: `RailgunScanner::load_state(&db, tree_id)`, `save_state(&db, tree_id)`
  - New convenience: `RailgunScanner::open(db_path, keys, chain_id, tree_id)`
  - `load_merkle_tree` / `save_merkle_tree` also available
- **RailgunScanner is now fully thread-safe**:
  - Uses `Arc<Mutex<RailgunScannerInner>>` (cheap Clone, Send + Sync)
  - Matches the pattern used by `WakuSidecarClient`
  - All public methods go through the lock
  - Added safe accessors: `last_synced_block()`, `owned_notes_len()`, `merkle_tree()`, `chain_id()`, etc.
  - `unspent_notes()` now returns `Vec<OwnedNote>` (cloned) to avoid holding locks
- All tests passing (17/17)
**Important note on 2026-06-29 refactor**:
- A background `cargo check` (old proc) showed many "unknown field `merkle_tree` / `last_synced_block` etc." errors.
- These were expected mid-refactor while changing `RailgunScanner` to use `Arc<Mutex<Inner>>`.
- All such direct field accesses have been removed. Current `cargo check -p zeus-railgun` is clean and all 17 tests pass.
**Goal**: Full native Rust Railgun privacy (shield, private transfers/swaps, unshield) inside Zeus (egui + alloy). Use Waku broadcasters for gas abstraction.  
**Key Decision**: Option A — complete Waku client first (done). Core privacy logic lives in `zeus-railgun`.

---

## Current State

- **zeus-waku-broadcaster** (client crate): Fully functional.
  - Node.js sidecar (pure `@waku/sdk`) handles networking (discovery, subscribe, publish, Store historical).
  - Rust owns everything else: fee parsing, `BroadcasterFeeCache`, selection (`get_best_fee_quote`), `BroadcasterTransaction` (real BabyJub ECDH + AES), response handling, `decode_address`.
  - Thread-safe (`Clone` cheap via `Arc<Mutex>` + kanal), `get_peers()`, `wait_for_peers()`, `clear_expired_fees()`.
  - Confirmed working on mainnet (mesh 2-3 fast, historical fees delivered, selection works).
  - User: "The client seems to generally work it only needs 2 things: receive live fee messages and verify that the trasnact works."

- **zeus-railgun**: Foundation + Note model started.
  - Full `RailgunKeys` (spend priv/pub, view priv/pub, nullifier, master pubkey).
  - `0zk` address + decode working.
  - **Note model** (new):
    - `Note`, `TokenData`, `TokenType`
    - `compute_note_public_key`, `compute_commitment`, `compute_token_hash`
    - `derive_shared_symmetric_key` (viewing priv + blinded pub → AES key)
    - `encrypt_note_v2` / `decrypt_note_v2` (AES-GCM)
  - All tests passing.

**Architecture (locked)**:
- `zeus-waku-broadcaster`: Dumb-pipe sidecar + Rust Railgun broadcaster logic (fees + transact).
- `zeus-railgun`: Core privacy engine (keys, notes, trees, proofs, contract builders).
- Zeus wallet integrates both later.

---

## Completed

- Waku client full cycle (historical + live, fees, selection, transact skeleton).
- Peer connectivity, thread-safety, observability (`get_peers`).
- Address + BabyJub crypto primitives (partial but usable).
- Sidecar architecture chosen and stabilized (no native waku bindings).

---

## Next: zeus-railgun Engine (Current Focus)

Start building the actual privacy engine.

**Immediate priorities**:
1. ✅ Note / commitment model + viewing-key encryption/decryption (AES-GCM) — **done**.
2. ✅ Proper blinded viewing keys (sender + receiver) + annotation data (AES-CTR) + nullifier calculation — **done**.
3. ✅ Full integration of RailgunKeys with Note creation + scanning support.
3. Basic on-chain interaction: Railgun contract addresses/ABIs, shield/unshield calls (via alloy).
4. Local state: Poseidon Merkle tree, commitment insertion, nullifier tracking.
5. Scanner: listen to events, decrypt notes with viewing key, maintain private balance.

Later phases (condensed):
- ZK proofs + private transact builder (biggest effort).
- Full broadcaster integration (real private txs via client).
- Wallet + GUI integration (Privacy Mode toggle, shield/unshield forms).
- Multi-chain, POI, swaps, testing.

See original long phases below only if needed for historical detail.

---

## Key Files

- `crates/zeus-railgun/src/` — address.rs (current), keys.rs, notes.rs, engine/, contracts.rs (future)
- `crates/zeus-waku-broadcaster/src/client.rs` — integrated client (reference for encryption patterns)
- `crates/zeus-railgun/src/lib.rs` — re-exports
- `RAILGUN_ROADMAP.md` (this file)

---

## References

- Official: https://railgun.org/ + https://docs.railgun.org/developer-guide
- Cloned repos (for reverse engineering):
  - `/home/cion/Railgun/waku-broadcaster-client`
  - `/home/cion/Railgun/wallet`
  - `/home/cion/Railgun/engine`
  - `/home/cion/Railgun/Railway-Wallet`
- Zeus crates: zeus-railgun (engine), zeus-waku-broadcaster (client), zeus-eth (alloy)

---

## Pitfalls to Avoid

- Do not duplicate logic between waku client and railgun crate.
- Keep sidecar dumb — all domain logic in Rust.
- Use real vectors from TS repos for tests (no change-detector tests).
- Address encoding must stay compatible with official 0zk format.
- Secure key handling (secure-types already in use).

---

**Update this file after every milestone.** Use `todo` tool + memory for session tracking.

Client phase complete. Engine phase now active. Let's go.
## Latest Progress (2026-06-29, this session)

**1. Unified redb helper for everything**
- Added `RailgunScanner::open(db_path, keys, chain_id, tree_id)` — opens one redb file and loads both merkle tree + scanner state.
- New methods:
  - `load_state(&db, tree_id)` / `save_state(&db, tree_id)`
  - `load_merkle_tree` / `save_merkle_tree` (were missing, now implemented)
- Scanner state (spent nullifiers, owned notes, last_synced_block) is now persisted in the same redb file using dedicated tables.

**2. RailgunScanner is now thread-safe**
- Refactored to `Arc<Mutex<RailgunScannerInner>>`
- Cheap to Clone
- All mutation protected
- Public accessors added: `last_synced_block()`, `owned_notes_len()`, `merkle_tree()`, `chain_id()`, etc.
- `unspent_notes()` returns `Vec<OwnedNote>` (cloned copies)
- Matches the exact pattern the user liked in the waku broadcaster client.

All tests (17) passing.

**Next focus**: Shield / Unshield transaction builders.
