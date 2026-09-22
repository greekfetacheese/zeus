pub mod derive;
pub mod secure_key;
pub mod wallet;

pub use derive::{Argon2Params, DeriveMethod, DeriveVersion, Deriver};
pub use secure_key::SecureKey;
pub use wallet::{SecureHDWallet, Wallet};

pub use argon2_rs;

use alloy_primitives::Address;
use alloy_signer_local::LocalSignerError;
use argon2_rs::error::Argon2Error;
use k256::ecdsa::Error as EcdsaError;
use secure_types::Error as SecureError;
use zeus_bip32::{DerivationPath, error::Bip32Error};

#[derive(Debug)]
pub enum Error {
   UsernameIsEmpty,
   PasswordIsEmpty,
   UsernameIsMissing,
   PasswordIsMissing,
   SeedLengthTooShort,
   XKeyInfoIsMissing,
   WalletIsNotChildOrMaster,
   ChildAlreadyExists {
      path: DerivationPath,
      address: Address,
   },
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
         Error::EcdsaError(e) => write!(f, "{}", e),
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

impl From<EcdsaError> for Error {
   fn from(e: EcdsaError) -> Self {
      Error::EcdsaError(e)
   }
}
