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
use secure_types::Error as SecureError;
use zeus_bip32::{DerivationPath, error::Bip32Error};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
   #[error("Username is empty")]
   UsernameIsEmpty,
   #[error("Password is empty")]
   PasswordIsEmpty,
   #[error("Username is missing")]
   UsernameIsMissing,
   #[error("Password is missing")]
   PasswordIsMissing,
   #[error("Seed is not 64 bytes long")]
   SeedLengthTooShort,
   #[error("XKeyInfo is missing")]
   XKeyInfoIsMissing,
   #[error(
      "Could not derive seed, The wallet must be either Master/Child or imported from a seed phrase"
   )]
   WalletIsNotChildOrMaster,
   #[error("Wallet At {} with Address {} already exists", path.derivation_string(), address)]
   ChildAlreadyExists {
      path: DerivationPath,
      address: Address,
   },
   #[error(transparent)]
   SecureError(#[from] SecureError),
   #[error(transparent)]
   Argon2Error(#[from] Argon2Error),
   #[error(transparent)]
   LocalSignerError(#[from] LocalSignerError),
   #[error(transparent)]
   Bip32Error(#[from] Bip32Error),
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
