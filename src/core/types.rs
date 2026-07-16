use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use zeus_eth::{
   alloy_primitives::{Address, Bytes},
   types::SUPPORTED_CHAINS,
   utils::NumericValue,
};

use crate::core::{
   WalletInfo,
   context::{DELEGATE_WALLET_CHECK_TIMEOUT, delegated_wallets_dir},
};

use super::serde_hashmap;
use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegatedWallets {
   #[serde(with = "serde_hashmap")]
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

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = delegated_wallets_dir()?;
      let data = std::fs::read(dir)?;
      let smart_accounts = serde_json::from_slice(&data)?;
      Ok(smart_accounts)
   }

   pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
      let data = serde_json::to_string(self)?;
      let dir = delegated_wallets_dir()?;
      std::fs::write(dir, data)?;
      Ok(())
   }

   pub fn add(&mut self, chain: u64, account: Address, delegated_address: Address) {
      self.map.insert((chain, account), delegated_address);
   }

   pub fn remove(&mut self, chain: u64, account: Address) {
      self.map.remove(&(chain, account));
   }

   pub fn should_check(&self, chain: u64, account: Address) -> bool {
      let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
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
