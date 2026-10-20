use super::super::{
   AddressBookHandle, ClientManager, CurrencyDB, NftDB, PoolManagerHandle, ZeusCtx,
   price_manager::PriceManagerHandle, tx::TxDBHandle,
};
use super::{bundler_url_dir, pool_data_dir, theme_kind_dir};
use crate::core::{
   Vault, WalletInfo, WalletState,
   persisted::{self, PersistedFile},
   types::*,
};
use crate::utils::TimeStamp;
use anyhow::anyhow;
use egui_elements::theme::ThemeKind;
use ncrypt_me::Argon2;
use std::collections::HashMap;

pub fn load_theme_kind() -> Result<ThemeKind, anyhow::Error> {
   let dir = theme_kind_dir()?;
   let theme_kind_str = std::fs::read_to_string(dir)?;
   let theme_kind = serde_json::from_str(&theme_kind_str)?;
   Ok(theme_kind)
}

/// `write_private` only tightens mode on the next save. Existing `0644` files
/// from older builds stay world-readable until then — fix them at startup.
pub(super) fn tighten_existing_secret_files() {
   let paths = [
      Vault::dir().ok(),
      WalletState::dir().ok(),
      TxDBHandle::dir().ok(),
      CurrencyDB::dir().ok(),
      AddressBookHandle::dir().ok(),
      pool_data_dir().ok(),
      ClientManager::dir().ok(),
      bundler_url_dir().ok(),
      PriceManagerHandle::dir().ok(),
      persisted::file_path(PersistedFile::Security).ok(),
      persisted::file_path(PersistedFile::Connector).ok(),
   ];

   for path in paths.into_iter().flatten() {
      if !path.exists() {
         continue;
      }
      if let Err(e) = crate::utils::restrict_file_to_owner(&path) {
         tracing::warn!("Failed to restrict {}: {e}", path.display());
      }
   }
}

impl ZeusCtx {
   /// Encrypt and save the vault (Argon2 — wallets + AEAD keys only).
   ///
   /// If `new_vault` is None, the current vault will be encrypted
   ///
   /// If `new_params` is None, the current [Argon2] params will be used
   pub fn encrypt_and_save_vault(
      &self,
      new_vault: Option<Vault>,
      new_params: Option<Argon2>,
   ) -> Result<(), anyhow::Error> {
      if self.save_vault_in_progress() {
         return Err(anyhow!(
            "Saving vault in progress, try again later"
         ));
      }

      self.write(|ctx| ctx.save_vault_in_progress = true);

      // Wallet mutations pass `new_vault` that may lack AEAD keys — merge from live.
      // Snapshot under a short vault lock; Argon2 encrypt must not hold it.
      let vault = match new_vault {
         Some(mut vault) => {
            self.read_vault(|live| vault.persisted_state_from(live));
            vault
         }
         None => self.clone_vault(),
      };

      let encrypted_data = match vault.encrypt(new_params) {
         Ok(data) => data,
         Err(e) => {
            self.write(|ctx| ctx.save_vault_in_progress = false);
            return Err(e);
         }
      };

      if let Err(e) = vault.save(None, encrypted_data) {
         self.write(|ctx| ctx.save_vault_in_progress = false);
         return Err(e);
      }

      self.write(|ctx| ctx.save_vault_in_progress = false);
      Ok(())
   }

   pub fn save_wallet_state(&self) -> Result<(), anyhow::Error> {
      if self.save_wallet_state_in_progress() {
         return Err(anyhow!(
            "Saving wallet state in progress, try again later"
         ));
      }

      self.write(|ctx| {
         ctx.save_wallet_state_in_progress = true;
      });

      let key = self.read_vault(|vault| vault.wallet_state_key())?;
      let state = self.wallet_state();

      let res = state.encrypt_and_save(&key);

      self.write(|ctx| {
         ctx.save_wallet_state_in_progress = false;
      });

      res
   }

   pub fn save_vault_in_progress(&self) -> bool {
      self.read(|ctx| ctx.save_vault_in_progress)
   }

   pub fn save_wallet_state_in_progress(&self) -> bool {
      self.read(|ctx| ctx.save_wallet_state_in_progress)
   }

   /// Load `data/security.data` with the vault-held key.
   ///
   /// The file is optional: a missing file falls back to the defaults. Returns an error only when
   /// the vault has no `wallet_state_key` (i.e. it was not unlocked).
   ///
   /// A file that is there and does *not* read is a different matter, and is not treated as a
   /// missing one: `autolock` is a security choice, so running on the defaults is a weaker policy
   /// than the user picked, and sealing those defaults back would destroy the choice for good. The
   /// defaults are used for this session, the file is left exactly as it is (see
   /// [`Self::save_security_settings`]), and `security.load_failed` carries the failure to the UI.
   pub fn load_security_settings(&self) -> Result<(), anyhow::Error> {
      let key = self.read_vault(|vault| vault.wallet_state_key())?;
      let present = persisted::file_path(PersistedFile::Security).is_ok_and(|p| p.exists());
      let read = SecuritySettings::load_from_file(&key);

      if present {
         if let Err(e) = &read {
            tracing::error!("Failed to load security settings, leaving the file untouched: {e:?}");
         }
      }

      let settings = SecuritySettings::from_read(read, present);
      self.write(|ctx| ctx.security = settings);
      Ok(())
   }

   /// Seal and write `data/security.data` with the vault-held key.
   ///
   /// Refuses while `security.load_failed` is set: the settings in memory are the defaults, and
   /// sealing them over a file that did not read would replace the user's choice with them. A
   /// deliberate change in the security UI clears the flag and then saves.
   pub fn save_security_settings(&self) -> Result<(), anyhow::Error> {
      let key = self.read_vault(|vault| vault.wallet_state_key())?;
      let settings = self.read(|ctx| ctx.security.clone());

      if settings.load_failed {
         return Err(anyhow!(
            "Refusing to overwrite {}: it could not be read, so these settings are the defaults",
            persisted::file_path(PersistedFile::Security)
               .map(|p| p.display().to_string())
               .unwrap_or_else(|_| "security.data".to_string())
         ));
      }

      settings.save(&key)
   }

   pub fn save_currency_db(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving CurrencyDB: {:?}", e);
            return;
         }
      };
      let db = self.read(|ctx| ctx.currency_db.clone());
      match db.save(&key) {
         Ok(_) => tracing::trace!("CurrencyDB saved"),
         Err(e) => tracing::error!("Error saving CurrencyDB: {:?}", e),
      }
   }

   /// Load sealed `tokens.data` into the live context (no-op if the file is missing).
   pub fn load_currency_db(&self) {
      match CurrencyDB::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("Token data file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking CurrencyDB: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading CurrencyDB: {:?}", e);
            return;
         }
      };

      match CurrencyDB::load_from_file(&key) {
         Ok(db) => self.write(|ctx| ctx.currency_db = db),
         Err(e) => tracing::error!("Error loading CurrencyDB: {:?}", e),
      }
   }

   pub fn save_nft_db(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving NftDB: {:?}", e);
            return;
         }
      };
      let db = self.read(|ctx| ctx.nft_db.clone());
      match db.save(&key) {
         Ok(_) => tracing::trace!("NftDB saved"),
         Err(e) => tracing::error!("Error saving NftDB: {:?}", e),
      }
   }

   /// Load sealed `nft_db.data` into the live context (no-op if the file is missing).
   pub fn load_nft_db(&self) {
      match NftDB::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("NFT data file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking NftDB: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading NftDB: {:?}", e);
            return;
         }
      };

      match NftDB::load_from_file(&key) {
         Ok(db) => self.write(|ctx| ctx.nft_db = db),
         Err(e) => tracing::error!("Error loading NftDB: {:?}", e),
      }
   }

   pub fn save_address_book(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(_) => return,
      };
      let book = self.address_book();
      match book.save(&key) {
         Ok(_) => tracing::trace!("AddressBook saved"),
         Err(e) => tracing::error!("Error saving AddressBook: {:?}", e),
      }
   }

   /// Load `address_book.data`, or create it from well-known contracts, wallets, and contacts.
   ///
   /// Call after vault unlock (wallets + contacts + currency db are available).
   pub fn load_or_create_address_book(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading AddressBook: {:?}", e);
            return;
         }
      };

      let book = self.address_book();
      match AddressBookHandle::exists() {
         Ok(true) => match AddressBookHandle::load_from_file(&key) {
            Ok(loaded) => book.replace_from(&loaded),
            Err(e) => tracing::error!("Error loading AddressBook: {:?}", e),
         },
         Ok(false) => {}
         Err(e) => {
            tracing::error!("Error checking AddressBook: {:?}", e);
            return;
         }
      }

      book.seed_well_known();
      let wallets = self.get_all_wallets_info();
      book.seed_wallets(&wallets);
      let contacts = self.contacts();
      book.seed_contacts(&contacts);

      if let Err(e) = book.save(&key) {
         tracing::error!("Error saving AddressBook: {:?}", e);
      }
   }

   pub fn save_client_manager(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving ClientManager: {:?}", e);
            return;
         }
      };
      let client = self.get_client_manager();
      match client.save_to_file(&key) {
         Ok(_) => tracing::trace!("ClientManager saved"),
         Err(e) => tracing::error!("Error saving ClientManager: {:?}", e),
      }

      if let Err(e) = client.save_settings() {
         tracing::error!("Error saving client settings: {:?}", e);
      }
   }

   /// Load sealed `providers.data` into the live client (no-op if the file is missing).
   pub fn load_client_manager(&self) {
      let client = self.get_client_manager();
      client.load_settings();

      match ClientManager::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("ClientManager file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking ClientManager: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading ClientManager: {:?}", e);
            return;
         }
      };

      if let Err(e) = client.load_from_file(&key) {
         tracing::error!("Error loading ClientManager: {:?}", e);
      }
   }

   pub fn save_pool_manager(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving Pool Manager: {:?}", e);
            return;
         }
      };
      let manager = self.pool_manager();
      match manager.save_to_file(&key) {
         Ok(_) => {}
         Err(e) => tracing::error!("Error saving Pool Manager: {:?}", e),
      }
   }

   /// Load sealed `pool_data.data` into the live handle (no-op if the file is missing).
   pub fn load_pool_manager(&self) {
      match PoolManagerHandle::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("Pool Manager file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking Pool Manager: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading Pool Manager: {:?}", e);
            return;
         }
      };

      let manager = self.pool_manager();
      if let Err(e) = manager.load_from_file(&key) {
         tracing::error!("Error loading Pool Manager: {:?}", e);
      }
   }

   /// After an import has written `data/`, drop the live vault/state and load
   /// the imported files. `vault` is already unlocked by [`crate::core::data_import::import_data_from_zip`].
   pub fn reload_from_data_dir(
      &self,
      mut vault: Vault,
   ) -> Result<(WalletInfo, Argon2), anyhow::Error> {
      let info = vault.encrypted_info()?;
      vault.ensure_wallet_state_key()?;
      let key = vault.wallet_state_key()?;
      let (wallet_state, _) = WalletState::load_or_migrate(&key, None)?;
      let master_wallet = vault.get_master_wallet();

      let mut new_wallet_info_map = HashMap::new();
      let mut new_wallet_info_ordered = Vec::new();

      for wallet in vault.clone_all_wallets() {
         let info = WalletInfo::from_wallet(&wallet, true);
         new_wallet_info_map.insert(wallet.address(), info.clone());
         new_wallet_info_ordered.push(info);
      }

      let new_wallet_info_cache =
         WalletInfoCache::new(new_wallet_info_map, new_wallet_info_ordered);

      let master_info = new_wallet_info_cache
         .get(&master_wallet.address())
         .cloned()
         .unwrap_or_else(|| WalletInfo::from_wallet(&master_wallet, true));

      self.write_vault(|old| {
         old.erase();
      });
      self.set_vault(vault);
      self.set_wallet_state(wallet_state);
      self.load_tx_db();
      self.load_security_settings()?;

      self.address_book().replace_from(&AddressBookHandle::default());
      self.ens_cache().clear();

      let now = TimeStamp::now_as_millis().unwrap_or_default().timestamp();

      self.write(|ctx| {
         ctx.currency_db = CurrencyDB::default();
         ctx.nft_db = NftDB::default();
         ctx.delegated_wallets = DelegatedWallets::new();
         ctx.railgun_provider.clear();
         ctx.railgun_status = RailgunStatus::new();
         ctx.railgun_resync_attempts.clear();
         ctx.wallet_info_cache = new_wallet_info_cache;
         ctx.current_wallet = master_info.clone();
         ctx.security.argon_params = info.argon2.clone();
         ctx.locked = false;
         ctx.last_activity_ms = now;
         ctx.last_autolock_notice_ms = now;
         ctx.autolock_notice_shown = false;
         ctx.vault_exists = true;
         ctx.vault_unlocked = true;
         ctx.eth_calls.clear();
         ctx.estimate_gas.clear();
         ctx.codes.clear();
         ctx.storage.clear();
         ctx.transactions.clear();
         ctx.receipts.clear();
         ctx.blocks_by_hash.clear();
         ctx.blocks_by_number.clear();
         ctx.tx_counts.clear();
      });

      let defaults = ClientManager::default();
      self.get_client_manager().write(|map| {
         defaults.read(|d| *map = d.clone());
      });
      // Every endpoint set was replaced, so a fork learned from the node that used to answer for a
      // chain is no longer that chain's answer.
      self.get_client_manager().forget_node_forks();

      let fresh_pools = PoolManagerHandle::default();
      self.pool_manager().write(|manager| {
         fresh_pools.read(|fresh| *manager = fresh.clone());
      });

      let fresh_prices = PriceManagerHandle::new();
      self.price_manager().write(|manager| {
         fresh_prices.read(|fresh| *manager = fresh.clone());
      });

      match DisabledChains::load_from_file() {
         Ok(chains) => self.write(|ctx| ctx.disabled_chains = chains),
         Err(_) => self.write(|ctx| ctx.disabled_chains = DisabledChains::default()),
      }
      match RailgunConfig::load_from_file() {
         Ok(config) => self.write(|ctx| ctx.railgun_config = config),
         Err(_) => self.write(|ctx| ctx.railgun_config = RailgunConfig::default()),
      }
      match MiscConfig::load_from_file() {
         Ok(config) => self.write(|ctx| ctx.misc_config = config),
         Err(_) => self.write(|ctx| ctx.misc_config = MiscConfig::default()),
      }
      match AppearanceSettings::load_from_file() {
         Ok(config) => self.write(|ctx| ctx.appearance_settings = config),
         Err(_) => self.write(|ctx| ctx.appearance_settings = AppearanceSettings::default()),
      }

      self.load_currency_db();
      self.load_nft_db();
      self.load_pool_manager();
      self.load_client_manager();
      self.load_price_manager();
      self.load_or_create_address_book();

      Ok((master_info, info.argon2))
   }

   pub fn save_price_manager(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving Price Manager: {:?}", e);
            return;
         }
      };
      let manager = self.price_manager();
      match manager.save_to_file(&key) {
         Ok(_) => tracing::trace!("Price Manager saved"),
         Err(e) => tracing::error!("Error saving Price Manager: {:?}", e),
      }
   }

   /// Load sealed `price_data.data` into the live handle (no-op if the file is missing).
   pub fn load_price_manager(&self) {
      match PriceManagerHandle::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("Price Manager file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking Price Manager: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading Price Manager: {:?}", e);
            return;
         }
      };

      let manager = self.price_manager();
      if let Err(e) = manager.load_from_file(&key) {
         tracing::error!("Error loading Price Manager: {:?}", e);
      }
   }

   /// Open `tx_history.db`, migrate any txs still embedded in WalletState, then
   /// clear that field and re-save wallet state so the old blob shrinks.
   pub fn load_tx_db(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading Tx history: {:?}", e);
            return;
         }
      };

      let tx_db = match TxDBHandle::open(&key) {
         Ok(db) => db,
         Err(e) => {
            tracing::error!("Error opening Tx history db: {:?}", e);
            let legacy = self.read_wallet_state(|ws| ws.tx_db.clone());
            self.write(|ctx| ctx.tx_db = legacy);
            return;
         }
      };

      let legacy = self.read_wallet_state(|ws| ws.tx_db.clone());
      match tx_db.import_legacy(&legacy) {
         Ok(true) => {
            self.write_wallet_state(|ws| {
               ws.tx_db = TxDBHandle::new();
            });
            self.write(|ctx| ctx.tx_db = tx_db);
            if let Err(e) = self.save_wallet_state() {
               tracing::error!(
                  "Error saving WalletState after tx db migration: {:?}",
                  e
               );
            } else {
               tracing::info!("Migrated transaction history to tx_history.db");
            }
         }
         Ok(false) => {
            self.write(|ctx| ctx.tx_db = tx_db);
         }
         Err(e) => {
            tracing::error!("Error migrating Tx history to redb: {:?}", e);
            self.write(|ctx| ctx.tx_db = legacy);
         }
      }
   }

   pub fn save_disabled_chains(&self) {
      let chains = self.read(|ctx| ctx.disabled_chains.clone());
      match chains.save_to_file() {
         Ok(_) => tracing::trace!("Disabled Chains saved"),
         Err(e) => tracing::error!("Error saving disabled chains: {:?}", e),
      }
   }
}
