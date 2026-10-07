use std::collections::{HashMap, HashSet};
use zeus_eth::{
   alloy_primitives::{Address, Bytes},
   alloy_rpc_types::Block as RpcBlock,
   types::ETH_SEPOLIA,
   types::{ChainId, SUPPORTED_CHAINS},
   utils::NumericValue,
};

use crate::core::{
   WalletInfo, WalletStateKey, ZK_ADDRESS_UNAVAILABLE,
   context::{
      DELEGATE_WALLET_CHECK_TIMEOUT, disabled_chains_dir, misc_config_dir, railgun_config_dir,
      security_dir,
   },
};
use crate::utils::{TimeStamp, write_private, write_private_atomic};

use zeus_railgun::indexer::syncer::rpc::{
   DEFAULT_BLOCK_RANGE, DEFAULT_CONCURRENCY, SEPOLIA_BLOCK_RANGE,
};

use serde::{Deserialize, Serialize};

use ncrypt_me::Argon2;

const DEFAULT_STATE_UPDATE_INTERVAL_MINUTES: u64 = 5;
const MIN_STATE_UPDATE_INTERVAL_MINUTES: u64 = 1;
const MAX_STATE_UPDATE_INTERVAL_MINUTES: u64 = 60;

fn default_state_update_interval_minutes() -> u64 {
   DEFAULT_STATE_UPDATE_INTERVAL_MINUTES
}

const MIN_BLOCK_RANGE: u64 = 100;
const MAX_BLOCK_RANGE: u64 = 30_000;

const MIN_CONCURRENCY: usize = 1;
const MAX_CONCURRENCY: usize = 10;

const MIN_UPDATE_INTERVAL: u64 = 1;
const MAX_UPDATE_INTERVAL: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RailgunConfig {
   pub rpc_syncer_concurrency: usize,
   pub rpc_syncer_block_range: HashMap<u64, u64>,
   #[serde(default)]
   pub enabled: HashMap<u64, bool>,
   /// When false, Railgun proving uses only embedded and on-disk circuits.
   #[serde(default)]
   pub allow_circuit_download: bool,
   /// How often to sync Railgun state, in minutes (1–60).
   #[serde(default = "default_state_update_interval_minutes")]
   pub state_update_interval_minutes: u64,
}

impl RailgunConfig {
   pub fn min_block_range() -> u64 {
      MIN_BLOCK_RANGE
   }
   pub fn max_block_range() -> u64 {
      MAX_BLOCK_RANGE
   }

   pub fn min_concurrency() -> usize {
      MIN_CONCURRENCY
   }

   pub fn max_concurrency() -> usize {
      MAX_CONCURRENCY
   }

   pub fn min_state_update_interval() -> u64 {
      MIN_UPDATE_INTERVAL
   }

   pub fn max_state_update_interval() -> u64 {
      MAX_UPDATE_INTERVAL
   }

   pub fn new() -> Self {
      let mut rpc_syncer_block_range = HashMap::new();

      for chain in ChainId::supported_chains() {
         let range = match chain {
            ChainId::Ethereum => DEFAULT_BLOCK_RANGE,
            ChainId::EthereumSepolia => SEPOLIA_BLOCK_RANGE,
            _ => DEFAULT_BLOCK_RANGE,
         };
         rpc_syncer_block_range.insert(chain.id(), range);
      }

      Self {
         rpc_syncer_concurrency: DEFAULT_CONCURRENCY,
         rpc_syncer_block_range,
         enabled: HashMap::new(),
         allow_circuit_download: false,
         state_update_interval_minutes: DEFAULT_STATE_UPDATE_INTERVAL_MINUTES,
      }
   }

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = railgun_config_dir()?;
      let data = std::fs::read_to_string(dir)?;
      let config = serde_json::from_str(&data)?;

      Ok(config)
   }

   pub fn save(&self) -> Result<(), anyhow::Error> {
      let dir = railgun_config_dir()?;
      let data = serde_json::to_string(self)?;
      write_private(&dir, data.as_bytes())?;
      Ok(())
   }

   /// Empty map (the default) means Railgun is disabled for every chain.
   pub fn is_enabled(&self, chain: u64) -> bool {
      self.enabled.get(&chain).copied().unwrap_or(false)
   }

   pub fn set_enabled(&mut self, chain: u64, enabled: bool) {
      self.enabled.insert(chain, enabled);
   }

   pub fn any_enabled(&self) -> bool {
      self.enabled.values().any(|enabled| *enabled)
   }

   pub fn allow_circuit_download(&self) -> bool {
      self.allow_circuit_download
   }

   pub fn set_allow_circuit_download(&mut self, allow: bool) {
      self.allow_circuit_download = allow;
   }

   pub fn rpc_syncer_block_range(&self, chain: u64) -> u64 {
      let range = self.rpc_syncer_block_range.get(&chain).cloned().unwrap_or_default();
      range.clamp(MIN_BLOCK_RANGE, MAX_BLOCK_RANGE)
   }

   pub fn rpc_syncer_concurrency(&self) -> usize {
      self.rpc_syncer_concurrency.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY)
   }

   /// Railgun state-update interval in seconds (clamped to 1–60 minutes).
   pub fn state_update_interval_secs(&self) -> u64 {
      self
         .state_update_interval_minutes
         .clamp(
            MIN_STATE_UPDATE_INTERVAL_MINUTES,
            MAX_STATE_UPDATE_INTERVAL_MINUTES,
         )
         .saturating_mul(60)
   }
}

impl Default for RailgunConfig {
   fn default() -> Self {
      Self::new()
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiscConfig {
   /// When true, unknown ERC-20 icons and NFT images may be downloaded from the network —
   /// SmolDapp for tokens, the collection's own metadata URI for NFTs.
   ///
   /// `alias` keeps the choice of anyone who opted in before this flag was widened: without it the
   /// rename would silently reset their setting back to "off".
   #[serde(default, alias = "fetch_token_icons")]
   pub fetch_asset_images: bool,
   /// When true, unknown contract names may be fetched from Sourcify.
   #[serde(default)]
   pub fetch_contract_names: bool,
   /// When true, Zeus may query GitHub for a newer release.
   #[serde(default)]
   pub check_for_updates: bool,
}

impl MiscConfig {
   pub fn new() -> Self {
      Self {
         fetch_asset_images: false,
         fetch_contract_names: false,
         check_for_updates: false,
      }
   }

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = misc_config_dir()?;
      let data = std::fs::read_to_string(dir)?;
      let config = serde_json::from_str(&data)?;

      Ok(config)
   }

   pub fn save(&self) -> Result<(), anyhow::Error> {
      let dir = misc_config_dir()?;
      let data = serde_json::to_string(self)?;
      write_private(&dir, data.as_bytes())?;
      Ok(())
   }

   pub fn fetch_asset_images(&self) -> bool {
      self.fetch_asset_images
   }

   pub fn set_fetch_asset_images(&mut self, allow: bool) {
      self.fetch_asset_images = allow;
   }

   pub fn fetch_contract_names(&self) -> bool {
      self.fetch_contract_names
   }

   pub fn set_fetch_contract_names(&mut self, allow: bool) {
      self.fetch_contract_names = allow;
   }

   pub fn check_for_updates(&self) -> bool {
      self.check_for_updates
   }

   pub fn set_check_for_updates(&mut self, allow: bool) {
      self.check_for_updates = allow;
   }
}

impl Default for MiscConfig {
   fn default() -> Self {
      Self::new()
   }
}

/// AAD bound to the sealed `security.data` slot.
pub const SECURITY_AAD: &[u8] = b"zeus-security-v1";

/// Idle period before Zeus locks the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoLock {
   TenMinutes,
   OneHour,
   FourHours,
   Never,
   /// Dev-build option. The variant is always compiled so a `security.data`
   /// written by a dev build still deserializes in release.
   OneMinute,
}

impl AutoLock {
   /// Release-build choices, in menu order.
   pub const ALL: [Self; 4] = [
      Self::TenMinutes,
      Self::OneHour,
      Self::FourHours,
      Self::Never,
   ];

   /// Dev-build choices (adds the 1-minute option first).
   #[cfg(feature = "dev")]
   pub const ALL_DEV: [Self; 5] = [
      Self::OneMinute,
      Self::TenMinutes,
      Self::OneHour,
      Self::FourHours,
      Self::Never,
   ];

   pub fn label(self) -> &'static str {
      match self {
         Self::OneMinute => "1 minute",
         Self::TenMinutes => "10 minutes",
         Self::OneHour => "1 hour",
         Self::FourHours => "4 hours",
         Self::Never => "Never",
      }
   }

   /// Idle seconds before locking; `None` = never lock.
   pub fn idle_secs(self) -> Option<u64> {
      match self {
         Self::OneMinute => Some(60),
         Self::TenMinutes => Some(600),
         Self::OneHour => Some(3_600),
         Self::FourHours => Some(14_400),
         Self::Never => None,
      }
   }
}

impl Default for AutoLock {
   fn default() -> Self {
      Self::OneHour
   }
}

/// Security settings, sealed in `data/security.data` with the vault's
/// `wallet_state_key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySettings {
   /// Idle timeout before the UI locks.
   #[serde(default)]
   pub autolock: AutoLock,
   /// Whether the user ever changed `autolock` away from the default. While
   /// false, the top bar nudges them to make a deliberate choice.
   #[serde(default)]
   pub autolock_changed: bool,
   /// Argon2 params of the current vault. Persisted in `vault.data`'s header —
   /// never duplicated here.
   #[serde(skip)]
   pub argon_params: Argon2,
}

impl SecuritySettings {
   pub fn load_from_file(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let sealed = std::fs::read(security_dir()?)?;
      key.open_json(&sealed, SECURITY_AAD)
   }

   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let sealed = key.seal_json(self, SECURITY_AAD)?;
      write_private_atomic(&security_dir()?, &sealed)?;
      Ok(())
   }
}

impl Default for SecuritySettings {
   fn default() -> Self {
      Self {
         autolock: AutoLock::default(),
         autolock_changed: false,
         argon_params: Argon2::balanced(),
      }
   }
}

#[derive(Default, Clone, Debug)]
pub struct Recipient {
   pub name: Option<String>,
   pub evm_address: String,
   pub zk_address: String,
   /// The chain an ERC-7828 name resolved for (`vitalik.eth@base` → `8453`). `None` when the
   /// recipient is chain-agnostic — a plain address, contact, wallet or plain ENS name.
   pub chain: Option<u64>,
}

impl Recipient {
   pub fn from_unknown_evm_address(address: Address) -> Self {
      Self {
         name: None,
         evm_address: address.to_string(),
         zk_address: String::new(),
         chain: None,
      }
   }

   pub fn from_unknown_zk_address(address: String) -> Self {
      Self {
         name: None,
         evm_address: String::new(),
         zk_address: address,
         chain: None,
      }
   }

   /// Recipient resolved from an ENS name. The address is what gets sent, the name
   /// is display only.
   ///
   /// `chain` is `Some` only when the name was chain-specific (`name@chain`, ERC-7828), which is
   /// what makes the send path refuse to go out on a different chain; `name` is `None` for a
   /// chain-specific *raw address* (`0x…@eip155:1`).
   pub fn from_ens_name(name: Option<String>, address: Address, chain: Option<u64>) -> Self {
      Self {
         name,
         evm_address: address.to_string(),
         zk_address: String::new(),
         chain,
      }
   }

   pub fn from_wallet_info(wallet_info: WalletInfo) -> Self {
      Self {
         name: Some(wallet_info.name_with_source()),
         evm_address: wallet_info.address.to_string(),
         zk_address: wallet_info.zk_address(),
         chain: None,
      }
   }

   pub fn from_contact(contact: Contact) -> Self {
      Self {
         name: Some(contact.name),
         evm_address: contact.evm_address,
         zk_address: contact.zk_address,
         chain: None,
      }
   }

   pub fn is_empty(&self, privacy_mode: bool) -> bool {
      if privacy_mode {
         return self.zk_address.is_empty();
      } else {
         return self.evm_address.is_empty();
      }
   }
}

/// Saved contact by the user
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
   pub name: String,
   #[serde(rename = "address")]
   pub evm_address: String,
   #[serde(default)]
   pub zk_address: String,
}

impl Contact {
   pub fn new(name: String, evm_address: String, zk_address: String) -> Self {
      Self {
         name,
         evm_address,
         zk_address,
      }
   }

   pub fn zk_address_truncated(&self) -> String {
      let zk_address = if self.zk_address.is_empty() {
         None
      } else {
         Some(self.zk_address.clone())
      };

      match &zk_address {
         Some(address) => format!("{}...{}", &address[..6], &address[121..]),
         None => ZK_ADDRESS_UNAVAILABLE.to_string(),
      }
   }
}

#[derive(Clone)]
pub struct Block {
   pub number: u64,
   pub timestamp: u64,
}

impl Block {
   pub fn new(number: u64, timestamp: u64) -> Self {
      Self { number, timestamp }
   }
}

#[derive(Clone)]
pub struct EthCall {
   pub timestamp: u64,
   pub result: Bytes,
}

#[derive(Clone)]
pub struct EstimateGas {
   pub timestamp: u64,
   pub gas: u64,
}

/// Cached block for the wallet-connector `eth_getBlockByHash` /
/// `eth_getBlockByNumber`.
///
/// Unlike the small [`Block`] above, this holds the full RPC block so it can be
/// serialized back to the dapp.
#[derive(Clone)]
pub struct CachedBlock {
   pub timestamp: u64,
   /// Whether [`Self::block`] carries full transactions (`true`) or hashes only.
   pub hydrated: bool,
   pub block: RpcBlock,
}

/// Cached `eth_getTransactionCount` nonce for an address.
#[derive(Clone)]
pub struct TransactionCount {
   pub timestamp: u64,
   pub count: u64,
}

#[derive(Debug, Clone)]
pub struct BaseFee {
   pub current: u64,
   pub next: u64,
}

impl Default for BaseFee {
   fn default() -> Self {
      Self {
         current: 1,
         next: 1,
      }
   }
}

impl BaseFee {
   pub fn new(current: u64, next: u64) -> Self {
      Self { current, next }
   }
}

/// A set of chains that are disabled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisabledChains {
   pub chains: HashSet<u64>,
}

impl Default for DisabledChains {
   fn default() -> Self {
      let mut chains = HashSet::new();
      chains.insert(ETH_SEPOLIA);
      Self { chains }
   }
}

impl DisabledChains {
   pub fn new(chains: HashSet<u64>) -> Self {
      Self { chains }
   }

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = disabled_chains_dir()?;
      let data = std::fs::read(dir)?;
      let disabled_chains = serde_json::from_slice(&data)?;
      Ok(disabled_chains)
   }

   pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
      let data = serde_json::to_string(self)?;
      let dir = disabled_chains_dir()?;
      write_private(&dir, data.as_bytes())?;
      Ok(())
   }

   pub fn disable(&mut self, chain: u64) {
      self.chains.insert(chain);
   }

   pub fn enable(&mut self, chain: u64) {
      self.chains.remove(&chain);
   }

   pub fn is_disabled(&self, chain: u64) -> bool {
      self.chains.contains(&chain)
   }
}

/// Suggested priority fees for each chain
#[derive(Debug, Clone)]
pub struct PriorityFee {
   pub fee: HashMap<u64, NumericValue>,
}

impl PriorityFee {
   pub fn get(&self, chain: u64) -> Option<&NumericValue> {
      self.fee.get(&chain)
   }
}

impl Default for PriorityFee {
   fn default() -> Self {
      let mut map = HashMap::with_capacity(SUPPORTED_CHAINS.len());

      let chains = ChainId::supported_chains();

      for chain in chains {
         match chain {
            ChainId::Ethereum => map.insert(chain.id(), NumericValue::parse_to_gwei("0.01")),
            ChainId::EthereumSepolia => map.insert(chain.id(), NumericValue::parse_to_gwei("0.01")),
            ChainId::Optimism => map.insert(chain.id(), NumericValue::parse_to_gwei("0.001")),
            ChainId::BinanceSmartChain => map.insert(chain.id(), NumericValue::parse_to_gwei("0")),
            ChainId::Base => map.insert(chain.id(), NumericValue::parse_to_gwei("0.001")),
            ChainId::Arbitrum => map.insert(chain.id(), NumericValue::parse_to_gwei("0.001")),
            ChainId::RobinHood => map.insert(chain.id(), NumericValue::parse_to_gwei("0.01")),
         };
      }

      Self { fee: map }
   }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Dapp {
   Across,
   Uniswap,
}

impl Dapp {
   pub fn is_across(&self) -> bool {
      matches!(self, Self::Across)
   }

   pub fn is_uniswap(&self) -> bool {
      matches!(self, Self::Uniswap)
   }
}

#[derive(Debug, Clone, Default)]
pub struct ConnectedDapps {
   pub dapps: Vec<String>,
}

impl ConnectedDapps {
   pub fn connected_dapps(&self) -> Vec<String> {
      self.dapps.clone()
   }

   pub fn connect_dapp(&mut self, dapp: String) {
      self.dapps.push(dapp);
   }

   pub fn disconnect_dapp(&mut self, dapp: &str) {
      self.dapps.retain(|d| d != dapp);
   }

   pub fn disconnect_all(&mut self) {
      self.dapps.clear();
   }

   pub fn is_connected(&self, dapp: &str) -> bool {
      self.dapps.contains(&dapp.to_string())
   }
}

/// One app's accounts: the one it currently sees, plus every one it has been
/// given before.
///
/// An app is given a dedicated account so it cannot correlate the user's
/// activity with apps connected to other accounts. Every account the app was
/// given is kept, not just the latest, because the app still knows the earlier
/// ones — the connect prompt has to be able to say "this wallet has been
/// connected to this app before" for each of them, not only for the last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DappConnection {
   /// The account handed out the next time this app connects.
   current: Address,
   /// Every account this app has been given, in the order they were given.
   seen: Vec<Address>,
}

impl DappConnection {
   fn new(address: Address) -> Self {
      Self {
         current: address,
         seen: vec![address],
      }
   }

   /// Make `address` the account the app currently sees, remembering it if the
   /// app has not been given it before.
   fn record(&mut self, address: Address) {
      self.current = address;

      if !self.seen.contains(&address) {
         self.seen.push(address);
      }
   }

   pub fn current(&self) -> Address {
      self.current
   }

   /// Every account this app has been given, oldest first.
   pub fn seen(&self) -> &[Address] {
      &self.seen
   }

   /// Drop accounts that are no longer wallets, and answer whether any are left.
   ///
   /// The account the app currently sees moves to the most recent one it still
   /// has, so an app is never remembered against a deleted wallet.
   fn retain_wallets(&mut self, wallets: &HashSet<Address>) -> bool {
      self.seen.retain(|address| wallets.contains(address));

      if self.seen.is_empty() {
         return false;
      }

      if !wallets.contains(&self.current) {
         if let Some(last_seen) = self.seen.last() {
            self.current = *last_seen
         } else {
            tracing::warn!("Last seen is empty")
         }
      }

      true
   }
}

/// The accounts Zeus exposed to each app, keyed by the app origin.
///
/// The mapping outlives a disconnect: reconnecting an app should offer the
/// account it had before rather than whichever account happens to be selected
/// at the time.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DappAccounts {
   pub accounts: HashMap<String, DappConnection>,
}

impl DappAccounts {
   /// The account `origin` currently sees.
   pub fn get(&self, origin: &str) -> Option<Address> {
      self.accounts.get(origin).map(DappConnection::current)
   }

   /// Every account `origin` has been given, oldest first.
   pub fn seen(&self, origin: &str) -> &[Address] {
      self.accounts.get(origin).map_or(&[], DappConnection::seen)
   }

   /// Record that `origin` was connected with `address`, making it the account
   /// the app sees from now on and adding it to the ones the app was given.
   pub fn record(&mut self, origin: &str, address: Address) {
      self
         .accounts
         .entry(origin.to_string())
         .and_modify(|connection| connection.record(address))
         .or_insert_with(|| DappConnection::new(address));
   }

   /// Forget every account that is no longer a wallet, dropping origins left
   /// with none. Returns the number of origins forgotten.
   pub fn retain_wallets(&mut self, wallets: &HashSet<Address>) -> usize {
      let before = self.accounts.len();
      self.accounts.retain(|_, connection| connection.retain_wallets(wallets));
      before - self.accounts.len()
   }
}

/// Holds addresses that are delegated to a smart contract
#[derive(Debug, Clone)]
pub struct DelegatedWallets {
   /// Map of (chain, account) to delegated address
   pub map: HashMap<(u64, Address), Address>,
   /// Last time we checked the smart account status
   /// Time is in UNIX timestamp
   pub last_check: HashMap<(u64, Address), u64>,
}

impl DelegatedWallets {
   pub fn new() -> Self {
      Self {
         map: HashMap::new(),
         last_check: HashMap::new(),
      }
   }

   pub fn add(&mut self, chain: u64, account: Address, delegated_address: Address) {
      self.map.insert((chain, account), delegated_address);
   }

   pub fn remove(&mut self, chain: u64, account: Address) {
      self.map.remove(&(chain, account));
   }

   pub fn should_check(&self, chain: u64, account: Address) -> bool {
      let now = TimeStamp::now_as_secs().unwrap_or_default().timestamp();
      let last_check = self.last_check.get(&(chain, account)).cloned();
      if last_check.is_none() {
         return true;
      }

      let last_check = last_check.unwrap();
      let time_passed = now.saturating_sub(last_check);
      time_passed > DELEGATE_WALLET_CHECK_TIMEOUT
   }

   pub fn get(&self, chain: u64, account: Address) -> Option<Address> {
      self.map.get(&(chain, account)).cloned()
   }
}

pub struct RailgunStatus {
   /// An operation in progress like shield/unshield etc..
   pub op_in_progress: HashMap<u64, bool>,
   pub circuits_download_in_progress: bool,
   pub resync_in_progress: HashMap<u64, bool>,
   pub loading_db_in_progress: HashMap<u64, bool>,
   pub sync_in_progress: HashMap<u64, bool>,
   pub railgun_synced: HashMap<u64, bool>,
   pub railgun_synced_block: HashMap<u64, u64>,
   pub railgun_sync_error: HashMap<u64, String>,
   pub ui_can_check: HashMap<u64, bool>,
}

impl RailgunStatus {
   pub fn new() -> Self {
      Self {
         op_in_progress: HashMap::new(),
         circuits_download_in_progress: false,
         resync_in_progress: HashMap::new(),
         loading_db_in_progress: HashMap::new(),
         sync_in_progress: HashMap::new(),
         railgun_synced: HashMap::new(),
         railgun_synced_block: HashMap::new(),
         railgun_sync_error: HashMap::new(),
         ui_can_check: HashMap::new(),
      }
   }

   pub fn for_testing() -> Self {
      let mut railgun_synced = HashMap::new();
      railgun_synced.insert(1, true);

      let mut railgun_synced_block = HashMap::new();
      railgun_synced_block.insert(1, 25594344);

      Self {
         op_in_progress: HashMap::new(),
         circuits_download_in_progress: false,
         resync_in_progress: HashMap::new(),
         loading_db_in_progress: HashMap::new(),
         sync_in_progress: HashMap::new(),
         railgun_synced,
         railgun_synced_block,
         railgun_sync_error: HashMap::new(),
         ui_can_check: HashMap::new(),
      }
   }

   pub fn op_in_progress(&self, chain: u64) -> bool {
      self.op_in_progress.get(&chain).cloned().unwrap_or(false)
   }

   pub fn loading_db_in_progress(&self, chain: u64) -> bool {
      self.loading_db_in_progress.get(&chain).cloned().unwrap_or(false)
   }

   pub fn set_loading_db_in_progress(&mut self, chain: u64, in_progress: bool) {
      self.loading_db_in_progress.insert(chain, in_progress);
   }

   pub fn circuits_download_in_progress(&self) -> bool {
      self.circuits_download_in_progress
   }

   pub fn set_circuits_download_in_progress(&mut self, in_progress: bool) {
      self.circuits_download_in_progress = in_progress;
   }

   pub fn set_op_in_progress(&mut self, chain: u64, in_progress: bool) {
      self.op_in_progress.insert(chain, in_progress);
   }

   pub fn resync_in_progress(&self, chain: u64) -> bool {
      self.resync_in_progress.get(&chain).cloned().unwrap_or(false)
   }

   pub fn set_resync_in_progress(&mut self, chain: u64, in_progress: bool) {
      self.resync_in_progress.insert(chain, in_progress);
   }

   pub fn set_sync_in_progress(&mut self, chain: u64, in_progress: bool) {
      self.sync_in_progress.insert(chain, in_progress);
   }

   pub fn sync_in_progress(&self, chain: u64) -> bool {
      self.sync_in_progress.get(&chain).cloned().unwrap_or(false)
   }

   pub fn synced(&self, chain: u64) -> bool {
      self.railgun_synced.get(&chain).cloned().unwrap_or(false)
   }

   pub fn synced_block(&self, chain: u64) -> u64 {
      self.railgun_synced_block.get(&chain).cloned().unwrap_or(0)
   }

   pub fn sync_error(&self, chain: u64) -> Option<String> {
      self.railgun_sync_error.get(&chain).cloned()
   }

   pub fn is_error_invalid_root(&self, chain: u64) -> bool {
      if let Some(error) = self.sync_error(chain) {
         return error.contains("Invalid root");
      }
      false
   }

   pub fn set_synced(&mut self, chain: u64, synced: bool) {
      self.railgun_synced.insert(chain, synced);
   }

   pub fn set_synced_block(&mut self, chain: u64, block: u64) {
      self.railgun_synced_block.insert(chain, block);
   }

   pub fn set_sync_error(&mut self, chain: u64, error: String) {
      self.railgun_sync_error.insert(chain, error);
   }

   pub fn clear_last_error(&mut self, chain: u64) {
      self.railgun_sync_error.remove(&chain);
   }
}

#[derive(Debug, Default, Clone)]
pub struct WalletInfoCache {
   /// Quickly access a wallet by its address
   pub map: HashMap<Address, WalletInfo>,

   /// Ordered list of wallets
   pub ordered_vec: Vec<WalletInfo>,
}

impl WalletInfoCache {
   pub fn new(map: HashMap<Address, WalletInfo>, ordered_vec: Vec<WalletInfo>) -> Self {
      Self { map, ordered_vec }
   }

   pub fn clear(&mut self) {
      self.map.clear();
      self.ordered_vec.clear();
   }

   pub fn get(&self, address: &Address) -> Option<&WalletInfo> {
      self.map.get(address)
   }

   pub fn ordered_slice(&self) -> &[WalletInfo] {
      &self.ordered_vec
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn autolock_idle_seconds() {
      assert_eq!(AutoLock::OneMinute.idle_secs(), Some(60));
      assert_eq!(AutoLock::TenMinutes.idle_secs(), Some(600));
      assert_eq!(AutoLock::OneHour.idle_secs(), Some(3_600));
      assert_eq!(AutoLock::FourHours.idle_secs(), Some(14_400));
      assert_eq!(AutoLock::Never.idle_secs(), None);
   }

   #[test]
   fn autolock_labels_are_unique() {
      let mut labels: Vec<_> = [
         AutoLock::OneMinute,
         AutoLock::TenMinutes,
         AutoLock::OneHour,
         AutoLock::FourHours,
         AutoLock::Never,
      ]
      .iter()
      .map(|autolock| autolock.label())
      .collect();

      labels.sort_unstable();
      labels.dedup();
      assert_eq!(labels.len(), 5);
   }

   #[test]
   fn autolock_defaults_to_one_hour() {
      assert_eq!(AutoLock::default(), AutoLock::OneHour);
      assert_eq!(
         SecuritySettings::default().autolock,
         AutoLock::OneHour
      );
      assert!(!SecuritySettings::default().autolock_changed);
   }

   /// The persisted fields round-trip; `argon_params` does not, because it is
   /// serde-skipped (it belongs to `vault.data`'s header).
   #[test]
   fn security_settings_seal_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let settings = SecuritySettings {
         autolock: AutoLock::FourHours,
         autolock_changed: true,
         argon_params: Argon2::new(1_024, 2, 2),
      };

      let sealed = key.seal_json(&settings, SECURITY_AAD).unwrap();
      let loaded: SecuritySettings = key.open_json(&sealed, SECURITY_AAD).unwrap();

      assert_eq!(loaded.autolock, AutoLock::FourHours);
      assert!(loaded.autolock_changed);
      assert_eq!(loaded.argon_params, Argon2::default());
   }

   /// A `security.data` written by an older build (or hand-truncated) must still
   /// load, falling back to the defaults for absent fields.
   #[test]
   fn security_settings_missing_fields_use_defaults() {
      let key = WalletStateKey::generate().unwrap();
      let sealed = key.seal_json(&serde_json::json!({}), SECURITY_AAD).unwrap();
      let loaded: SecuritySettings = key.open_json(&sealed, SECURITY_AAD).unwrap();

      assert_eq!(loaded.autolock, AutoLock::OneHour);
      assert!(!loaded.autolock_changed);
   }
}
