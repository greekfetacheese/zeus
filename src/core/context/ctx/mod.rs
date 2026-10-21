use super::{
   AddressBookHandle, ApprovalManagerHandle, BalanceManagerHandle, ClientManager, CurrencyDB,
   EnsCache, NftDB, PoolManagerHandle, price_manager::PriceManagerHandle, tx::TxDBHandle,
};
use crate::core::types::DEFAULT_CONNECTOR_PORT;
use crate::core::{Vault, WalletInfo, WalletState, persisted, types::*};
use cache::CONNECTOR_CACHE_CAP;
use persist::tighten_existing_secret_files;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use zeus_eth::{
   alloy_primitives::{Address, Bytes, FixedBytes, U256},
   alloy_rpc_types::{Transaction, TransactionReceipt, TransactionRequest},
   types::{ChainId, SUPPORTED_CHAINS},
   utils::client::RpcClient,
};
use zeus_railgun::RailgunProvider;

/// This is the minimum USD value in a base currency that a pool needs to have in order to be considered sufficiently liquid
pub const DEFAULT_POOL_MINIMUM_LIQUIDITY: f64 = 10_000.0;

pub const DELEGATE_WALLET_CHECK_TIMEOUT: u64 = 600;

/// Thread-safe handle to the [ZeusContext]
#[derive(Clone)]
pub struct ZeusCtx(Arc<RwLock<ZeusContext>>);

pub struct ZeusContext {
   /// Client manager that handles almost all the RPC calls in Zeus
   pub client_manager: ClientManager,

   /// The current selected chain from the GUI
   pub chain: ChainId,

   /// True if the privacy mode is enabled (Railgun)
   pub privacy_mode: bool,

   /// Railgun resync attempts
   pub railgun_resync_attempts: HashMap<u64, u8>,

   /// Railgun provider mapped by chain
   pub railgun_provider: HashMap<u64, RailgunProvider<RpcClient>>,

   /// The current selected wallet from the GUI (no key material).
   pub current_wallet: WalletInfo,

   /// Cached `WalletInfo` for quickly accessing & cloning any wallet
   /// without its private key.
   pub wallet_info_cache: WalletInfoCache,

   /// Loaded Vault
   pub vault: Arc<Mutex<Vault>>,

   /// Frequently updated wallet app state (contacts, balances, portfolios, txs, …).
   ///
   /// Sealed separately in `wallet_state.data` with [`Vault`]'s `wallet_state_key`.
   pub wallet_state: WalletState,

   /// Transaction history (`tx_history.db`), sealed with [`Vault`]'s `wallet_state_key`.
   pub tx_db: TxDBHandle,

   /// Security settings (persisted, sealed in `security.data` with the vault's
   /// `wallet_state_key`). Holds the Argon2 params of the current vault.
   pub security: SecuritySettings,

   /// True while the UI is locked by auto-lock. Not persisted; false at startup.
   pub locked: bool,

   /// Wall-clock ms of the last user input, for the auto-lock timer.
   pub last_activity_ms: u64,

   /// Wall-clock ms of the last "auto-lock is not configured" toast.
   pub last_autolock_notice_ms: u64,

   /// False until the first "auto-lock is not configured" toast has fired, so
   /// the first one comes `first_delay` after unlock and the rest `repeat`.
   pub autolock_notice_shown: bool,

   /// True if a vault exists in the data directory
   pub vault_exists: bool,

   /// True if the vault is unlocked,
   /// only then the Zeus UI unlocks
   pub vault_unlocked: bool,

   /// Fast address names (wallets, contacts, well-known + ERC-7730/Sourcify).
   ///
   /// Token names stay in [`Self::currency_db`]. ENS names live in [`Self::ens_cache`] — they can
   /// expire, so they are deliberately not persisted here.
   pub address_book: AddressBookHandle,

   /// ENS names resolved this session. In memory only — see [`EnsCache`].
   pub ens_cache: EnsCache,

   /// Holds all ERC20 tokens
   pub currency_db: CurrencyDB,

   /// Holds the NFTs the user tracks, plus cached collection metadata
   pub nft_db: NftDB,

   /// Pool manager used for the Uniswap UI
   /// and price manager
   pub pool_manager: PoolManagerHandle,

   /// Calculate the $USD price of an ERC20 token
   /// based purely on the on-chain pool data
   /// no 3rd party APIs
   pub price_manager: PriceManagerHandle,

   /// State flags for the UI that showup on the top right corner
   pub data_syncing: bool,
   pub on_startup_syncing: bool,
   pub save_vault_in_progress: bool,
   pub save_wallet_state_in_progress: bool,

   /// State sync flag per chain
   pub state_sync: HashMap<u64, bool>,

   /// Cached base fees for each chain
   pub base_fee: HashMap<u64, BaseFee>,

   // Cached data for the wallet connector
   pub latest_block: HashMap<u64, Block>,
   pub eth_calls: HashMap<(u64, TransactionRequest), EthCall>,
   pub estimate_gas: HashMap<(u64, TransactionRequest), EstimateGas>,
   pub codes: HashMap<(u64, u64, Address), Bytes>,
   pub storage: HashMap<(u64, u64, Address, U256), U256>,
   pub transactions: HashMap<(u64, FixedBytes<32>), Transaction>,
   pub receipts: HashMap<(u64, FixedBytes<32>), TransactionReceipt>,
   pub blocks_by_hash: HashMap<(u64, FixedBytes<32>), CachedBlock>,
   pub blocks_by_number: HashMap<(u64, u64), CachedBlock>,
   pub tx_counts: HashMap<(u64, Address), TransactionCount>,

   /// Cached priority fees for each chain
   pub priority_fee: PriorityFee,

   /// Currently connected dapps
   pub connected_dapps: ConnectedDapps,

   /// Currently delegated wallets
   pub delegated_wallets: DelegatedWallets,

   /// Local connector HTTP port (default [`crate::core::types::DEFAULT_CONNECTOR_PORT`];
   /// may change at bind time if that port is taken).
   pub server_port: u16,

   /// True if the local server that communicates with the wallet connector (browser extension) is running
   pub server_running: bool,

   /// Last time checked for available RPCs
   pub last_checked_for_available_rpcs: HashMap<u64, u64>,

   /// Last time checked for malfunctioning RPCs
   pub last_checked_for_malfunction: HashMap<u64, u64>,

   /// Last time we detected a malfunctioning RPC
   pub last_detected_malfunction: HashMap<u64, u64>,

   /// Mapped available RPCs for each chain
   ///
   /// - `Key`: chain_id
   ///
   /// - `Value`: true if we have at least one working & enabled RPC
   pub available_rpcs: HashMap<u64, bool>,

   /// Last UNIX ms timestamp we checked if a railgun provider
   /// is syncing for the given chain
   pub railgun_provider_sync_last_check: HashMap<u64, u64>,

   /// Disabled Chains
   pub disabled_chains: DisabledChains,

   /// Railgun status for the services UI
   pub railgun_status: RailgunStatus,

   /// Railgun configuration
   pub railgun_config: RailgunConfig,

   /// Misc persisted settings (token icons, Sourcify names, …)
   pub misc_config: MiscConfig,
}

mod auth;
mod cache;
mod dapps;
mod persist;
mod railgun;
mod rpc;
mod state;
mod wallet;

#[cfg(test)]
mod tests;

pub use persisted::{
   appearance_settings_dir, bundler_url_dir, data_dir, disabled_chains_dir, misc_config_dir,
   pool_data_dir, railgun_config_dir, railgun_db_file, railgun_dir, security_dir, theme_kind_dir,
};

pub use railgun::create_railgun_provider;

impl ZeusCtx {
   pub fn new() -> Self {
      Self(Arc::new(RwLock::new(ZeusContext::new())))
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
   pub fn read_wallet_state<R>(
      &self,
      reader: impl FnOnce(&crate::core::WalletStateInner) -> R,
   ) -> R {
      self.wallet_state().read(reader)
   }

   /// Mutable access to wallet app state (does not hold the ZeusContext lock).
   pub fn write_wallet_state<R>(
      &self,
      writer: impl FnOnce(&mut crate::core::WalletStateInner) -> R,
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
   pub fn new() -> Self {
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
   pub fn read_wallet_state<R>(
      &self,
      reader: impl FnOnce(&crate::core::WalletStateInner) -> R,
   ) -> R {
      self.wallet_state.read(reader)
   }

   /// Mutable access to wallet app state.
   pub fn write_wallet_state<R>(
      &self,
      writer: impl FnOnce(&mut crate::core::WalletStateInner) -> R,
   ) -> R {
      self.wallet_state.write(writer)
   }
}
