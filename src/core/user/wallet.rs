use std::str::FromStr;
use alloy_signer_local::PrivateKeySigner;
use zeus_eth::wallet::SecureSigner;
use secure_types::SecureString;

/// User Wallet
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Wallet {
   /// Name of the wallet (if empty, we generate a name)
   pub name: String,

   pub notes: String,

   /// Hide this wallet from the GUI?
   pub hidden: bool,

   /// The key of the wallet
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

      Self {
         name,
         notes: String::new(),
         hidden: false,
         key,
      }
   }

   /// Create a new wallet from a given private key
   pub fn new_from_key(name: String, notes: String, hidden: bool, key_str: SecureString) -> Result<Self, anyhow::Error> {
      let key = PrivateKeySigner::from_str(key_str.borrow())?;
      let key = SecureSigner::new(key);

      Ok(Self {
         name,
         notes,
         hidden,
         key,
      })
   }

   pub fn address(&self) -> String {
      self.key.borrow().address().to_string()
   }

   /// Get the wallet address truncated
   pub fn address_truncated(&self) -> String {
      let address = self.key.borrow().address().to_string();
      format!("{}...{}", &address[..6], &address[36..])
   }
}
