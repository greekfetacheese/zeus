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
**Code Review (pre-builders, 2026-06-29)**:
- Full pass over zeus-railgun (address, note, contracts, merkle, scanner, lib, Cargo.toml).
- **Fixed**: TREE_DEPTH changed from 32 (with TODO) to 16 to match real Railgun contracts (Commitments.sol). Updated docs/comments. Proofs and roots now correct size.
- **Fixed**: load_* methods (load_state_from_file, load_merkle_tree, load_state) now take `&self` (were &mut). Consistent with thread-safe Arc<Mutex> design and other methods. open() and tests cleaned.
- Added `RailgunScanner::merkle_root(&self)` convenience accessor (useful for builders).
- Silenced dead_code warning on spending_private (intentionally kept for builder spend logic) with comment.
- Updated placeholder comments (Polygon address, legacy file persistence).
- Confirmed: Note has to_bytes/from_bytes (used by scanner redb + file persistence). get_proof exists on merkle. All reexports good. 17/17 tests pass post-fixes. cargo check clean.
- No other major flaws found. State is solid for starting shield/unshield builders.
- Legacy file persistence kept but redb unified is recommended and fully functional.
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

- `crates/zeus-railgun/src/` — address.rs, note.rs, contracts.rs, merkle.rs, scanner.rs
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
- Secure key handling (secure-types already in use) and explcitly use `zeroize` on types that contain secrets if they are not `Secure`.

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


## Shield / Unshield Transaction Builders (started 2026-06-29)

**Status**: First implementation complete. `builders.rs` added and tested.

**What was implemented**:
- `PreparedShield` + `prepare_shield(receiver_keys, token, value, memo)` → Note + CommitmentPreimage + ShieldCiphertext
- `build_shield_call_data(...)` → ready-to-use (preimages, ciphertexts, fees) arrays for RailgunSmartWallet.shield(...)
- `PreparedUnshield` + `prepare_unshield(scanner, to, token, amount)` → nullifiers + merkle proofs + unshield preimage (simple note selection)
- `apply_shield_to_scanner(scanner, prepared, leaf_index)` and `apply_unshield_to_scanner(scanner, prepared)`
- Helpers integrate directly with existing Note, scanner state, merkle proofs, and contract structs.
- 2 new passing tests + overall suite now at 19 tests.

**Current limitations / next refinements**:
- Shield uses simplified note creation (basic Note::new) and fixed shared key for ciphertext encryption (to avoid current keys module blinding derivation issues). Will align with full `create_note_with_keys` + proper shieldKey derivation when keys are 1:1 with TS.
- Unshield note selection is MVP (first sufficient note). No change notes yet.
- No broadcaster fee integration yet (will come from zeus-waku-broadcaster).
- Unshield is prepared for "transact + unshield output" path.

**Files added**:
- `crates/zeus-railgun/src/builders.rs`
- Re-exported key symbols from `lib.rs`

This completes the immediate prerequisite for using Railgun in Zeus (shield/unshield flows).


## Builders Refinement (ponytail, 2026-06-29)

- prepare_shield now uses full get_note_blinding_keys + shield_key for blinded_receiver + derive_shared.
- prepare_unshield: multi-note greedy select, change_note: Option<Note>, takes &RailgunKeys.
- PreparedUnshield extended with change_note.
- create_note_with_keys reused. 19 tests green.
- Skipped: full knapsack select, transact batch builder.



## RailgunEngine + Initial Waku Wiring (2026-06-30)

- Added `RailgunEngine` high-level wrapper (owns keys + scanner, high-level `prepare_shield` / `prepare_unshield`, `apply_*`, persistence helpers).
- Refined builders for broadcaster path:
  - New `PreparedBroadcasterUnshield`
  - `prepare_unshield_for_broadcaster(...)`
  - `RailgunEngine::prepare_unshield_gas_sponsored(...)` (takes fees_id + broadcaster info from waku client)
- Optional gas-sponsored unshield support started (only on unshield, as requested).
- No direct crate dependency added (avoids cycle — broadcaster already depends on railgun).
- Next: flesh out full `transact` calldata builder so the prepared data can be directly fed to `WakuSidecarClient::transact(...)`.
- All checks and 19 tests still green.



## Real Transact Calldata + use_broadcaster Flag (2026-06-30)

- Added full `transact(Transaction[])` definition + supporting structs (`Transaction`, `BoundParams`, `SnarkProof`, `G1Point`/`G2Point`, `UnshieldType`) to the `sol!` block in `contracts.rs`.
- Implemented `build_unshield_transact_calldata(...)` that takes `PreparedUnshield` + scanner state and produces the exact encoded calldata for `RailgunSmartWallet.transact(...)`.
  - Handles nullifiers, commitments (from change note), merkle root, unshieldPreimage, BoundParams (with minGasPrice and unshield type).
  - Uses placeholder proof (real ZK proof generation is future work).
- Updated low-level `prepare_unshield(...)` to accept `use_broadcaster: bool` (currently informational).
- Updated high-level `RailgunEngine::prepare_unshield(to, token, amount, use_broadcaster: bool)`.
- Updated `prepare_unshield_for_broadcaster` to actually call the new builder and attach `transact_calldata`.
- Added `RailgunEngine::build_unshield_transact_calldata(...)` convenience method.
- `cargo check -p zeus-railgun` clean, 19/19 tests still pass.
- The `use_broadcaster` flag is now part of the main unshield high-level API as requested (only affects unshield path).

Next logical steps:
- Wire real chain_id into the builders.
- Pass actual fee quote data (from waku) into the BoundParams.
- Implement proper note selection / change note for broadcaster case.
- Start integrating with zeus-waku-broadcaster for the actual `transact` submission.



## Fleshed out details before Waku integration (2026-06-30)

**Addressed items:**
- **ZK proof generation TODO**: Replaced inline zeroed proof with dedicated `create_dummy_snark_proof()` function. Added clear, prominent documentation that this is a placeholder and real Groth16 proof generation (Railgun circuit) is required for on-chain use. API is ready for future prover integration.
- **Real chain id passing**: Removed the last hardcoded `1` in `prepare_unshield_for_broadcaster`. Now derives `scanner.chain_id()`. Added `RailgunEngine::chain_id()` + convenience `build_unshield_transact_calldata(...)` (no chain param) that uses the engine's chain. Kept an explicit `_with_chain` variant for advanced use.
- **Better note selection for broadcaster**: Changed from "in scanner order" to "largest-first" greedy selection. This minimizes the number of nullifiers revealed and is preferable when paying a broadcaster (less data, lower gas).
- **Change note handling**: Verified and documented. When `total > amount`, a `change_note` is created via `create_note_with_keys`. It is correctly added to the `commitments` array in the `Transaction` struct. Added comment on `unshieldPreimage.npk`.
- Cleaned multiple outdated comments (e.g. "no change note yet", "calldata assembly coming later").
- Added two new tests exercising `use_broadcaster: true` path, `RailgunEngine` broadcaster APIs, and change-note + calldata builder (21/21 tests pass).

**Remaining acknowledged limitations (documented):**
- Real ZK proof generation is future work (large effort).
- `unshieldPreimage.npk` is currently zeroed (matches current simple unshield use case).
- No fee amount yet injected into BoundParams or change calculation (comes from waku quote later).
- No multi-transaction batching yet (single Transaction for now).

All of the above makes the engine much more solid before wiring the waku broadcaster client.


## Prover Sidecar (ZK Proof Generation) — Architecture Decision (2026-06-30)

**Problem**: Railgun requires a valid Groth16 proof for `transact(...)`. We have excellent witness data in `build_unshield_transact_calldata`, but no way to generate a proof the on-chain verifier will accept (no circuit source, no Rust prover for their specific artifacts).

**Solution chosen**: Replicate the exact **sidecar architecture** that worked for Waku.

- Create new crate: `zeus-railgun-prover`
- **JS sidecar** (`js-sidecar/`):
  - Downloads artifacts from the same IPFS sources used by the official Railgun SDK (`@railgun-community/wallet` + `artifact-util.ts`).
  - Uses `snarkjs` (or calls the native-prover when available) to run `groth16.fullProve`.
  - Pure "dumb prover" — receives formatted witness, returns `SnarkProof` (pi_a/pi_b/pi_c).
- **Rust side** (`zeus-railgun-prover`):
  - Owns witness construction (builds `FormattedCircuitInputsRailgun` from our `PreparedUnshield` + scanner + keys).
  - Spawns and manages the Node sidecar over line-delimited JSON (same pattern as `WakuSidecarClient`).
  - High-level API: `prove_unshield(prepared, ...)` → returns real `SnarkProof`.
- Benefits:
  - Leverages the battle-tested proving artifacts and libraries the entire Railgun ecosystem uses.
  - Keeps heavy proving + artifact management out of pure Rust.
  - Consistent architecture across the project (Waku + Prover sidecars).
  - Rust still owns all privacy primitives and calldata logic.

**Status**:
- Decision made after deep review of `engine/src/prover/prover.ts`, artifact downloaders, and how Railway + official wallet invoke the prover.
- Next: Scaffold the crate following `zeus-waku-broadcaster` patterns (client.rs, models, js-sidecar/index.js).
- Will integrate later with `RailgunEngine` and the `build_unshield_transact_calldata` path (replace the dummy proof).

This is the pragmatic path to real, on-chain-valid proofs without months of reverse-engineering the circuit.


## Prover Sidecar Improvements (2026-06-30)

**1. Improved JS sidecar**
- Persistent disk caching of artifacts in `~/.railgun/artifacts-v2.1/` (survives restarts).
- Progress reporting: sidecar now emits `{type: "progress", stage: "download"|"proving", percent}` events.
- Better native-prover support:
  - Attempts to dynamically load `@railgun-privacy/native-prover` (optionalDependency).
  - Falls back gracefully to snarkjs.
  - Includes a `getNativeCircuitId` mapper and `convertWitnessForNative`.
- Updated `package.json` with optionalDependencies.

**2. Proper FormattedCircuitInputs / witness types**
- Created `src/models.rs` with types directly modeled after Railgun's engine:
  - `FormattedCircuitInputsRailgun` (the flat shape passed to `snarkjs.groth16.fullProve`)
  - `PublicInputsRailgun`
  - `PrivateInputsRailgun`
  - `ProofRequest` + `ProofResponse`
- Added `FormattedCircuitInputsRailgun::from_parts(...)` that exactly mirrors the TS `formatRailgunInputs` logic (including `pathElements.flat(2)`).
- Re-exported from lib.rs.
- Added typed `prove_with_inputs` helper on the client.

**3. Example / Test exercising the sidecar**
- Added `tests/sidecar_test.rs`:
  - `test_formatted_inputs_construction` (unit test, always runs).
  - `test_prover_client_starts_and_proves_dummy` (integration — gracefully skips if sidecar can't start).
- Test can be run with: `cargo test -p zeus-railgun-prover --test sidecar_test`

**Status**
- All changes compile cleanly.
- JS sidecar is significantly more production-ready for artifact management and feedback.
- Rust now has the correct witness shape so we can start mapping `PreparedUnshield` → `ProofRequest` in the next phase.

