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
3. Full integration of RailgunKeys with Note creation + scanning support.
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