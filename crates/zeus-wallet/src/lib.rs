pub mod secure_key;
pub mod wallet;

pub use secure_key::SecureKey;
pub use wallet::{SecureHDWallet, Wallet, derive_seed};

use alloy_signer_local::LocalSignerError;
use argon2_rs::error::Argon2Error;
use k256::ecdsa::Error as EcdsaError;
use secure_types::Error as SecureError;
use zeus_bip32::error::Bip32Error;

pub enum Error {
   SeedLengthTooShort(String),
   XKeyInfoIsMissing(String),
   WalletIsNotChildOrMaster(String),
   ChildAlreadyExists(String),
   SecureError(SecureError),
   Argon2Error(Argon2Error),
   LocalSignerError(LocalSignerError),
   Bip32Error(Bip32Error),
   EcdsaError(EcdsaError),
   Custom(String),
}

impl std::fmt::Display for Error {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Error::SeedLengthTooShort(s) => write!(f, "{}", s),
         Error::XKeyInfoIsMissing(s) => write!(f, "{}", s),
         Error::WalletIsNotChildOrMaster(s) => write!(f, "{}", s),
         Error::ChildAlreadyExists(s) => write!(f, "{}", s),
         Error::SecureError(e) => write!(f, "{}", e),
         Error::Argon2Error(e) => write!(f, "{}", e),
         Error::LocalSignerError(e) => write!(f, "{}", e),
         Error::Bip32Error(e) => write!(f, "{}", e),
         Error::EcdsaError(e) => write!(f, "{}", e),
         Error::Custom(s) => write!(f, "{}", s),
      }
   }
}

impl std::fmt::Debug for Error {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      write!(f, "{:?}", self)
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

impl From<EcdsaError> for Error {
   fn from(e: EcdsaError) -> Self {
      Error::EcdsaError(e)
   }
}
