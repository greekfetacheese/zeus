//! The `impl` blocks for [`ZeusCtx`] and [`ZeusContext`].

use super::super::{
   AddressBookHandle, ApprovalManagerHandle, BalanceManagerHandle, ClientManager, CurrencyDB,
   EnsCache, NftDB, PoolManagerHandle, price_manager::PriceManagerHandle, tx::TxDBHandle,
};
use super::cache::CONNECTOR_CACHE_CAP;
use super::persist::tighten_existing_secret_files;
use super::{ZeusContext, ZeusCtx};
use crate::types::DEFAULT_CONNECTOR_PORT;
use crate::{Vault, WalletInfo, WalletState, types::*};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use zeus_eth::types::{ChainId, SUPPORTED_CHAINS};

impl ZeusCtx {
   pub fn new(rt: tokio::runtime::Handle) -> Self {
      Self(Arc::new(RwLock::new(ZeusContext::new(rt))))
   }

   /// Cheap clone of the handle background work spawns on.
   pub fn rt(&self) -> tokio::runtime::Handle {
      self.read(|ctx| ctx.rt.clone())
   }

   /// Shared access to the context
   pub fn read<R>(&self, reader: impl FnOnce(&ZeusContext) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   /// Exclusive mutable access to the context
   pub fn write<R>(&self, writer: impl FnOnce(&mut ZeusContext) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   pub fn pool_manager(&self) -> PoolManagerHandle {
      self.read(|ctx| ctx.pool_manager.clone())
   }

   pub fn price_manager(&self) -> PriceManagerHandle {
      self.read(|ctx| ctx.price_manager.clone())
   }

   pub fn balance_manager(&self) -> BalanceManagerHandle {
      self.read_wallet_state(|ws| ws.balance_manager.clone())
   }

   /// Cheap clone of the vault handle (`Arc<Mutex<Vault>>`).
   fn vault_handle(&self) -> Arc<Mutex<Vault>> {
      self.read(|ctx| Arc::clone(&ctx.vault))
   }

   /// Cheap clone of the wallet-state handle.
   pub fn wallet_state(&self) -> WalletState {
      self.read(|ctx| ctx.wallet_state.clone())
   }

   /// Shared access to the vault without cloning its contents.
   pub fn read_vault<R>(&self, reader: impl FnOnce(&Vault) -> R) -> R {
      let vault = self.vault_handle();
      reader(&vault.lock().unwrap())
   }

   /// Shared access to wallet app state (contacts, balances, portfolios, txs, …).
   pub fn read_wallet_state<R>(&self, reader: impl FnOnce(&crate::WalletStateInner) -> R) -> R {
      self.wallet_state().read(reader)
   }

   /// Mutable access to wallet app state (does not hold the ZeusContext lock).
   pub fn write_wallet_state<R>(
      &self,
      writer: impl FnOnce(&mut crate::WalletStateInner) -> R,
   ) -> R {
      self.wallet_state().write(writer)
   }

   /// Replace wallet-state contents in-place (same `Arc`).
   pub fn set_wallet_state(&self, state: WalletState) {
      self.write(|ctx| {
         // Keep the same outer handle so clones stay valid, copy inner into live Arc.
         let live = ctx.wallet_state.clone();
         live.set(state.clone_inner());
      });
   }

   /// Mutable access to the vault (does not hold the ZeusContext lock).
   pub fn write_vault<R>(&self, writer: impl FnOnce(&mut Vault) -> R) -> R {
      let vault = self.vault_handle();
      writer(&mut vault.lock().unwrap())
   }

   /// Replace vault contents in-place (same `Arc`).
   pub fn set_vault(&self, new_vault: Vault) {
      self.write_vault(|vault| {
         *vault = new_vault;
      });
   }

   /// Deep-clone vault contents. Prefer [`Self::read_vault`] / [`Self::write_vault`]
   /// for ordinary access; use this only for offline mutate → encrypt snapshots.
   pub fn clone_vault(&self) -> Vault {
      self.read_vault(|vault| vault.clone())
   }

   /// Alias for [`Self::clone_vault`] (legacy name used at wallet mutation sites).
   pub fn get_vault(&self) -> Vault {
      self.clone_vault()
   }

   pub fn address_book(&self) -> AddressBookHandle {
      self.read(|ctx| ctx.address_book.clone())
   }

   /// The session-only ENS name cache. See [`EnsCache`].
   pub fn ens_cache(&self) -> EnsCache {
      self.read(|ctx| ctx.ens_cache.clone())
   }

   pub fn get_client_manager(&self) -> ClientManager {
      self.read(|ctx| ctx.client_manager.clone())
   }

   pub fn chain(&self) -> ChainId {
      self.read(|ctx| ctx.chain)
   }

   pub fn tx_db(&self) -> TxDBHandle {
      self.read(|ctx| ctx.tx_db.clone())
   }

   pub fn approval_manager(&self) -> ApprovalManagerHandle {
      self.read_wallet_state(|ws| ws.approval_manager.clone())
   }

   pub fn server_port(&self) -> u16 {
      self.read(|ctx| ctx.server_port)
   }

   pub fn server_running(&self) -> bool {
      self.read(|ctx| ctx.server_running)
   }
}

impl ZeusContext {
   pub fn new(rt: tokio::runtime::Handle) -> Self {
      tighten_existing_secret_files();

      let client_manager = ClientManager::default();

      let currency_db = CurrencyDB::default();

      let nft_db = NftDB::default();

      let vault_exists = Vault::exists().is_ok_and(|p| p);

      let pool_manager = PoolManagerHandle::default();

      let price_manager = PriceManagerHandle::new();

      let delegated_wallets = DelegatedWallets::new();

      let disabled_chains = match DisabledChains::load_from_file() {
         Ok(chains) => chains,
         Err(e) => {
            tracing::error!("Failed to load disabled chains: {:?}", e);
            DisabledChains::default()
         }
      };

      let railgun_config = match RailgunConfig::load_from_file() {
         Ok(config) => config,
         Err(e) => {
            tracing::error!("Failed to load Railgun config: {:?}", e);
            RailgunConfig::default()
         }
      };

      let misc_config = match MiscConfig::load_from_file() {
         Ok(config) => config,
         Err(e) => {
            tracing::error!("Failed to load misc config: {:?}", e);
            MiscConfig::default()
         }
      };

      let priority_fee = PriorityFee::default();

      Self {
         rt,
         client_manager,
         chain: ChainId::new(1).unwrap(),
         privacy_mode: false,
         railgun_resync_attempts: HashMap::new(),
         railgun_provider: HashMap::new(),
         current_wallet: WalletInfo::default(),
         wallet_info_cache: WalletInfoCache::default(),
         vault: Arc::new(Mutex::new(Vault::default())),
         wallet_state: WalletState::default(),
         tx_db: TxDBHandle::new(),
         security: SecuritySettings::default(),
         locked: false,
         last_activity_ms: 0,
         last_autolock_notice_ms: 0,
         autolock_notice_shown: false,
         save_vault_in_progress: false,
         save_wallet_state_in_progress: false,
         vault_exists,
         vault_unlocked: false,
         address_book: AddressBookHandle::default(),
         ens_cache: EnsCache::new(),
         currency_db,
         nft_db,
         pool_manager,
         price_manager,
         data_syncing: false,
         on_startup_syncing: false,
         state_sync: HashMap::with_capacity(SUPPORTED_CHAINS.len()),
         base_fee: HashMap::with_capacity(SUPPORTED_CHAINS.len()),
         latest_block: HashMap::with_capacity(SUPPORTED_CHAINS.len()),
         eth_calls: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         estimate_gas: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         codes: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         storage: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         transactions: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         receipts: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         blocks_by_hash: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         blocks_by_number: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         tx_counts: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         priority_fee,
         connected_dapps: ConnectedDapps::default(),
         delegated_wallets,
         server_port: DEFAULT_CONNECTOR_PORT,
         server_running: false,
         last_checked_for_available_rpcs: HashMap::new(),
         last_checked_for_malfunction: HashMap::new(),
         last_detected_malfunction: HashMap::new(),
         railgun_provider_sync_last_check: HashMap::new(),
         available_rpcs: HashMap::new(),
         disabled_chains,
         railgun_status: RailgunStatus::new(),
         railgun_config,
         misc_config,
      }
   }

   /// Shared access to the vault without cloning its contents.
   pub fn read_vault<R>(&self, reader: impl FnOnce(&Vault) -> R) -> R {
      reader(&self.vault.lock().unwrap())
   }

   /// Mutable access to the vault.
   pub fn write_vault<R>(&self, writer: impl FnOnce(&mut Vault) -> R) -> R {
      writer(&mut self.vault.lock().unwrap())
   }

   /// Shared access to wallet app state.
   pub fn read_wallet_state<R>(&self, reader: impl FnOnce(&crate::WalletStateInner) -> R) -> R {
      self.wallet_state.read(reader)
   }

   /// Mutable access to wallet app state.
   pub fn write_wallet_state<R>(
      &self,
      writer: impl FnOnce(&mut crate::WalletStateInner) -> R,
   ) -> R {
      self.wallet_state.write(writer)
   }
}
