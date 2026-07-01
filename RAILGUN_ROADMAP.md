# Zeus + Railgun Integration Roadmap

**Current Status (2026-07-01)**

We have extracted common Railgun protocol primitives into a new `zeus-railgun-shared` crate. This solves duplication and will simplify future integration of the Waku broadcaster and ZK prover into the core engine.

## Architecture (Locked)

- **`zeus-railgun-shared`** — Foundational types only:
  - `RailgunKeys`, `RailgunAddress` + `0zk` encoding
  - BabyJubJub primitives + `babyjub_shared_secret`
  - `poseidon_hash`, key derivation
  - `Chain`

- **`zeus-railgun`** — The core privacy engine:
  - Notes, commitments, encryption (AES-GCM + annotation)
  - Scanner + Poseidon Merkle tree (unified redb, thread-safe)
  - Shield / Unshield builders + `RailgunEngine`
  - Transact calldata construction + `build_unshield_proof_request`
  - Will own integration with prover + broadcaster clients

- **`zeus-waku-broadcaster`** — Gas abstraction client:
  - Dumb Node.js Waku sidecar
  - Rust owns fee cache, selection, encryption, transact messages
  - Now depends on `zeus-railgun-shared` (instead of zeus-railgun) to avoid cycles

- **Prover sidecar** (currently `zeus-railgun-prover` or integrated) — Same dumb-pipe pattern for snarkjs Groth16 proofs.

**Key Principle**: Rust owns all domain logic. Sidecars are thin pipes only.

## Major Milestones Completed

- Railgun address generation (`0zk...`) + key derivation (very close to Railway wallet)
- Full note model with proper blinding + AES-GCM encryption/decryption
- Poseidon Merkle tree + redb persistence (unified with scanner state)
- Thread-safe `RailgunScanner` (`Arc<Mutex<Inner>>`)
- Shield / Unshield builders (`PreparedShield`, `PreparedUnshield`, change notes, multi-note selection)
- `RailgunEngine` high-level wrapper
- Full `transact(...)` calldata builder (with `use_broadcaster` flag)
- Real BabyJub public key + exact `bound_params_hash` (keccak(abi.encode) % field) in witness
- `build_unshield_proof_request` + `snark_proof_from_sidecar` helper
- Full flow test (prepare → proof request → calldata)
- Extracted `zeus-railgun-shared` for address/keys/crypto (this refactor)

- Waku broadcaster client (historical + live fees, selection, encryption, transact skeleton) — complete and tested on mainnet
- Prover sidecar scaffolded with proper witness types + persistent artifact caching + progress events

All core `zeus-railgun` tests pass (22+).

## Current Focus (Next Phase)

**Integrate the prover sidecar and Waku broadcaster client into `RailgunEngine`**.

Specific goals:
- Allow `RailgunEngine` to optionally own or use `RailgunProverClient`
- Wire `prepare_unshield_gas_sponsored` / `build_unshield_transact_calldata` to request real fee quotes from the Waku client
- Replace dummy proofs with real proofs from the sidecar in the unshield path
- Support the full "gas-sponsored private unshield" flow end-to-end
- Keep the architecture clean (no cycles, clear ownership)

After that:
- GUI integration (Privacy Mode in Zeus)
- Full private transact / swaps
- Scanner live watching + balance tracking in privacy mode

## Pitfalls to Avoid

- Do not put Waku or proving logic inside the core engine crate (keep them as optional clients or submodules).
- Never duplicate key derivation / address encoding logic.
- Sidecars must remain "dumb" — all Railgun protocol decisions stay in Rust.
- Use the new `zeus-railgun-shared` for anything that both the engine and broadcaster need.

## References

- Official: https://railgun.org/ + https://docs.railgun.org/developer-guide
- Cloned reference repos: `/home/cion/Railgun/{waku-broadcaster-client, wallet, engine, poseidon-hash-wasm}`
- Key files:
  - `crates/zeus-railgun-shared/src/` (address, keys, crypto)
  - `crates/zeus-railgun/src/{builders.rs, engine, scanner, note}`
  - `crates/zeus-waku-broadcaster/src/`

Update this file after every major integration milestone.

**Next discussion**: How to cleanly wire `WakuSidecarClient` + `RailgunProverClient` into `RailgunEngine`.
