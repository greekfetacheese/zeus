//! Hierarchical deterministic (BIP32) wallets derived from a username and a password.
//!
//! The two credentials go through Argon2id to produce a 64-byte seed, and that seed
//! becomes the master wallet every child is derived from. A derivation therefore has
//! two stages, and [`DeriveVersion`] pins both of them together: the
//! [`Argon2Params`] that turn the credentials into the seed and the [`DeriveMethod`]
//! that turns the seed into keys. Pinning only one of the two would not reproduce a
//! wallet.
//!
//! - [`Deriver`] holds a version plus the credentials. [`Deriver::derive_kdf_seed`]
//!   returns the 64-byte seed; [`Deriver::new_hd_wallet`] returns the whole
//!   [`SecureHDWallet`].
//! - [`SecureHDWallet`] owns the master [`Wallet`] and the children derived from it,
//!   each at `m/44'/60'/0'/0/0/{index}` — `{index}'` (hardened) when derived through
//!   [`SecureHDWallet::derive_child`], a raw index through the `_at` methods.
//! - [`Wallet`] is one key: a master, a child of one, or an imported key that carries
//!   no derivation info.
//!
//! # Errors
//!
//! Fallible entry points return [`Error`]. It converts [`From`] each underlying error
//! (`secure_types`, `argon2-rs`, `alloy-signer-local`, `zeus-bip32`) and implements
//! [`std::error::Error`], so it composes with `anyhow` and exposes the wrapped cause
//! through [`std::error::Error::source`].
#![warn(missing_docs)]

/// The derivation stages: the Argon2 parameters, the key-derivation method, the
/// version that pins both, and the [`Deriver`] that runs them.
pub mod derive;
/// The [`SecureKey`] wrapper: a private key and its address, held in secure memory.
pub mod secure_key;
/// The wallet types: a single [`Wallet`] and the [`SecureHDWallet`] that derives
/// children from a master.
pub mod wallet;

/// Re-export of the `argon2-rs` crate, so callers can build
/// [`Argon2Params`] the wrapper does not expose without adding the dependency.
pub use argon2_rs;
/// The derivation API: parameters, method, version and [`Deriver`].
pub use derive::{Argon2Params, DeriveMethod, DeriveVersion, Deriver};
/// The secure private key wrapper.
pub use secure_key::SecureKey;
/// The wallet types.
pub use wallet::{SecureHDWallet, Wallet};

use alloy_primitives::Address;
use alloy_signer_local::LocalSignerError;
use argon2_rs::error::Argon2Error;
use secure_types::Error as SecureError;
use zeus_bip32::{DerivationPath, error::Bip32Error};

/// Everything this crate can fail with.
///
/// Wrapped variants keep the underlying error as their [`std::error::Error::source`],
/// so an `anyhow` chain shows both the outer and the inner message.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
   /// A required username was set, but it is empty.
   #[error("Username is empty")]
   UsernameIsEmpty,
   /// A required password was set, but it is empty.
   #[error("Password is empty")]
   PasswordIsEmpty,
   /// No username was given to the deriver.
   #[error("Username is missing")]
   UsernameIsMissing,
   /// No password was given to the deriver.
   #[error("Password is missing")]
   PasswordIsMissing,
   /// The KDF produced a seed that is not the 64 bytes BIP32 expects.
   #[error("Seed is not 64 bytes long")]
   SeedLengthTooShort,
   /// The wallet needs its [`zeus_bip32::XKeyInfo`] (path and chain code) but has none.
   #[error("XKeyInfo is missing")]
   XKeyInfoIsMissing,
   /// A seed was requested from a wallet that neither derives from a master nor
   /// carries an imported seed phrase, so there is nothing to derive it from.
   #[error(
      "Could not derive seed, The wallet must be either Master/Child or imported from a seed phrase"
   )]
   WalletIsNotChildOrMaster,
   /// [`SecureHDWallet::derive_child_at_mut`] was called for a path already
   /// present in `children`; no second wallet was added.
   #[error("Wallet At {} with Address {} already exists", path.derivation_string(), address)]
   ChildAlreadyExists {
      /// The derivation path of the wallet that already exists.
      path: DerivationPath,
      /// The address of the wallet that already exists.
      address: Address,
   },
   /// A [`secure_types`] operation failed.
   #[error(transparent)]
   SecureError(#[from] SecureError),
   /// Hashing the credentials with Argon2 failed.
   #[error(transparent)]
   Argon2Error(#[from] Argon2Error),
   /// Parsing or handling a local signer key failed.
   #[error(transparent)]
   LocalSignerError(#[from] LocalSignerError),
   /// Building or deriving a BIP32 key failed.
   #[error(transparent)]
   Bip32Error(#[from] Bip32Error),
   /// A crate-specific failure whose message is built at the call site.
   #[error("{0}")]
   Custom(String),
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::address;
   use std::str::FromStr;
   use zeus_bip32::DerivationPath;

   #[test]
   fn test_error_display_messages() {
      let path = DerivationPath::from_str("m/44'/60'/0'/0/0/9").unwrap();
      let address = address!("0xD5aE2042392b91D8d4C3bB48A294B4d61F6CA97c");

      let cases: &[(Error, &str)] = &[
         (Error::UsernameIsEmpty, "Username is empty"),
         (Error::PasswordIsEmpty, "Password is empty"),
         (Error::UsernameIsMissing, "Username is missing"),
         (Error::PasswordIsMissing, "Password is missing"),
         (
            Error::SeedLengthTooShort,
            "Seed is not 64 bytes long",
         ),
         (Error::XKeyInfoIsMissing, "XKeyInfo is missing"),
         (
            Error::WalletIsNotChildOrMaster,
            "Could not derive seed, The wallet must be either Master/Child or imported from a seed phrase",
         ),
         (Error::Custom("boom".to_string()), "boom"),
         (
            Error::ChildAlreadyExists { path, address },
            "Wallet At m/44'/60'/0'/0/0/9 with Address 0xD5aE2042392b91D8d4C3bB48A294B4d61F6CA97c already exists",
         ),
      ];

      for (error, expected) in cases {
         assert_eq!(&error.to_string(), expected);
      }

      // A wrapped variant renders as the inner error's `Display`, both today by
      // hand and through `#[error(transparent)]` once the derive lands.
      assert_eq!(
         Error::from(Argon2Error::IncorrectParameter).to_string(),
         Argon2Error::IncorrectParameter.to_string()
      );
   }
}
