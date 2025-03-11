use crate::core::utils::data_dir;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

const PROVIDERS_FILE: &str = "providers.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rpc {
   pub url: String,
   pub chain_id: u64,
   /// False if the rpc is added by the user
   pub default: bool,
   pub enabled: bool,
   pub latency: Option<Duration>,
}

impl Rpc {
   pub fn new(url: impl Into<String>, chain_id: u64, default: bool, enabled: bool) -> Self {
      Self {
         url: url.into(),
         chain_id,
         default,
         enabled,
         latency: None,
      }
   }

   pub fn latency_ms(&self) -> u128 {
      if let Some(latency) = self.latency {
         latency.as_millis()
      } else {
         0
      }
   }

   pub fn latency_str(&self) -> String {
      if let Some(latency) = self.latency {
         format!("{}ms", latency.as_millis())
      } else {
         "N/A".to_string()
      }
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcProviders {
   pub rpcs: HashMap<u64, Vec<Rpc>>,
}

impl RpcProviders {
   /// Load RPC providers from a file
   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(PROVIDERS_FILE);
      let data = std::fs::read(dir)?;
      let providers = serde_json::from_slice(&data)?;
      Ok(providers)
   }

   /// Save RPC providers to a file
   pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
      let providers = serde_json::to_vec(&self)?;
      let dir = data_dir()?.join(PROVIDERS_FILE);
      std::fs::write(dir, providers)?;
      Ok(())
   }

   /// Add a user-provided RPC for a chain
   pub fn add_user_rpc(&mut self, chain_id: u64, url: String) {
      let new_rpc = Rpc::new(url.clone(), chain_id, false, true);
      let rpcs = self.get_all(chain_id);
      if rpcs.iter().any(|rpc| rpc.url == url) {
         return;
      } else {
         self.rpcs.entry(chain_id).or_insert_with(Vec::new).push(new_rpc);
      }
   }

   pub fn remove_rpc(&mut self, chain_id: u64, url: String) {
      self.rpcs.entry(chain_id).or_insert_with(Vec::new).retain(|rpc| rpc.url != url);
   }

   pub fn rpc_mut(&mut self, chain_id: u64, url: String) -> Option<&mut Rpc> {
      self.rpcs.get_mut(&chain_id)?.iter_mut().find(|rpc| rpc.url == url)
   }

   /// Get the fastest RPC for a chain from the default list
   pub fn get_fastest(&self, chain_id: u64) -> Result<Rpc, anyhow::Error> {
      let rpcs = self
         .rpcs
         .get(&chain_id)
         .ok_or_else(|| anyhow!("No RPCs found for chain id {}", chain_id))?;
      let mut sorted_rpcs = rpcs.iter().filter(|rpc| rpc.default).collect::<Vec<_>>();
      sorted_rpcs.sort_by(|a, b| {
         // Sort by latency, treating unmeasured (None) as slower than measured
         a.latency
            .unwrap_or(Duration::MAX)
            .cmp(&b.latency.unwrap_or(Duration::MAX))
      });
      sorted_rpcs
         .first()
         .cloned()
         .ok_or_else(|| anyhow!("No RPCs available for chain id {}", chain_id))
         .cloned()
   }

   /// Get the fastest RPC added by the user for a chain
   pub fn get_fastest_user(&self, chain_id: u64) -> Result<Rpc, anyhow::Error> {
      let rpcs = self
         .rpcs
         .get(&chain_id)
         .ok_or_else(|| anyhow!("No RPCs available for chain id {}", chain_id))?;
      let mut sorted_rpcs = rpcs.iter().filter(|rpc| !rpc.default && rpc.enabled).collect::<Vec<_>>();
      sorted_rpcs.sort_by(|a, b| {
         a.latency
            .unwrap_or(Duration::MAX)
            .cmp(&b.latency.unwrap_or(Duration::MAX))
      });
      sorted_rpcs
         .first()
         .cloned()
         .ok_or_else(|| anyhow!("No RPCs available for chain id {}", chain_id))
         .cloned()
   }

   /// Get an RPC by its chain id
   /// 
   /// If the user has added a custom RPC for the chain, it will be returned first else the fastest default RPC will be returned
   pub fn get_rpc(&self, chain_id: u64) -> Result<Rpc, anyhow::Error> {
      if let Ok(rpc) = self.get_fastest_user(chain_id) {
         Ok(rpc)
      } else {
         self.get_fastest(chain_id)
      }
   }

   /// Get all RPCs for a chain regardless of everything
   pub fn get_all(&self, chain_id: u64) -> Vec<Rpc> {
      self.rpcs.get(&chain_id).unwrap_or(&vec![]).to_vec()
   }

}

impl Default for RpcProviders {
   fn default() -> Self {
      let mut rpcs = HashMap::new();

      // Chain ID 1: Ethereum
      rpcs.insert(
         1,
         vec![
            Rpc::new("https://eth.merkle.io", 1, true, true),
            Rpc::new("https://eth.llamarpc.com", 1, true, true),
            Rpc::new("https://ethereum-rpc.publicnode.com", 1, true, true),
            Rpc::new("https://rpc.mevblocker.io", 1, true, true),
            Rpc::new("https://rpc.flashbots.net", 1, true, true),
         ],
      );

      // Chain ID 10: Optimism
      rpcs.insert(
         10,
         vec![
            Rpc::new("https://optimism.llamarpc.com", 10, true, true),
            Rpc::new("https://mainnet.optimism.io", 10, true, true),
            Rpc::new("https://1rpc.io/op", 10, true, true),
            Rpc::new("https://optimism-rpc.publicnode.com", 10, true, true),
            Rpc::new("https://optimism.drpc.org", 10, true, true),
         ],
      );

      // Chain ID 56: BSC
      rpcs.insert(
         56,
         vec![
            Rpc::new("https://bsc.drpc.org", 56, true, true),
            Rpc::new("https://binance.llamarpc.com", 56, true, true),
            Rpc::new("https://bsc-pokt.nodies.app", 56, true, true),
            Rpc::new("https://bsc-dataseed.bnbchain.org", 56, true, true),
         ],
      );

      // Chain ID 8453: Base
      rpcs.insert(
         8453,
         vec![
            Rpc::new("https://base.llamarpc.com", 8453, true, true),
            Rpc::new("https://mainnet.base.org", 8453, true, true),
            Rpc::new("https://developer-access-mainnet.base.org", 8453, true, true),
            Rpc::new("https://1rpc.io/base", 8453, true, true),
            Rpc::new("https://base-pokt.nodies.app", 8453, true, true),
            Rpc::new("https://base-rpc.publicnode.com", 8453, true, true),
         ],
      );

      // Chain ID 42161: Arbitrum
      rpcs.insert(
         42161,
         vec![
            Rpc::new("https://arbitrum.llamarpc.com", 42161, true, true),
            Rpc::new("https://arb1.arbitrum.io/rpc", 42161, true, true),
            Rpc::new("https://1rpc.io/arb", 42161, true, true),
            Rpc::new("https://arb-pokt.nodies.app", 42161, true, true),
         ],
      );

      RpcProviders { rpcs }
   }
}