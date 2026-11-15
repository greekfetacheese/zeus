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
//! [`std::error::Error`] by hand, so it composes with `anyhow`. The wrapped variants
//! render as the inner error's own message.
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
use secure_types::{Error as SecureError, SecureString};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use zeus_bip32::{DerivationPath, XKeyInfo, error::Bip32Error};

use std::sync::{Arc, Mutex};

/// A seed phrase behind a shared lock, so cloning a wallet does not copy secure
/// memory.
///
/// Every clone shares one phrase: erasing it through any clone zeroes it for all
/// of them.
pub struct SecurePhrase(Arc<Mutex<SecureString>>);

impl SecurePhrase {
   /// Wrap a seed phrase in a shared lock
   pub fn new(phrase: SecureString) -> Self {
      Self(Arc::new(Mutex::new(phrase)))
   }

   /// Acquire the lock for read access to the phrase
   pub fn lock<R>(&self, read: impl FnOnce(&SecureString) -> R) -> R {
      read(&self.0.lock().unwrap())
   }

   /// Acquire the lock for mutable access to the phrase
   pub fn lock_mut<R>(&self, write: impl FnOnce(&mut SecureString) -> R) -> R {
      write(&mut self.0.lock().unwrap())
   }
}

impl Clone for SecurePhrase {
   fn clone(&self) -> Self {
      Self(Arc::clone(&self.0))
   }
}

#[cfg(feature = "serde")]
impl Serialize for SecurePhrase {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: serde::Serializer,
   {
      self.lock(|phrase| phrase.serialize(serializer))
   }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for SecurePhrase {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      let phrase = SecureString::deserialize(deserializer)?;
      Ok(Self::new(phrase))
   }
}

/// An [`XKeyInfo`] behind a shared lock, so cloning a wallet does not copy the
/// chain code in secure memory.
///
/// Every clone shares one [`XKeyInfo`]: erasing it through any clone zeroes the
/// chain code for all of them.
pub struct SecureXKeyInfo(Arc<Mutex<XKeyInfo>>);

impl SecureXKeyInfo {
   /// Wrap an [`XKeyInfo`] in a shared lock
   pub fn new(xkey_info: XKeyInfo) -> Self {
      Self(Arc::new(Mutex::new(xkey_info)))
   }

   /// Acquire the lock for read access to the info
   pub fn lock<R>(&self, read: impl FnOnce(&XKeyInfo) -> R) -> R {
      read(&self.0.lock().unwrap())
   }

   /// Acquire the lock for mutable access to the info
   pub fn lock_mut<R>(&self, write: impl FnOnce(&mut XKeyInfo) -> R) -> R {
      write(&mut self.0.lock().unwrap())
   }
}

impl Clone for SecureXKeyInfo {
   fn clone(&self) -> Self {
      Self(Arc::clone(&self.0))
   }
}

#[cfg(feature = "serde")]
impl Serialize for SecureXKeyInfo {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: serde::Serializer,
   {
      self.lock(|xkey_info| xkey_info.serialize(serializer))
   }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for SecureXKeyInfo {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      let xkey_info = XKeyInfo::deserialize(deserializer)?;
      Ok(Self::new(xkey_info))
   }
}

/// Everything this crate can fail with.
///
/// Wrapped variants render as the underlying error's own message, so an `anyhow`
/// report reads the same as the cause it wraps.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
   /// A required username was set, but it is empty.
   UsernameIsEmpty,
   /// A required password was set, but it is empty.
   PasswordIsEmpty,
   /// No username was given to the deriver.
   UsernameIsMissing,
   /// No password was given to the deriver.
   PasswordIsMissing,
   /// The KDF produced a seed that is not the 64 bytes BIP32 expects.
   SeedLengthTooShort,
   /// The wallet needs its [`zeus_bip32::XKeyInfo`] (path and chain code) but has none.
   XKeyInfoIsMissing,
   /// A seed was requested from a wallet that neither derives from a master nor
   /// carries an imported seed phrase, so there is nothing to derive it from.
   WalletIsNotChildOrMaster,
   /// [`SecureHDWallet::derive_child_at_mut`] was called for a path already
   /// present in `children`; no second wallet was added.
   ChildAlreadyExists {
      /// The derivation path of the wallet that already exists.
      path: DerivationPath,
      /// The address of the wallet that already exists.
      address: Address,
   },
   /// A [`secure_types`] operation failed.
   SecureError(SecureError),
   /// Hashing the credentials with Argon2 failed.
   Argon2Error(Argon2Error),
   /// Parsing or handling a local signer key failed.
   LocalSignerError(LocalSignerError),
   /// Building or deriving a BIP32 key failed.
   Bip32Error(Bip32Error),
   /// A crate-specific failure whose message is built at the call site.
   Custom(String),
}

impl std::fmt::Display for Error {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Error::UsernameIsEmpty => write!(f, "Username is empty"),
         Error::PasswordIsEmpty => write!(f, "Password is empty"),
         Error::UsernameIsMissing => write!(f, "Username is missing"),
         Error::PasswordIsMissing => write!(f, "Password is missing"),
         Error::SeedLengthTooShort => write!(f, "Seed is not 64 bytes long"),
         Error::XKeyInfoIsMissing => write!(f, "XKeyInfo is missing"),
         Error::WalletIsNotChildOrMaster => write!(
            f,
            "Could not derive seed, The wallet must be either Master/Child or imported from a seed phrase"
         ),
         Error::ChildAlreadyExists { path, address } => write!(
            f,
            "Wallet At {} with Address {} already exists",
            path.derivation_string(),
            address
         ),
         Error::SecureError(e) => write!(f, "{}", e),
         Error::Argon2Error(e) => write!(f, "{}", e),
         Error::LocalSignerError(e) => write!(f, "{}", e),
         Error::Bip32Error(e) => write!(f, "{}", e),
         Error::Custom(s) => write!(f, "{}", s),
      }
   }
}

impl std::error::Error for Error {}

impl From<SecureError> for Error {
   fn from(e: SecureError) -> Self {
      Error::SecureError(e)
   }
}

impl From<Argon2Error> for Error {
   fn from(e: Argon2Error) -> Self {
      Error::Argon2Error(e)
   }
}

impl From<LocalSignerError> for Error {
   fn from(e: LocalSignerError) -> Self {
      Error::LocalSignerError(e)
   }
}

impl From<Bip32Error> for Error {
   fn from(e: Bip32Error) -> Self {
      Error::Bip32Error(e)
   }
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

      // A wrapped variant renders as the inner error's own `Display`, which is what
      // the hand-written `impl Display for Error` delegates to.
      assert_eq!(
         Error::from(Argon2Error::IncorrectParameter).to_string(),
         Argon2Error::IncorrectParameter.to_string()
      );
   }
}
