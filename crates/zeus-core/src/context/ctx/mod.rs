use super::{
   AddressBookHandle, ClientManager, CurrencyDB, EnsCache, NftDB, PoolManagerHandle,
   price_manager::PriceManagerHandle, tx::TxDBHandle,
};
use crate::{Vault, WalletInfo, WalletState, persisted, types::*};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use zeus_eth::{
   alloy_primitives::{Address, Bytes, FixedBytes, U256},
   alloy_rpc_types::{Transaction, TransactionReceipt, TransactionRequest},
   types::ChainId,
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
   /// The tokio handle background work spawns on. The runtime itself is owned by the binary
   /// (or by a test); core only ever holds the handle.
   pub rt: tokio::runtime::Handle,

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

   /// Local connector HTTP port (default [`crate::types::DEFAULT_CONNECTOR_PORT`];
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
mod zeus_ctx;

#[cfg(test)]
mod tests;

pub use persisted::{
   appearance_settings_dir, bundler_url_dir, data_dir, disabled_chains_dir, misc_config_dir,
   pool_data_dir, railgun_config_dir, railgun_db_file, railgun_dir, security_dir, theme_kind_dir,
};

pub use railgun::create_railgun_provider;
