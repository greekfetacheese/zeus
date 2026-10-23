//! BIP32 key derivation for Zeus.
//!
//! The crate derives an extended private key tree from a seed and exposes the matching public
//! keys, chain codes and fingerprints. Derivation is hardened and non-hardened as BIP32 defines
//! it, and paths parse from and render to the standard `m/44'/60'/0'/0/0` form.
//!
//! A [`Hint`] is the xpub/ypub/zpub convention recorded on a key: it says which address type the
//! key was intended for. Downstream crates are free to follow or ignore it.
#![doc = include_str!("../readme.md")]
#![warn(missing_docs)]

/// The error type returned by this crate's fallible operations.
pub mod error;
/// BIP32 derivation paths, their parsing and their rendering.
pub mod path;
/// The building blocks shared by extended private and public keys.
pub mod primitives;
/// Extended private keys, extended public keys and root key generation.
pub mod xpriv;

pub use path::*;
pub use primitives::*;
pub use xpriv::{SecureXPriv, root_from_seed};
