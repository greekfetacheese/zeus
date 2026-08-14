# <p align="center">Zeus-BIP32</p>

BIP32 hierarchical deterministic key derivation for [Zeus](https://github.com/greekfetacheese/zeus).

A slim, security-oriented fork of [`coins-bip32`](https://github.com/summa-tx/coins). It derives secp256k1 extended keys from a seed and walks a derivation path. Private keys and chain codes live in [`secure-types`](https://crates.io/crates/secure-types) so they can be locked and zeroized.

Higher-level wallet types (username + password → seed, child bookkeeping, Ethereum addresses) live in [`zeus-wallet`](https://github.com/greekfetacheese/zeus/tree/master/crates/zeus-wallet).

## What it does

- Build a master key from a seed (`root_from_seed`, HMAC-SHA512 with the BIP32 `"Bitcoin seed"` key)
- Derive hardened and normal children along a path (`SecureXPriv::derive_path`)
- Parse and format BIP32 paths (`m/44'/60'/0'/0/0`, `h` or `'` for hardened)
- Keep `XKeyInfo` (depth, parent fingerprint, index, chain code) next to the secret
- Optional `serde` for `SecureXPriv`, `XKeyInfo`, and `DerivationPath`

Default Ethereum path constants:

```text
m/44'/60'/0'/0/0     DEFAULT_DERIVATION_PATH
m/44'/60'/0'/0/      DEFAULT_DERIVATION_PATH_PREFIX
```

Hardened indices use `BIP32_HARDEN` (`0x8000_0000`). Zeus derives HD children as `base_path.extended(index + BIP32_HARDEN)`.


## Usage

```rust
use std::str::FromStr;
use zeus_bip32::{
    BIP32_HARDEN, DEFAULT_DERIVATION_PATH, DerivationPath, SecureXPriv, root_from_seed,
};

// In Zeus this seed comes from Argon2id(username, password) via zeus-wallet::derive_seed.
let seed = [0x42u8; 64];
let (key, xkey_info) = root_from_seed(&seed, None)?;
let master = SecureXPriv::new(key, xkey_info);

// m/44'/60'/0'/0/0/{index}'
let path = DerivationPath::from_str(DEFAULT_DERIVATION_PATH)?.extended(0 + BIP32_HARDEN);
let child = master.derive_path(path)?;

assert_eq!(child.xkey_info.depth, 6);
assert!(child.xkey_info.index >= BIP32_HARDEN);

// Unlock only for the call, the buffer is not copied out.
child.key.unlock(|bytes| assert_eq!(bytes.len(), 32));
# Ok::<(), zeus_bip32::error::Bip32Error>(())
```

Seed must be at least 16 bytes (BIP32). Prefer 64 bytes.

## Features

- `serde` — serialize `DerivationPath` as its `m/…` string; serialize key material via `secure-types`

## License

MIT OR Apache-2.0. Derived from [coins](https://github.com/summa-tx/coins).
