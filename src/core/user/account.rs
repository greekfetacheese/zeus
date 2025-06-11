use super::wallet::*;
use crate::core::utils::data_dir;
use anyhow::anyhow;
use ncrypt_me::{Argon2Params, Credentials, EncryptedInfo, decrypt_data, encrypt_data};
use secure_types::{SecureBytes, SecureString};
use std::path::PathBuf;
use zeus_eth::alloy_primitives::Address;

pub const ACCOUNT_FILE: &str = "account.data";

/// Main user account struct
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
   #[serde(skip)]
   credentials: Credentials,

   wallets: Vec<Wallet>,

   /// The current selected wallet from the GUI
   pub current_wallet: WalletInfo,
}

impl Default for Account {
   fn default() -> Self {
      let wallet = Wallet::new_rng("Wallet 1".to_string());
      let wallets = vec![wallet.clone()];
      Self {
         credentials: Credentials::default(),
         wallets,
         current_wallet: wallet.info.clone(),
      }
   }
}

impl Account {
   const MAX_CHARS: usize = 20;

   pub fn wallets(&self) -> &Vec<Wallet> {
      &self.wallets
   }

   pub fn is_default(&self) -> bool {
      self.credentials.is_valid().is_err()
   }

   pub fn credentials_mut(&mut self) -> &mut Credentials {
      &mut self.credentials
   }

   pub fn wallets_mut(&mut self) -> &mut Vec<Wallet> {
      &mut self.wallets
   }

   pub fn set_credentials(&mut self, credentials: Credentials) {
      self.credentials = credentials;
   }

   pub fn set_wallets(&mut self, wallets: Vec<Wallet>) {
      self.wallets = wallets;
   }

   pub fn set_current_wallet(&mut self, current_wallet: WalletInfo) {
      self.current_wallet = current_wallet;
   }

   pub fn new_wallet_from_key_or_phrase(
      &mut self,
      mut name: String,
      from_key: bool,
      key: SecureString,
   ) -> Result<Address, anyhow::Error> {
      if !name.is_empty() {
         if self.wallet_name_exists(&name) {
            return Err(anyhow!(
               "Wallet with name {} already exists",
               name
            ));
         }

         if name.len() > Self::MAX_CHARS {
            return Err(anyhow!(
               "Wallet name cannot be longer than {} characters",
               Self::MAX_CHARS
            ));
         }
      } else {
         name = self.generate_wallet_name();
      }

      let wallet = if from_key {
         Wallet::new_from_key(name, key)?
      } else {
         Wallet::new_from_mnemonic(name, key)?
      };

      if self.wallet_address_exists(wallet.info.address) {
         return Err(anyhow!(
            "Wallet with address {} already exists",
            wallet.info.address
         ));
      }

      let wallet_address = wallet.info.address;
      self.wallets.push(wallet);
      Ok(wallet_address)
   }

   pub fn new_wallet_rng(&mut self, mut name: String) -> Result<(), anyhow::Error> {
      if !name.is_empty() {
         if self.wallet_name_exists(&name) {
            return Err(anyhow!(
               "Wallet with name {} already exists",
               name
            ));
         }

         if name.len() > Self::MAX_CHARS {
            return Err(anyhow!(
               "Wallet name cannot be longer than {} characters",
               Self::MAX_CHARS
            ));
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
      self.wallets.iter().any(|w| w.info.name == name)
   }

   pub fn wallet_address_exists(&self, address: Address) -> bool {
      self
         .wallets
         .iter()
         .any(|w| &w.key.address() == &address)
   }

   /// Encrypt this account and return the encrypted data
   pub fn encrypt(&self, new_params: Option<Argon2Params>) -> Result<Vec<u8>, anyhow::Error> {
      // ! make sure we dont accidentally erased any of the wallet keys
      // ! this should actually never happen
      for wallet in self.wallets.iter() {
         if wallet.is_key_erased() {
            return Err(anyhow!("Wallet key is erased"));
         }
      }
      let account_data = serde_json::to_vec(self)?;
      let argon_params = match new_params {
         Some(params) => params,
         None => self.encrypted_info()?.argon2_params,
      };
      let encrypted_data = encrypt_data(
         argon_params,
         account_data,
         self.credentials.clone(),
      )?;
      Ok(encrypted_data)
   }

   /// Save the encrypted account data to the given directory
   pub fn save(&self, dir: Option<PathBuf>, encrypted_data: Vec<u8>) -> Result<(), anyhow::Error> {
      let dir = match dir {
         Some(dir) => dir,
         None => Account::dir()?,
      };
      std::fs::write(dir, encrypted_data)?;
      Ok(())
   }

   /// Decrypt this account and return the decrypted data
   pub fn decrypt(&self, dir: Option<PathBuf>) -> Result<SecureBytes, anyhow::Error> {
      let dir = match dir {
         Some(dir) => dir,
         None => Account::dir()?,
      };
      let encrypted_data = std::fs::read(dir)?;
      let decrypted_data = decrypt_data(encrypted_data, self.credentials.clone())?;
      Ok(decrypted_data)
   }

   /// Load the account from the decrypted data
   pub fn load(&mut self, decrypted_data: SecureBytes) -> Result<(), anyhow::Error> {
      let account: Account = decrypted_data.slice_scope(|slice| {
         serde_json::from_slice(slice)
      })?;
      self.wallets = account.wallets;
      self.current_wallet = account.current_wallet;
      Ok(())
   }

   pub fn remove_wallet(&mut self, wallet: &WalletInfo) {
      self.wallets.retain(|w| &w.info != wallet);
   }

   /// Get the current wallet address
   pub fn wallet_address(&self) -> Address {
      self.current_wallet.address
   }

   pub fn encrypted_info(&self) -> Result<EncryptedInfo, anyhow::Error> {
      let data = std::fs::read(Account::dir()?)?;
      let info = EncryptedInfo::from_encrypted_data(&data)?;
      Ok(info)
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

      let argon_params = Argon2Params::very_fast();
      let mut account = Account {
         credentials,
         wallets: vec![wallet_1.clone(), wallet_2],
         current_wallet: wallet_1.info.clone(),
      };

      let dir = PathBuf::from("account_test.data");

      let encrypted_data = account
         .encrypt(Some(argon_params))
         .expect("Profile Encryption failed");

      account
         .save(Some(dir.clone()), encrypted_data)
         .expect("Profile Encryption failed");

      let decrypted_data = account.decrypt(Some(dir)).expect("Profile Recovery failed");

      account
         .load(decrypted_data)
         .expect("Profile Recovery failed");

      let recovered_wallet_1 = account.wallets.get(0).unwrap();
      let recovered_wallet_2 = account.wallets.get(1).unwrap();

      assert_eq!(recovered_wallet_1.info.name, "Wallet 1");
      assert_eq!(recovered_wallet_2.info.name, "Wallet 2");

      let key_1 = recovered_wallet_1.key_string();
      let key_2 = recovered_wallet_2.key_string();

      let key_1_string = key_1.str_scope(|key| key.to_string());
      let key_2_string = key_2.str_scope(|key| key.to_string());
      let original_key1_string = original_key1.str_scope(|key| key.to_string());
      let original_key2_string = original_key2.str_scope(|key| key.to_string());

      assert_eq!(key_1_string, original_key1_string);
      assert_eq!(key_2_string, original_key2_string);

      fs::remove_file("account_test.data").unwrap();
   }
}
