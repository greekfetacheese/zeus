# zeus-railgun

Native Rust implementation of Railgun privacy primitives for the Zeus wallet.

This crate will eventually contain:

- zk-Railgun address (0zk...) generation and encoding (partially complete)
- Key derivation (spending + viewing keys)
- Note model, encryption/decryption
- Poseidon Merkle trees and proofs
- Private transaction building
- On-chain contract interactions (via alloy, shared with zeus-eth)
- Shield / unshield / private transact logic

**Waku Broadcaster client** lives in the sibling crate `zeus-waku-broadcaster` (to keep networking concerns separate).

See the top-level `RAILGUN_ROADMAP.md` for the full phased plan, current status, and tracking.

## Current Status

- Basic address generation + bech32m encoding works and produces addresses very similar to Railway wallet (see tests in `src/address.rs`).
- Uses `light-poseidon`, arkworks (bn254 + ed-on-bn254), curve25519-dalek, secure-types, etc.

## Usage (future)

```rust
use zeus_railgun::address::{generate_address_data, encode_address, Chain};

let address_data = generate_address_data(seed, index, Some(chain))?;
let zk_address = encode_address(&address_data)?;
```

## References

- Official Railgun: https://railgun.org/
- Dev guide: https://docs.railgun.org/developer-guide
- Local reference implementations in `/home/cion/Railgun/`

## Development

Part of the Zeus workspace. Run tests from root or crate dir:

```bash
cargo test -p zeus-railgun
```

Long-term project — see roadmap for milestones.
