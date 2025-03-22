use super::wallet::Wallet;
use crate::core::utils::data_dir;
use anyhow::anyhow;
use ncrypt_me::{Argon2Params, Credentials, decrypt_data, encrypt_data, zeroize::Zeroize};
use secure_types::SecureString;
use std::path::PathBuf;
use zeus_eth::alloy_primitives::Address;

pub const ACCOUNT_FILE: &str = "account.data";

/// Main user account struct
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
   #[serde(skip)]
   pub credentials: Credentials,

   pub wallets: Vec<Wallet>,

   /// The current selected wallet from the GUI
   pub current_wallet: Wallet,
}

impl Default for Account {
   fn default() -> Self {
      let wallet = Wallet::new_rng("Wallet 1".to_string());
      let wallets = vec![wallet.clone()];
      Self {
         credentials: Credentials::default(),
         wallets,
         current_wallet: wallet,
      }
   }
}

impl Account {
   pub fn new_wallet_from_key_or_phrase(
      &mut self,
      mut name: String,
      from_key: bool,
      key: SecureString,
   ) -> Result<(), anyhow::Error> {
      if !name.is_empty() {
         if self.wallet_name_exists(&name) {
            return Err(anyhow!("Wallet with name {} already exists", name));
         }
      } else {
         name = self.generate_wallet_name();
      }

      let wallet = if from_key {
         Wallet::new_from_key(name, String::new(), false, key)?
      } else {
         Wallet::new_from_mnemonic(name, String::new(), false, key)?
      };

      if self.wallet_address_exists(wallet.address()) {
         return Err(anyhow!("Wallet with address {} already exists", wallet.address()));
      }

      self.wallets.push(wallet);
      Ok(())
   }

   pub fn new_wallet_rng(&mut self, mut name: String) -> Result<(), anyhow::Error> {
      if !name.is_empty() {
         if self.wallet_name_exists(&name) {
            return Err(anyhow!("Wallet with name {} already exists", name));
         }
      } else {
         name = self.generate_wallet_name();
      }

      let wallet = Wallet::new_rng(name);
      self.wallets.push(wallet);
      Ok(())
   }

   pub fn generate_wallet_name(&self) -> String {
      let mut starter_number = 1;
      loop {
         let dummy_name = format!("Wallet {}", starter_number);
         if !self.wallet_name_exists(&dummy_name) {
            return dummy_name;
         }
         starter_number += 1;
      }
   }

   pub fn wallet_name_exists(&self, name: &str) -> bool {
      self.wallets.iter().any(|w| w.name == name)
   }

   pub fn wallet_address_exists(&self, address: Address) -> bool {
      self
         .wallets
         .iter()
         .any(|w| &w.key.borrow().address() == &address)
   }

   /// Encrypt the account and save it to a file
   pub fn encrypt_and_save(&self, dir: &PathBuf, argon: Argon2Params) -> Result<(), anyhow::Error> {
      // ! make sure we dont accidentally erased any of the wallet keys
      // ! this should actually never happen
      for wallet in self.wallets.iter() {
         if wallet.is_key_erased() {
            return Err(anyhow!("Wallet key is erased"));
         }
      }
      let account_data = serde_json::to_vec(self)?;
      let encrypted_data = encrypt_data(argon, account_data, self.credentials.clone())?;
      std::fs::write(dir, encrypted_data)?;
      Ok(())
   }

   pub fn decrypt(&self, dir: &PathBuf) -> Result<Vec<u8>, anyhow::Error> {
      let encrypted_data = std::fs::read(dir)?;
      let decrypted_data = decrypt_data(encrypted_data, self.credentials.clone())?;
      Ok(decrypted_data)
   }

   /// Do not return the decrypted data, just verify the credentials
   pub fn decrypt_zero(&self, dir: &PathBuf) -> Result<(), anyhow::Error> {
      let mut decrypted_data = self.decrypt(dir)?;
      decrypted_data.zeroize();
      Ok(())
   }

   /// Decrypt and load the account
   pub fn decrypt_and_load(&mut self, dir: &PathBuf) -> Result<(), anyhow::Error> {
      let mut decrypted_data = self.decrypt(dir)?;
      let account: Account = serde_json::from_slice(&decrypted_data)?;
      decrypted_data.zeroize();

      self.wallets = account.wallets;
      self.current_wallet = account.current_wallet;

      Ok(())
   }

   pub fn remove_wallet(&mut self, wallet: Wallet) {
      self.wallets.retain(|w| w != &wallet);
   }

   /// Get the current wallet address
   pub fn wallet_address(&self) -> Address {
      self.current_wallet.key.borrow().address()
   }

   /// Is an account exists at the data directory
   pub fn exists() -> Result<bool, anyhow::Error> {
      let dir = data_dir()?.join(ACCOUNT_FILE);
      Ok(dir.exists())
   }

   /// Account directory
   pub fn dir() -> Result<PathBuf, anyhow::Error> {
      Ok(data_dir()?.join(ACCOUNT_FILE))
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::fs;

   #[test]
   fn test_profile() {
      let wallet_1 = Wallet::new_rng("Wallet 1".to_string());
      let wallet_2 = Wallet::new_rng("Wallet 2".to_string());

      let original_key1 = wallet_1.key_string();
      let original_key2 = wallet_2.key_string();

      let credentials = Credentials::new(
         SecureString::from("test".to_string()),
         SecureString::from("password".to_string()),
         SecureString::from("password".to_string()),
      );

      let mut account = Account {
         credentials,
         wallets: vec![wallet_1.clone(), wallet_2],
         current_wallet: wallet_1,
      };

      let argon_2 = Argon2Params::very_fast();
      let dir = PathBuf::from("account_test.data");
      account
         .encrypt_and_save(&dir, argon_2)
         .expect("Profile Encryption failed");

      account
         .decrypt_and_load(&dir)
         .expect("Profile Recovery failed");

      let recovered_wallet_1 = account.wallets.get(0).unwrap();
      let recovered_wallet_2 = account.wallets.get(1).unwrap();

      assert_eq!(recovered_wallet_1.name, "Wallet 1");
      assert_eq!(recovered_wallet_2.name, "Wallet 2");

      let key_1 = recovered_wallet_1.key_string();
      let key_2 = recovered_wallet_2.key_string();

      assert_eq!(key_1, original_key1);
      assert_eq!(key_2, original_key2);

      fs::remove_file("account_test.data").unwrap();
   }
}
