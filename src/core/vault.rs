use super::{types::Contact, wallet::*};
use crate::core::context::{
   BalanceManagerHandle, DiscoveredWallets, PortfolioDB, TxDBHandle, data_dir,
};
use anyhow::anyhow;
use brotli::{BrotliCompress, BrotliDecompress, enc::BrotliEncoderParams};
use ncrypt_me::{
   Argon2, Credentials, EncryptedInfo, decrypt::decrypt_data_unsecured, encrypt::encrypt_data_ref,
};
use secure_types::{SecureString, Zeroize};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::PathBuf;
use zeus_eth::alloy_primitives::Address;
use zeus_railgun::RailgunAddress;
use zeus_wallet::{SecureHDWallet, Wallet, derive_seed};

pub const VAULT_FILE: &str = "vault.data";

/// Plaintext vault payload encoding (first byte of decrypted data).
///
/// - `0` raw JSON
/// - `1` brotli-compressed JSON
///
/// New saves always write version `1`. Load also accepts a legacy unversioned
/// blob that starts with `{` (raw JSON from before this envelope existed).
const VAULT_PAYLOAD_RAW_JSON: u8 = 0;
const VAULT_PAYLOAD_BROTLI: u8 = 1;

/// Mid-range quality: good ratio on JSON without max-level CPU cost on save.
const VAULT_BROTLI_QUALITY: i32 = 5;

fn brotli_compress(input: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   let mut params = BrotliEncoderParams::default();
   params.quality = VAULT_BROTLI_QUALITY;
   let mut out = Vec::new();
   BrotliCompress(&mut Cursor::new(input), &mut out, &params)
      .map_err(|e| anyhow!("brotli compress vault: {e}"))?;
   Ok(out)
}

fn brotli_decompress(input: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   let mut out = Vec::new();
   BrotliDecompress(&mut &input[..], &mut out)
      .map_err(|e| anyhow!("brotli decompress vault: {e}"))?;
   Ok(out)
}

/// Build the encrypted plaintext: `[version][payload]`.
fn encode_vault_payload(json: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   let compressed = brotli_compress(json)?;
   let mut out = Vec::with_capacity(1 + compressed.len());
   out.push(VAULT_PAYLOAD_BROTLI);
   out.extend_from_slice(&compressed);
   Ok(out)
}

/// Decode decrypted bytes into vault JSON bytes.
fn decode_vault_payload(data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   if data.is_empty() {
      return Err(anyhow!("vault payload is empty"));
   }

   // Legacy: unversioned raw JSON (created after vault-in-JSON, before envelope)
   if data[0] == b'{' {
      return Ok(data.to_vec());
   }

   let version = data[0];
   let payload = &data[1..];

   match version {
      VAULT_PAYLOAD_RAW_JSON => Ok(payload.to_vec()),
      VAULT_PAYLOAD_BROTLI => brotli_decompress(payload),
      other => Err(anyhow!("unknown vault payload version: {other}")),
   }
}

/// User Vault
#[derive(Clone, Serialize, Deserialize)]
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

   /// Public balances (persisted with the vault; saved on vault encrypt)
   #[serde(default)]
   pub balance_manager: BalanceManagerHandle,

   /// Portfolio values / token lists per wallet
   #[serde(default)]
   pub portfolio_db: PortfolioDB,

   /// Transaction history
   #[serde(default)]
   pub tx_db: TxDBHandle,

   /// HD child discovery state
   #[serde(default)]
   pub discovered_wallets: DiscoveredWallets,
}

impl Default for Vault {
   fn default() -> Self {
      let hd_wallet = SecureHDWallet::random();
      Self {
         credentials: Credentials::default(),
         hd_wallet,
         imported_wallets: Vec::new(),
         contacts: Vec::new(),
         balance_manager: BalanceManagerHandle::default(),
         portfolio_db: PortfolioDB::default(),
         tx_db: TxDBHandle::new(),
         discovered_wallets: DiscoveredWallets::new(),
      }
   }
}

impl Vault {
   const MAX_CHARS: usize = 20;

   pub fn name_max_chars(&self) -> usize {
      Self::MAX_CHARS
   }

   pub fn credentials(&self) -> &Credentials {
      &self.credentials
   }

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

   pub fn clone_all_wallets(&self) -> Vec<Wallet> {
      let mut all_wallets = vec![self.hd_wallet.master_wallet.clone()];
      all_wallets.extend(self.hd_wallet.children.iter().map(|w| w.clone()));
      all_wallets.extend(self.imported_wallets.iter().map(|w| w.clone()));
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

   pub fn master_wallet_address(&self) -> Address {
      self.hd_wallet.master_wallet.address()
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

   /// Copy balance / portfolio / tx / discovery state from another vault.
   pub fn persisted_state_from(&mut self, other: &Vault) {
      self.balance_manager = other.balance_manager.clone();
      self.tx_db = other.tx_db.clone();
      self.portfolio_db = other.portfolio_db.clone();
      self.discovered_wallets = other.discovered_wallets.clone();
   }

   pub fn wallet_name_exists(&self, name: &str) -> bool {
      self.all_wallets().iter().any(|w| w.name == name)
   }

   pub fn wallet_address_exists(&self, address: Address) -> bool {
      self.all_wallets().iter().any(|w| w.key.address() == address)
   }

   pub fn wallet_with_zk_address_exists(&self, zk_address: &RailgunAddress) -> bool {
      for wallet in self.all_wallets() {
         if let Ok(seed) = wallet.seed() {
            let railgun_address = RailgunAddress::new(&seed, 0, None).unwrap();
            if railgun_address.address == zk_address.address {
               return true;
            }
         }
      }

      false
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

   pub fn remove_child(&mut self, address: Address) {
      self.hd_wallet.children.retain(|w| w.address() != address);
   }

   pub fn recover_hd_wallet(&mut self, name: String) -> Result<(), anyhow::Error> {
      self.credentials.is_valid()?;

      let m_cost = if cfg!(feature = "dev") {
         DEV_M_COST
      } else {
         M_COST
      };

      let t_cost = if cfg!(feature = "dev") {
         DEV_T_COST
      } else {
         T_COST
      };

      let p_cost = if cfg!(feature = "dev") {
         DEV_P_COST
      } else {
         P_COST
      };

      let username = &self.credentials.username;
      let password = &self.credentials.password;

      let name = if name.is_empty() { None } else { Some(name) };

      let seed = derive_seed(username, password, m_cost, t_cost, p_cost)?;
      let hd_wallet = SecureHDWallet::new_from_seed(name, seed);
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

   pub fn derive_child_wallet_at_mut(
      &mut self,
      mut name: String,
      index: u32,
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

      let wallet = self.hd_wallet.derive_child_at_mut(name, index)?;
      Ok(wallet.address())
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
         Wallet::new_from_key_str(name, key)?
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

      let mut json = serde_json::to_vec(self)?;
      let mut vault_data = match encode_vault_payload(&json) {
         Ok(data) => data,
         Err(e) => {
            json.zeroize();
            return Err(e);
         }
      };

      json.zeroize();

      let encrypted_info = match self.encrypted_info() {
         Ok(info) => info,
         Err(e) => {
            vault_data.zeroize();
            return Err(anyhow!(
               "EncryptedInfo is missing, corrupted vault?: {:?}",
               e
            ));
         }
      };

      let argon_params = match new_params {
         Some(params) => params,
         None => encrypted_info.argon2,
      };

      let encrypted_data = match encrypt_data_ref(
         argon_params,
         &vault_data,
         self.credentials.clone(),
      ) {
         Ok(data) => data,
         Err(e) => {
            vault_data.zeroize();
            return Err(anyhow!("Failed to encrypt vault data: {:?}", e));
         }
      };

      vault_data.zeroize();

      Ok(encrypted_data)
   }

   /// Save the encrypted Vault data to the given directory
   pub fn save(&self, dir: Option<PathBuf>, encrypted_data: Vec<u8>) -> Result<(), anyhow::Error> {
      let dir = match dir {
         Some(dir) => dir,
         None => Vault::dir()?,
      };
      std::fs::write(dir, encrypted_data)?;
      Ok(())
   }

   /// Decrypt this Vault and return the decrypted data
   pub fn decrypt(&self, dir: Option<PathBuf>) -> Result<Vec<u8>, anyhow::Error> {
      let dir = match dir {
         Some(dir) => dir,
         None => Vault::dir()?,
      };

      let encrypted_data = std::fs::read(dir)?;
      let decrypted_data = decrypt_data_unsecured(encrypted_data, self.credentials.clone())?;

      Ok(decrypted_data)
   }

   /// Load the vault from the decrypted data
   pub fn load(&mut self, mut decrypted_data: Vec<u8>) -> Result<(), anyhow::Error> {
      let mut json = match decode_vault_payload(&decrypted_data) {
         Ok(json) => json,
         Err(e) => {
            decrypted_data.zeroize();
            return Err(e);
         }
      };

      decrypted_data.zeroize();

      let vault: Vault = match serde_json::from_slice(&json) {
         Ok(vault) => vault,
         Err(e) => {
            json.zeroize();
            return Err(anyhow!("Failed to parse vault data: {:?}", e));
         }
      };

      json.zeroize();

      self.hd_wallet = vault.hd_wallet;
      self.imported_wallets = vault.imported_wallets;
      self.contacts = vault.contacts;
      self.balance_manager = vault.balance_manager;
      self.portfolio_db = vault.portfolio_db;
      self.tx_db = vault.tx_db;
      self.discovered_wallets = vault.discovered_wallets;
      Ok(())
   }

   /// Remove the wallet with the given address
   ///
   /// Master wallet cannot be removed
   pub fn remove_wallet(&mut self, address: Address) {
      self.imported_wallets.retain(|w| w.address() != address);
      self.hd_wallet.children.retain(|w| w.address() != address);
   }

   /// Return a mutable reference iter to all the wallets in the vault
   pub fn all_wallets_mut(&mut self) -> impl Iterator<Item = &mut Wallet> {
      std::iter::once(&mut self.hd_wallet.master_wallet)
         .chain(self.hd_wallet.children.iter_mut())
         .chain(self.imported_wallets.iter_mut())
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

#[cfg(test)]
mod tests {
   use super::*;
   use ncrypt_me::Credentials;
   use secure_types::SecureString;
   use zeus_wallet::SecureHDWallet;

   fn sample_vault() -> Vault {
      let mut vault = Vault::default();
      vault.set_credentials(Credentials::new(
         SecureString::from("user"),
         SecureString::from("pass"),
         SecureString::from("pass"),
      ));

      let hd_wallet = SecureHDWallet::random();
      vault.set_hd_wallet(hd_wallet);

      vault
   }

   #[test]
   fn vault_json_roundtrip() {
      let vault = sample_vault();
      let json = serde_json::to_vec(&vault).expect("serialize vault");
      let loaded: Vault = serde_json::from_slice(&json).expect("deserialize vault");

      assert_eq!(
         loaded.master_wallet_address(),
         vault.master_wallet_address()
      );
      assert_eq!(loaded.contacts.len(), vault.contacts.len());
   }

   #[test]
   fn vault_payload_brotli_roundtrip() {
      let vault = sample_vault();
      let json = serde_json::to_vec(&vault).unwrap();
      let encoded = encode_vault_payload(&json).unwrap();
      assert_eq!(encoded[0], VAULT_PAYLOAD_BROTLI);

      let decoded = decode_vault_payload(&encoded).unwrap();
      let loaded: Vault = serde_json::from_slice(&decoded).unwrap();
      assert_eq!(
         loaded.master_wallet_address(),
         vault.master_wallet_address()
      );
   }

   #[test]
   fn vault_payload_raw_json_version() {
      let vault = sample_vault();
      let json = serde_json::to_vec(&vault).unwrap();
      let mut encoded = Vec::with_capacity(1 + json.len());
      encoded.push(VAULT_PAYLOAD_RAW_JSON);
      encoded.extend_from_slice(&json);

      let decoded = decode_vault_payload(&encoded).unwrap();
      let loaded: Vault = serde_json::from_slice(&decoded).unwrap();
      assert_eq!(
         loaded.master_wallet_address(),
         vault.master_wallet_address()
      );
   }

   #[test]
   fn vault_payload_legacy_unversioned_json() {
      let vault = sample_vault();
      let json = serde_json::to_vec(&vault).unwrap();
      assert_eq!(json[0], b'{');

      let decoded = decode_vault_payload(&json).unwrap();
      let loaded: Vault = serde_json::from_slice(&decoded).unwrap();
      assert_eq!(
         loaded.master_wallet_address(),
         vault.master_wallet_address()
      );
   }
}
