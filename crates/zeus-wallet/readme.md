# <p align="center">Zeus-Wallet</p>

## Implementation of an Hierarchical Deterministic Wallet (BIP32) that can be derived from a username and password.

## Part of [Zeus](https://github.com/greekfetacheese/zeus).


# Usage

```rust
use secure_types::SecureString;
use zeus_wallet::{DeriveVersion, DeriveMethod, Deriver, SecureHDWallet};

let username = SecureString::from("username");
let password = SecureString::from("password");

// `DeriveVersion::V1` is the scheme Zeus uses in production. It pins both the
// Argon2 parameters used to derive the seed and the method used to derive keys
// from it (see Versioning below).
let deriver = Deriver::new(DeriveVersion::V1, Some(username), Some(password));

// Derive the seed from the credentials and build the HD wallet from it
let mut hd_wallet =
   SecureHDWallet::new_from_deriver(Some("My Wallet".to_string()), deriver).unwrap();

println!("Wallet Address: {}", hd_wallet.master_wallet.address());

// Generate 10 child wallets using the master wallet
for i in 0..10 {
   let name = format!("Child Wallet {}", i);
   hd_wallet.derive_child(name).unwrap();
}

for (i, child) in hd_wallet.children.iter().enumerate() {
   assert!(!child.is_master());
   assert!(!child.is_imported());
   assert!(child.is_hardened());
   assert!(child.is_child());

   let path = child.derivation_path_string();
   println!(
      "Child: {} Path: {} Address: {}",
      i,
      path,
      child.address()
   );
}
```

# Derivation API

A derivation has two stages, and a version names them together:

| Stage | Type | Purpose |
|-------|------|---------|
| KDF | **`Argon2Params`** | Turns the credentials into the 64-byte seed. `Argon2Params::zeus_v1()`, or `Argon2Params::new(m_cost, t_cost, p_cost)`. |
| Keys | **`DeriveMethod`** | Turns the seed into keys and derivation paths. `BIP32` is the standard (and currently only) method. |

| Type | Purpose |
|------|---------|
| **`Deriver`** | Holds the `DeriveVersion` and the credentials. `derive_kdf_seed()` returns the 64-byte seed, `new_hd_wallet(name)` derives the seed and returns a `SecureHDWallet`. |
| **`DeriveVersion`** | The complete derivation scheme: which Argon2 parameters derive the seed *and* which method derives keys. `DeriveVersion::V1` (production) and `DeriveVersion::Custom(params, derive_method)` (dev builds / tests). |

## Building a deriver

`Deriver::new` takes the version plus both credentials at once; `Deriver::zeus_v1()` starts with
the production version and no credentials, which are then set separately:

```rust
use secure_types::SecureString;
use zeus_wallet::{Deriver, SecureHDWallet};

let mut deriver = Deriver::zeus_v1();

deriver.set_username(SecureString::from("username"));
deriver.set_password(SecureString::from("password"));

// The seed alone (64 bytes), or straight to the HD wallet
let seed = deriver.derive_kdf_seed().unwrap();
let hd_wallet = SecureHDWallet::new_from_deriver(None, deriver).unwrap();
```

A deriver with no credentials set fails to derive: `derive_kdf_seed()` returns
`Error::UsernameIsMissing` / `Error::PasswordIsMissing`, and empty credentials return
`Error::UsernameIsEmpty` / `Error::PasswordIsEmpty`. The missing checks run first, so the
empty-credential errors are only reached once both credentials are set.

A version reports the stages it pins: `kdf_params()` returns its `Argon2Params` and `method()` its
`DeriveMethod`.

## Custom Argon2 parameters

`DeriveVersion::Custom` takes caller supplied Argon2 parameters & DeriveMethod. It exists for `dev` builds and
tests (it allows much lower costs, so be careful with it in production), and it is not a wallet
generation: a wallet derived with arbitrary parameters is only recoverable with those exact
parameters.

```rust
use zeus_wallet::{Argon2Params, DeriveVersion, Deriver};

// This is just an example, in reality you should use way higher values
let m_cost = 64_000; // 64 MB of memory
let t_cost = 8; // 8 iterations
let p_cost = 1; // 1 parallel thread

let method = DeriveMethod::BIP32;
let version = DeriveVersion::Custom(Argon2Params::new(m_cost, t_cost, p_cost), method);

// `None` credentials: only inspecting the version here, not deriving with it
let deriver = Deriver::new(version, None, None);

// The parameters this version derives the seed with
assert_eq!(deriver.version().kdf_params().argon2().m_cost, m_cost);
```

`Argon2Params::argon2()` returns the underlying `Argon2`. `argon2-rs` is re-exported
(`zeus_wallet::argon2_rs`) if you need to build parameters it does not expose through
`Argon2Params`, without adding the dependency yourself.

## Versioning

The parameters of each `DeriveVersion` are fixed, so wallets stay recoverable: changing the
derivation cost — or the derivation method — means adding a **new** variant rather than editing an
existing one.

`DeriveVersion::V1` — the scheme Zeus uses for the master wallet:

- **Salt:** SHA3-512 of the username
- **Memory cost:** 8192_000 KiB (8192 MB)
- **Iterations:** 96
- **Parallelism:** 1
- **Argon2 version:** 0x13
- **Hash length:** 64 bytes (512 bits)
- **Method:** [BIP32](https://github.com/bitcoin/bips/blob/master/bip-0032.mediawiki)

# Errors

`zeus_wallet::Error` implements `Display` and `std::error::Error`, and converts from the underlying
`secure_types`, `argon2-rs`, `alloy-signer-local`, `zeus-bip32` and `k256` errors.
