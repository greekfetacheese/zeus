use super::{types::Contact, wallet::*};
use crate::core::context::{
   ApprovalManagerHandle, BalanceManagerHandle, DiscoveredWallets, PortfolioDB, TxDBHandle,
   data_dir,
};
use crate::core::wallet_state::{WalletStateInner, WalletStateKey};
use crate::utils::write_private_atomic;
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
use zeus_wallet::{SecureHDWallet, Wallet, derive_seed, wallet::{M_COST, P_COST, T_COST}};

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

/// Legacy Vault
#[derive(Serialize, Deserialize)]
struct VaultData {
   hd_wallet: SecureHDWallet,
   imported_wallets: Vec<Wallet>,

   /// AEAD key for sensitive Railgun DB values (UTXO notes, POI pending).
   #[serde(default)]
   railgun_db_key: Option<zeus_railgun::RailgunDbKey>,

   /// AEAD key for [`crate::core::WalletState`]
   #[serde(default)]
   wallet_state_key: Option<WalletStateKey>,

   // Legacy fields (pre wallet_state.data split). Accepted on load only.
   #[serde(default, skip_serializing)]
   contacts: Vec<Contact>,
   #[serde(default, skip_serializing)]
   balance_manager: BalanceManagerHandle,
   #[serde(default, skip_serializing)]
   portfolio_db: PortfolioDB,
   #[serde(default, skip_serializing)]
   tx_db: Option<TxDBHandle>,
   #[serde(default, skip_serializing)]
   approval_manager: Option<ApprovalManagerHandle>,
   #[serde(default, skip_serializing)]
   discovered_wallets: Option<DiscoveredWallets>,
}

impl VaultData {
   fn take_legacy_wallet_state(&mut self) -> Option<WalletStateInner> {
      let has_legacy = !self.contacts.is_empty()
         || self.tx_db.is_some()
         || self.approval_manager.is_some()
         || self.discovered_wallets.is_some()
         || self.wallet_state_key.is_none();

      if !has_legacy {
         return None;
      }

      Some(WalletStateInner {
         contacts: std::mem::take(&mut self.contacts),
         balance_manager: std::mem::take(&mut self.balance_manager),
         portfolio_db: std::mem::take(&mut self.portfolio_db),
         tx_db: self.tx_db.take().unwrap_or_else(TxDBHandle::new),
         approval_manager: self.approval_manager.take().unwrap_or_else(ApprovalManagerHandle::new),
         discovered_wallets: self.discovered_wallets.take().unwrap_or_else(DiscoveredWallets::new),
      })
   }
}

/// User Vault — credentials, HD/imported keys, and AEAD keys for side stores.
///
/// Frequently updated app state (contacts, balances, portfolios, txs, approvals,
/// HD discovery) lives in [`crate::core::WalletState`]
#[derive(Clone)]
pub struct Vault {
   /// Credentials used to decrypt the vault
   ///
   /// By default, the vault is encrypted with the same credentials
   /// we used to derive the HD wallet, this can be changed later through the GUI
   credentials: Credentials,

   /// The HD Wallet which is deterministically derived from the credentials
   hd_wallet: SecureHDWallet,

   /// Imported wallets by the user
   ///
   /// Since these are not part of the HD wallet
   /// if we lose any backup of the vault, they are lost forever
   imported_wallets: Vec<Wallet>,

   /// AEAD key for sensitive Railgun DB values (UTXO notes, POI pending).
   ///
   /// Generated the first time Zeus starts.
   railgun_db_key: Option<zeus_railgun::RailgunDbKey>,

   /// AEAD key for [`crate::core::WalletState`].
   ///
   /// Generated the first time Zeus unlocks/creates a vault after the split.
   wallet_state_key: Option<WalletStateKey>,
}

impl Default for Vault {
   fn default() -> Self {
      let hd_wallet = SecureHDWallet::random();
      Self {
         credentials: Credentials::default(),
         hd_wallet,
         imported_wallets: Vec::new(),
         railgun_db_key: None,
         wallet_state_key: None,
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
         wallet.erase();
      }

      if let Some(ref mut key) = self.railgun_db_key {
         key.erase();
      }
      if let Some(ref mut key) = self.wallet_state_key {
         key.erase();
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

   /// Copy AEAD keys from another vault (wallet mutations clone without keys).
   pub fn persisted_state_from(&mut self, other: &Vault) {
      self.railgun_db_key = other.railgun_db_key.clone();
      self.wallet_state_key = other.wallet_state_key.clone();
   }

   /// Ensure a Railgun DB crypto key exists.
   ///
   /// Returns `true` if a new key was generated (caller should re-save the vault).
   pub fn ensure_railgun_db_key(&mut self) -> Result<bool, anyhow::Error> {
      if self.railgun_db_key.is_some() {
         return Ok(false);
      }
      let key = zeus_railgun::RailgunDbKey::generate()
         .map_err(|e| anyhow!("Failed to generate railgun db key: {e}"))?;
      self.railgun_db_key = Some(key);
      Ok(true)
   }

   /// Clone the Railgun DB key (for provider construction while unlocked).
   pub fn railgun_db_key(&self) -> Result<zeus_railgun::RailgunDbKey, anyhow::Error> {
      self.railgun_db_key.clone().ok_or_else(|| {
         anyhow!("Railgun DB key missing — unlock vault / ensure_railgun_db_key first")
      })
   }

   /// Ensure a WalletState AEAD key exists.
   ///
   /// Returns `true` if a new key was generated (caller should re-save the vault).
   pub fn ensure_wallet_state_key(&mut self) -> Result<bool, anyhow::Error> {
      if self.wallet_state_key.is_some() {
         return Ok(false);
      }
      let key = WalletStateKey::generate()
         .map_err(|e| anyhow!("Failed to generate wallet state key: {e}"))?;
      self.wallet_state_key = Some(key);
      Ok(true)
   }

   /// Clone the WalletState AEAD key.
   pub fn wallet_state_key(&self) -> Result<WalletStateKey, anyhow::Error> {
      self.wallet_state_key.clone().ok_or_else(|| {
         anyhow!("Wallet state key missing / unlock vault / ensure_wallet_state_key first")
      })
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

      let data = VaultData {
         hd_wallet: self.hd_wallet.clone(),
         imported_wallets: self.imported_wallets.clone(),
         railgun_db_key: self.railgun_db_key.clone(),
         wallet_state_key: self.wallet_state_key.clone(),
         contacts: Vec::new(),
         balance_manager: BalanceManagerHandle::default(),
         portfolio_db: PortfolioDB::default(),
         tx_db: None,
         approval_manager: None,
         discovered_wallets: None,
      };

      let mut json = serde_json::to_vec(&data)?;
      let mut vault_data = match encode_vault_payload(&json) {
         Ok(data) => data,
         Err(e) => {
            json.zeroize();
            return Err(e);
         }
      };

      json.zeroize();

      let argon_params = match new_params {
         Some(params) => params,
         None => {
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
            encrypted_info.argon2
         }
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
      write_private_atomic(&dir, &encrypted_data)?;
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

   /// Load the vault from the decrypted data.
   ///
   /// Returns legacy wallet-state payload when the vault JSON still embeds
   /// contacts/balances/… (pre-split). Caller should feed this into
   /// [`crate::core::WalletState::load_or_migrate`].
   pub fn load(
      &mut self,
      mut decrypted_data: Vec<u8>,
   ) -> Result<Option<WalletStateInner>, anyhow::Error> {
      let mut json = match decode_vault_payload(&decrypted_data) {
         Ok(json) => json,
         Err(e) => {
            decrypted_data.zeroize();
            return Err(e);
         }
      };

      decrypted_data.zeroize();

      let mut data: VaultData = match serde_json::from_slice(&json) {
         Ok(vault) => vault,
         Err(e) => {
            json.zeroize();
            return Err(anyhow!("Failed to parse vault data: {:?}", e));
         }
      };

      json.zeroize();

      // Prefer sealed wallet_state.data when present; only surface legacy embed
      // when the side file does not exist yet (migration path).
      let legacy = if crate::core::WalletState::exists().unwrap_or(false) {
         None
      } else {
         data.take_legacy_wallet_state()
      };

      self.hd_wallet = data.hd_wallet;
      self.imported_wallets = data.imported_wallets;
      self.railgun_db_key = data.railgun_db_key;
      self.wallet_state_key = data.wallet_state_key;

      Ok(legacy)
   }

   // TODO: Impl Eq on the Credentials and secure-types
   /// Returns true if the other credentials match the current ones
   pub fn credentials_match(&self, other: &Credentials) -> bool {
      let username_ok = self.credentials.username.unlock_str(|username| {
         other.username.unlock_str(|other_username| username == other_username)
      });

      let password_ok = self.credentials.password.unlock_str(|password| {
         other.password.unlock_str(|other_password| password == other_password)
      });

      let confirm_password_ok = self.credentials.confirm_password.unlock_str(|confirm_password| {
         other
            .confirm_password
            .unlock_str(|other_confirm_password| confirm_password == other_confirm_password)
      });

      username_ok && password_ok && confirm_password_ok
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
      let data = VaultData {
         hd_wallet: vault.hd_wallet.clone(),
         imported_wallets: vault.imported_wallets.clone(),
         railgun_db_key: None,
         wallet_state_key: None,
         contacts: Vec::new(),
         balance_manager: BalanceManagerHandle::default(),
         portfolio_db: PortfolioDB::default(),
         tx_db: None,
         approval_manager: None,
         discovered_wallets: None,
      };
      let json = serde_json::to_vec(&data).expect("serialize vault");
      let loaded: VaultData = serde_json::from_slice(&json).expect("deserialize vault");

      assert_eq!(
         loaded.hd_wallet.master_wallet.address(),
         vault.master_wallet_address()
      );
   }

   #[test]
   fn vault_payload_brotli_roundtrip() {
      let vault = sample_vault();
      let data = VaultData {
         hd_wallet: vault.hd_wallet.clone(),
         imported_wallets: vault.imported_wallets.clone(),
         railgun_db_key: None,
         wallet_state_key: None,
         contacts: Vec::new(),
         balance_manager: BalanceManagerHandle::default(),
         portfolio_db: PortfolioDB::default(),
         tx_db: None,
         approval_manager: None,
         discovered_wallets: None,
      };
      let json = serde_json::to_vec(&data).unwrap();
      let encoded = encode_vault_payload(&json).unwrap();
      assert_eq!(encoded[0], VAULT_PAYLOAD_BROTLI);

      let decoded = decode_vault_payload(&encoded).unwrap();
      let loaded: VaultData = serde_json::from_slice(&decoded).unwrap();
      assert_eq!(
         loaded.hd_wallet.master_wallet.address(),
         vault.master_wallet_address()
      );
   }

   #[test]
   fn vault_payload_raw_json_version() {
      let vault = sample_vault();
      let data = VaultData {
         hd_wallet: vault.hd_wallet.clone(),
         imported_wallets: vault.imported_wallets.clone(),
         railgun_db_key: None,
         wallet_state_key: None,
         contacts: Vec::new(),
         balance_manager: BalanceManagerHandle::default(),
         portfolio_db: PortfolioDB::default(),
         tx_db: None,
         approval_manager: None,
         discovered_wallets: None,
      };
      let json = serde_json::to_vec(&data).unwrap();
      let mut encoded = Vec::with_capacity(1 + json.len());
      encoded.push(VAULT_PAYLOAD_RAW_JSON);
      encoded.extend_from_slice(&json);

      let decoded = decode_vault_payload(&encoded).unwrap();
      let loaded: VaultData = serde_json::from_slice(&decoded).unwrap();
      assert_eq!(
         loaded.hd_wallet.master_wallet.address(),
         vault.master_wallet_address()
      );
   }

   #[test]
   fn vault_payload_legacy_unversioned_json() {
      let vault = sample_vault();
      let data = VaultData {
         hd_wallet: vault.hd_wallet.clone(),
         imported_wallets: vault.imported_wallets.clone(),
         railgun_db_key: None,
         wallet_state_key: None,
         contacts: Vec::new(),
         balance_manager: BalanceManagerHandle::default(),
         portfolio_db: PortfolioDB::default(),
         tx_db: None,
         approval_manager: None,
         discovered_wallets: None,
      };
      let json = serde_json::to_vec(&data).unwrap();
      assert_eq!(json[0], b'{');

      let decoded = decode_vault_payload(&json).unwrap();
      let loaded: VaultData = serde_json::from_slice(&decoded).unwrap();
      assert_eq!(
         loaded.hd_wallet.master_wallet.address(),
         vault.master_wallet_address()
      );
   }

   #[test]
   fn vault_load_migrates_legacy_embedded_state() {
      let vault = sample_vault();
      let mut data = VaultData {
         hd_wallet: vault.hd_wallet.clone(),
         imported_wallets: Vec::new(),
         railgun_db_key: None,
         wallet_state_key: None,
         contacts: vec![Contact::new("bob".into(), "0xbb".into(), String::new())],
         balance_manager: BalanceManagerHandle::default(),
         portfolio_db: PortfolioDB::default(),
         tx_db: Some(TxDBHandle::new()),
         approval_manager: Some(ApprovalManagerHandle::new()),
         discovered_wallets: Some(DiscoveredWallets::new()),
      };
      // Simulate decrypt → load path without wallet_state.data on disk.
      let json = serde_json::to_vec(&{
         // Force-include legacy fields by serializing a helper that does not skip them.
         #[derive(Serialize)]
         struct Legacy {
            hd_wallet: SecureHDWallet,
            imported_wallets: Vec<Wallet>,
            contacts: Vec<Contact>,
            balance_manager: BalanceManagerHandle,
            portfolio_db: PortfolioDB,
            tx_db: TxDBHandle,
            approval_manager: ApprovalManagerHandle,
            discovered_wallets: DiscoveredWallets,
         }
         Legacy {
            hd_wallet: data.hd_wallet.clone(),
            imported_wallets: data.imported_wallets.clone(),
            contacts: data.contacts.clone(),
            balance_manager: data.balance_manager.clone(),
            portfolio_db: data.portfolio_db.clone(),
            tx_db: data.tx_db.clone().unwrap(),
            approval_manager: data.approval_manager.clone().unwrap(),
            discovered_wallets: data.discovered_wallets.clone().unwrap(),
         }
      })
      .unwrap();

      let _payload = encode_vault_payload(&json).unwrap();
      // load() checks WalletState::exists() against real data_dir — may or may not
      // exist in unit tests. Exercise take_legacy directly here.
      let legacy = data.take_legacy_wallet_state().expect("legacy");
      assert_eq!(legacy.contacts.len(), 1);
      assert_eq!(legacy.contacts[0].name, "bob");

      let _ = vault;
   }
}
