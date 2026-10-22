# zeus-railgun

A Railgun shielded-pool client for Rust: key derivation, notes and UTXOs, merkle trees, Groth16
proving against circom artifacts, RPC and Subsquid syncers, redb persistence, and private
proof-of-innocence support.

`zeus-railgun` is a fork of
[ethereum/kohaku's `crates/railgun`](https://github.com/ethereum/kohaku/tree/master/crates/railgun),
maintained for [Zeus](https://github.com/greekfetacheese/zeus). Upstream has since deprecated and
removed its Rust implementations; this fork keeps the crate alive for Zeus.

## What this fork changes

- Any kind of secret/private key is handled by the `secure-types` crate, and `zeroize` is used to
  wipe intermediates where needed.
- Added decoding of legacy pre-Mar23 events, so the `RpcSyncer` can sync the indexer from scratch
  without the `SubsquidSyncer` (legacy note *decryption* is still a stub).
- Added a redb database implementation for disk persistence.
- Added disk persistence for the `RemoteArtifactLoader`, with SHA-256 pinned artifacts.
- Tree-state serialization with bincode, which is much more space efficient than JSON.
- Added a snapshot loader that persists and loads all `SyncEvent`s to disk for faster syncing.

## Installation

```bash
cargo add zeus-railgun
```

## Modules

- `abi` — generated Railgun smart-wallet and legacy contract bindings.
- `account` — key derivation, Railgun addresses and the signer.
- `adapter_data` — paymaster/adaptor data and its ABI encoding.
- `caip` — CAIP-10 style asset identifiers and token data.
- `chain_config` — per-chain Railgun deployment configuration.
- `circuit` — Groth16 prover, circuit inputs/witnesses, artifact pins and the remote artifact loader.
- `crypto` — BabyJubJub, Poseidon, hashing and note-encryption primitives.
- `database` — redb-backed persistent store with encrypted account/POI envelopes.
- `indexer` — `RpcSyncer`, `SubsquidSyncer`, `UtxoIndexer` and the snapshot loader.
- `merkle_tree` — UTXO and TXID merkle trees, roots and proofs.
- `note` — transfer/unshield notes, UTXO encryption and operation shapes.
- `poi` — private proof-of-innocence client, note and provider.
- `privacy` — unshield amount scoring and safer-amount suggestions.
- `provider` — the gas-paying provider, balances and note queries.
- `transact` — transaction/shield builders and proved operations.
- `types` — shared keys, chain types and serialisable primitives.

## License

MIT. See `LICENCE`.
