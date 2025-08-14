use super::{Contact, wallet::*};
use crate::core::utils::data_dir;
use anyhow::anyhow;
use ncrypt_me::{Argon2, Credentials, EncryptedInfo, decrypt_data, encrypt_data};
use secure_types::{SecureBytes, SecureString};
use std::path::PathBuf;
use zeus_eth::alloy_primitives::Address;

pub const VAULT_FILE: &str = "vault.data";

/// User Vault
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Vault {
   /// Credentials used to decrypt the vault
   ///
   /// By default, the vault is encrypted with the same credentials
   /// we used to derive the HD wallet, this can be changed later through the GUI
   #[serde(skip)]
   credentials: Credentials,

   /// The HD Wallet which is deterministically derived from the credentials
   hd_wallet: SecureHDWallet,

   /// Imported wallets by the user
   ///
   /// Since these are not part of the HD wallet
   /// if we lose any backup of the vault, they are lost forever
   imported_wallets: Vec<Wallet>,

   #[serde(default)]
   pub contacts: Vec<Contact>,
}

impl Default for Vault {
   fn default() -> Self {
      let hd_wallet = SecureHDWallet::random();
      Self {
         credentials: Credentials::default(),
         hd_wallet,
         imported_wallets: Vec::new(),
         contacts: Vec::new(),
      }
   }
}

impl Vault {
   const MAX_CHARS: usize = 20;

   /// Return all the wallets in the vault in this order:
   ///
   /// - The master wallet
   /// - The children
   /// - The imported wallets
   pub fn all_wallets(&self) -> Vec<&Wallet> {
      let mut all_wallets = vec![&self.hd_wallet.master_wallet];
      all_wallets.extend(self.hd_wallet.children.iter());
      all_wallets.extend(self.imported_wallets.iter());
      all_wallets
   }

   /// Erase everything in the vault
   pub fn erase(&mut self) {
      self.credentials.erase();
      self.hd_wallet.erase();

      for wallet in self.imported_wallets.iter_mut() {
         wallet.key.erase();
      }
   }

   pub fn get_master_wallet(&self) -> Wallet {
      self.hd_wallet.master_wallet.clone()
   }

   pub fn get_hd_wallet(&self) -> SecureHDWallet {
      self.hd_wallet.clone()
   }

   pub fn set_credentials(&mut self, credentials: Credentials) {
      self.credentials = credentials;
   }

   pub fn set_hd_wallet(&mut self, hd_wallet: SecureHDWallet) {
      self.hd_wallet = hd_wallet;
   }

   pub fn set_imported_wallets(&mut self, imported_wallets: Vec<Wallet>) {
      self.imported_wallets = imported_wallets;
   }

   pub fn wallet_name_exists(&self, name: &str) -> bool {
      self.all_wallets().iter().any(|w| w.name == name)
   }

   pub fn wallet_address_exists(&self, address: Address) -> bool {
      self.all_wallets().iter().any(|w| w.key.address() == address)
   }

   fn generate_wallet_name(&self) -> String {
      let mut starter_number = 1;
      loop {
         let dummy_name = format!("Wallet {}", starter_number);
         if !self.wallet_name_exists(&dummy_name) {
            return dummy_name;
         }
         starter_number += 1;
      }
   }

   pub fn recover_hd_wallet(&mut self, name: String) -> Result<(), anyhow::Error> {
      self.credentials.is_valid()?;
      let hd_wallet = SecureHDWallet::new_from_credentials(name, self.credentials.clone())?;
      self.hd_wallet = hd_wallet;
      Ok(())
   }

   pub fn derive_child_wallet(&mut self, mut name: String) -> Result<Address, anyhow::Error> {
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

      let address = self.hd_wallet.derive_child(name)?;
      Ok(address)
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
      self.imported_wallets.push(wallet);
      Ok(())
   }

   /// Import a wallet from a private key or a seed phrase
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

      let wallet_address = wallet.address();

      if self.wallet_address_exists(wallet_address) {
         return Err(anyhow!(
            "Wallet with address {} already exists",
            wallet_address
         ));
      }

      self.imported_wallets.push(wallet);
      Ok(wallet_address)
   }

   /// Encrypt this account and return the encrypted data
   pub fn encrypt(&self, new_params: Option<Argon2>) -> Result<Vec<u8>, anyhow::Error> {
      // ! make sure we dont accidentally erased any of the wallet keys
      // ! this should actually never happen
      for wallet in self.all_wallets() {
         if wallet.is_key_erased() {
            return Err(anyhow!(
               "At least one Wallet key is erased, this is a bug"
            ));
         }
      }

      let vault_data = serde_json::to_vec(self)?;
      let secure_vault_data = SecureBytes::from_vec(vault_data)?;

      let argon_params = match new_params {
         Some(params) => params,
         None => self.encrypted_info()?.argon2,
      };

      let encrypted_data = encrypt_data(
         argon_params,
         secure_vault_data,
         self.credentials.clone(),
      )?;

      Ok(encrypted_data)
   }

   /// Save the encrypted account data to the given directory
   pub fn save(&self, dir: Option<PathBuf>, encrypted_data: Vec<u8>) -> Result<(), anyhow::Error> {
      let dir = match dir {
         Some(dir) => dir,
         None => Vault::dir()?,
      };
      std::fs::write(dir, encrypted_data)?;
      Ok(())
   }

   /// Decrypt this account and return the decrypted data
   pub fn decrypt(&self, dir: Option<PathBuf>) -> Result<SecureBytes, anyhow::Error> {
      let dir = match dir {
         Some(dir) => dir,
         None => Vault::dir()?,
      };

      let encrypted_data = std::fs::read(dir)?;
      let decrypted_data = decrypt_data(encrypted_data, self.credentials.clone())?;

      Ok(decrypted_data)
   }

   /// Load the account from the decrypted data
   pub fn load(&mut self, decrypted_data: SecureBytes) -> Result<(), anyhow::Error> {
      let vault: Vault = decrypted_data.slice_scope(|slice| serde_json::from_slice(slice))?;
      self.hd_wallet = vault.hd_wallet;
      self.imported_wallets = vault.imported_wallets;
      self.contacts = vault.contacts;
      Ok(())
   }

   /// Master wallet cannot be removed
   pub fn remove_wallet(&mut self, address: Address) {
      self.imported_wallets.retain(|w| w.address() != address);
      self.hd_wallet.children.retain(|w| w.address() != address);
   }

   pub fn encrypted_info(&self) -> Result<EncryptedInfo, anyhow::Error> {
      let data = std::fs::read(Vault::dir()?)?;
      let info = EncryptedInfo::from_encrypted_data(&data)?;
      Ok(info)
   }

   /// Vault directory
   pub fn dir() -> Result<PathBuf, anyhow::Error> {
      Ok(data_dir()?.join(VAULT_FILE))
   }

   /// Is a Vault exists at the data directory
   pub fn exists() -> Result<bool, anyhow::Error> {
      let dir = data_dir()?.join(VAULT_FILE);
      Ok(dir.exists())
   }
}
