# zeus-railgun-shared

Common types, address encoding, key derivation, and cryptographic primitives for the Railgun privacy protocol.

This crate exists to share the foundational Railgun protocol logic across multiple Zeus crates without creating cyclic dependencies.

## Purpose

- Provide a single source of truth for Railgun-specific address handling (`0zk...` addresses) and key derivation.
- Expose BabyJubJub and Poseidon primitives used by both the core engine and the Waku broadcaster.
- Allow `zeus-railgun` (the privacy engine) and `zeus-waku-broadcaster` (the gas-abstraction client) to depend on the same primitives.
- Keep heavy protocol logic out of higher-level crates while making it reusable.

## What lives here

- `RailgunAddress` + `encode_address` / `RailgunAddress::from_zk_address` — official `0zk` bech32m address format.
- `RailgunKeys` — complete key set (spending + viewing private/public, nullifying key, master public key).
- BabyJubJub operations (`compute_spending_key`, `compute_viewing_key`, `babyjub_shared_secret` for broadcaster ECDH).
- Poseidon hash (`poseidon_hash`) used for commitments, nullifiers and master public keys.
- `Chain` type (Ethereum mainnet, Polygon, etc.).
- Secure key handling using `secure-types`.

Higher-level logic (notes, scanner, merkle trees, transaction builders, ZK witness construction) lives in `zeus-railgun`.

## Usage Example

```rust
use secure_types::SecureArray;
use zeus_railgun_shared::{
    RailgunKeys, Chain,
    address::{generate_address_data, encode_address, decode_address},
    crypto::babyjub_shared_secret,
};

// Generate keys from a 64-byte seed (e.g. from BIP39)
let seed: SecureArray<u8, 64> = ...; // from wallet
let keys = RailgunKeys::new(seed.clone(), 0)?;

// Create a Railgun address (0zk...)
let addr = generate_address_data(seed, 0, Some(Chain::ETHEREUM_MAINNET))?;
let zk_address = encode_address(&addr)?; // "0zk1qy9r46..."

// For broadcaster: compute ECDH shared secret
let (random_priv, broadcaster_viewing_pub): ([u8; 32], [u8; 32]) = ...;
let (pub_r, shared) = babyjub_shared_secret(&random_priv, &broadcaster_viewing_pub)?;
```

## Crates that depend on it

- `zeus-railgun` — core engine (notes, scanner, builders, ZK integration)
- `zeus-waku-broadcaster` — fee messages, selection, encrypted transact payloads

## Versioning note

This crate is intentionally small and stable. Breaking changes here affect multiple downstream crates, so we keep the API surface focused.

See the top-level `RAILGUN_ROADMAP.md` for the overall integration plan.
