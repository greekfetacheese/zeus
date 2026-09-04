use std::collections::{HashMap, HashSet};
use zeus_eth::{
   alloy_primitives::{Address, Bytes},
   types::ETH_SEPOLIA,
   types::{ChainId, SUPPORTED_CHAINS},
   utils::NumericValue,
};

use crate::core::{
   WalletInfo,
   context::{DELEGATE_WALLET_CHECK_TIMEOUT, disabled_chains_dir, railgun_config_dir},
};
use crate::utils::{TimeStamp, write_private};

use zeus_railgun::indexer::syncer::rpc::{
   DEFAULT_BLOCK_RANGE, DEFAULT_CONCURRENCY, SEPOLIA_BLOCK_RANGE,
};

use serde::{Deserialize, Serialize};

const DEFAULT_STATE_UPDATE_INTERVAL_MINUTES: u64 = 5;
const MIN_STATE_UPDATE_INTERVAL_MINUTES: u64 = 1;
const MAX_STATE_UPDATE_INTERVAL_MINUTES: u64 = 60;

fn default_state_update_interval_minutes() -> u64 {
   DEFAULT_STATE_UPDATE_INTERVAL_MINUTES
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RailgunConfig {
   pub rpc_syncer_concurrency: usize,
   pub rpc_syncer_block_range: HashMap<u64, u64>,
   #[serde(default)]
   pub enabled: HashMap<u64, bool>,
   /// How often to sync Railgun state, in minutes (1–60).
   #[serde(default = "default_state_update_interval_minutes")]
   pub state_update_interval_minutes: u64,
}

impl RailgunConfig {
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

#[derive(Default, Clone, Debug)]
pub struct Recipient {
   pub name: Option<String>,
   pub evm_address: String,
   pub zk_address: String,
}

impl Recipient {
   pub fn from_unknown_evm_address(address: Address) -> Self {
      Self {
         name: None,
         evm_address: address.to_string(),
         zk_address: String::new(),
      }
   }

   pub fn from_unknown_zk_address(address: String) -> Self {
      Self {
         name: None,
         evm_address: String::new(),
         zk_address: address,
      }
   }

   pub fn from_wallet_info(wallet_info: WalletInfo) -> Self {
      Self {
         name: Some(wallet_info.name_with_source()),
         evm_address: wallet_info.address.to_string(),
         zk_address: wallet_info.zk_address(),
      }
   }

   pub fn from_contact(contact: Contact) -> Self {
      Self {
         name: Some(contact.name),
         evm_address: contact.evm_address,
         zk_address: contact.zk_address,
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
         None => "zkAddress not available".to_string(),
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
      // Eth
      map.insert(1, NumericValue::parse_to_gwei("1"));

      // Optimism
      map.insert(10, NumericValue::parse_to_gwei("0.002"));

      // BSC (Legacy Tx)
      map.insert(56, NumericValue::parse_to_gwei("0"));

      // Base
      map.insert(8453, NumericValue::parse_to_gwei("0.002"));

      // Arbitrum (Legacy Tx)
      map.insert(42161, NumericValue::parse_to_gwei("0"));

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
