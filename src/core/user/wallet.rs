use alloy_signer_local::{MnemonicBuilder, PrivateKeySigner, coins_bip39::English};
use anyhow::anyhow;
use secure_types::SecureString;
use std::str::FromStr;
use zeus_eth::{alloy_primitives::Address, utils::SecureSigner};

#[derive(Clone, Default, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WalletInfo {
   pub name: String,
   pub address: Address,
}

impl WalletInfo {
   pub fn address_string(&self) -> String {
      self.address.to_string()
   }

   pub fn address_truncated(&self) -> String {
      format!(
         "{}...{}",
         &self.address_string()[..6],
         &self.address_string()[36..]
      )
   }
}

/// User Wallet
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Wallet {
   pub info: WalletInfo,
   pub key: SecureSigner,
}

impl Wallet {
   pub fn is_key_erased(&self) -> bool {
      self.key.is_erased()
   }

   /// Return the wallet's key
   pub fn key_string(&self) -> SecureString {
      self.key.key_string()
   }

   /// Create a new wallet from a random private key
   pub fn new_rng(name: String) -> Self {
      let key = SecureSigner::random();
      let info = WalletInfo {
         name,
         address: key.address(),
      };

      Self { info, key }
   }

   /// Create a new wallet from a given private key
   pub fn new_from_key(name: String, key_str: SecureString) -> Result<Self, anyhow::Error> {
      let key = key_str.str_scope(|key_str| PrivateKeySigner::from_str(key_str))?;
      let key = SecureSigner::from(key);
      let info = WalletInfo {
         name,
         address: key.address(),
      };

      Ok(Self { info, key })
   }

   /// Create a new wallet from a mnemonic phrase
   pub fn new_from_mnemonic(name: String, phrase: SecureString) -> Result<Self, anyhow::Error> {
      // return a custom error to not expose the phrase in case it just has a typo
      let phrase_string = phrase.str_scope(|phrase| phrase.to_string());
      let wallet = MnemonicBuilder::<English>::default()
         .phrase(phrase_string)
         .index(0)?
         .build()
         .map_err(|_| anyhow!("It seems that the given phrase is invalid"))?;
      let key = SecureSigner::from(wallet);

      let info = WalletInfo {
         name,
         address: key.address(),
      };

      Ok(Self { info, key })
   }
}
