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


## Sidecar Embedding (Build Script) - Started (2026-07-01)

- Created `crates/zeus-railgun/build.rs` — the "compiler script".
  - Embeds only the essential sidecar sources (`package.json` + `src/index.js`) from:
    - `zeus-railgun-prover/js-sidecar`
    - `zeus-waku-broadcaster/js-sidecar`
  - Does **not** embed `node_modules` (they will be installed on first extraction if needed).

- Added `crates/zeus-railgun/src/sidecar_assets.rs`:
  - Uses `include_bytes!` from `OUT_DIR` (populated by build.rs).
  - `extract_sidecars(base_dir)` — writes the files out.
  - `extract_sidecars_to_zeus_data()` — writes to `data/railgun_sidecars/{prover,waku}`.

- Updated `RailgunEngine`:
  - `start_clients()` now automatically calls extraction first.
  - New public helper: `extract_sidecars()`.
  - Removed the old hardcoded development paths (the TODO is addressed).

- `RailgunEngine` owns the clients and now has a clean path toward production single-binary distribution.

Next steps for this area:
- Optionally run `npm install --production` automatically on first extraction (or on version bump).
- Make sidecar paths configurable.
- Package-time improvements (e.g. pre-installing node_modules or using a bundled node).

This enables the fee quote + proof flows we will wire next.


## Sidecar Embedding — npm install + Version Hash (2026-07-01)

Completed the production sidecar solution:

- `sidecar_assets.rs` now implements:
  - `current_sidecars_hash()` — stable SHA256 of the embedded sources.
  - `.railgun-sidecar-version` marker file per sidecar.
  - `ensure_sidecars_extracted()` — only extracts when hash changed or first run.
  - `ensure_npm_dependencies(dir)` — runs `npm install --production` **only** if `node_modules` is missing.
  - `ensure_sidecars_ready()` — the recommended one-stop function (extract + npm).
  - `is_node_available()` — quick pre-check.

- Clear, user-friendly error when `npm` is not found:
  > "Node.js / npm is required for Railgun privacy features but was not found on this system.
  > Please install Node.js from https://nodejs.org ..."

- `RailgunEngine::start_clients()` now uses the smart path.
- Added `RailgunEngine::is_node_available()` and `ensure_sidecars_ready()`.
- Re-exported the helpers from the crate root.

This makes the single-binary distribution story complete for the sidecars.


## Fee Quote + Real Proof Flow in RailgunEngine (2026-07-01)

Implemented the core integration:

### Fee quote flow
- `RailgunEngine::get_best_fee_quote(token_address)` / `get_all_fee_quotes`
- Delegates directly to the owned `WakuSidecarClient`
- `prepare_broadcaster_unshield_with_proof(...)` and `build_unshield_calldata_via_broadcaster(...)` automatically fetch the quote and attach `fees_id` + broadcaster railgun address.

### Proof flow
- `generate_unshield_proof(prepared, circuit_variant)` 
  - builds `ProofRequest`
  - calls `prover_client.prove_with_inputs`
  - converts with `snark_proof_from_sidecar`
- `build_unshield_calldata(...)` : prepare → real proof → calldata in one call
- `build_unshield_calldata_via_broadcaster(...)` : full auto for gas-sponsored path

### Error handling / fallbacks
- Explicit checks: "Call start_clients() first"
- Clear message when no fee quote: "No broadcaster fee quote available... wait for fee messages or use self-broadcast"
- Prover errors (timeout, sidecar errors) bubble up naturally from the client
- Broadcaster not started → the get_*_fee_quote will just return empty (no panic)

### Other cleanups
- `prepare_unshield_for_broadcaster` no longer builds dummy proof/calldata inside (real proof is now done at the Engine level)
- `SelectedBroadcaster` re-exported from the crate root

The three requested flows are now wired end-to-end inside `RailgunEngine`.
Users can do privacy unshield (with or without broadcaster) with real ZK proofs.


## High-Level Shield / Unshield Public APIs (2026-07-01)

Added the three requested final high-level methods on `RailgunEngine`:

```rust
pub async fn shield(&self, token: TokenData, value: U256, memo: Option<String>) -> Result<Vec<u8>>
pub async fn unshield(&self, to: Address, token: TokenData, amount: U256) -> Result<Vec<u8>>
pub async fn unshield_via_broadcaster(&self, to: Address, token: TokenData, amount: U256) -> Result<WakuTransactResponse>
```

- `shield` and `unshield` (fallback) return raw calldata (caller signs & broadcasts).
- `unshield_via_broadcaster` performs the full flow: quote → prepare → real proof → calldata → Waku encrypted transact → returns `WakuTransactResponse` (with tx_hash or error).
- `unshield_via_broadcaster` takes `&mut self` (required by the broadcaster client for sending).
- Added `build_shield_transact_calldata` builder.
- `WakuTransactResponse` is re-exported from the crate root.
- Shield currently falls back to dummy proof (shield witness/proof request still needs dedicated work, same as early unshield days).
- All existing tests continue to pass.


## Shield Proof Generation (2026-07-01)

Implemented full proof generation path for `shield`:

- Added `build_shield_proof_request(scanner, keys, prepared: &PreparedShield, variant) -> ProofRequest`
  - 0 nullifiers, 1 commitment out
  - Empty input arrays (random_in, value_in, path_elements, leaves_indices)
  - Populates npk_out + value_out from the created note
  - Uses `UnshieldType::NONE` for BoundParams

- Added `RailgunEngine::generate_shield_proof(&self, prepared, variant) -> SnarkProof` (real sidecar call)

- Added `RailgunEngine::build_shield_proof_request(...)` wrapper for inspection

- Updated high-level `pub async fn shield(&self, ...)` to use real proof (no more dummy fallback)

- Added unit test `test_build_shield_proof_request_shape` (verifies empty inputs, 1 output commitment, etc.)

- All 20 tests pass.
