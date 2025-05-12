use crate::core::utils::data_dir;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use zeus_eth::{
   alloy_provider::Provider,
   alloy_rpc_types::{BlockNumberOrTag, Filter},
   amm::{DexKind, UniswapV3Pool},
   currency::ERC20Token,
   utils::{
      batch::{self, V3Pool},
      client,
   },
};

const PROVIDERS_FILE: &str = "providers.json";

/// Request per second
pub const CLIENT_RPS: u32 = 10;
/// Max retries
pub const MAX_RETRIES: u32 = 10;
/// Initial backoff
pub const INITIAL_BACKOFF: u64 = 400;
/// Compute units per second
pub const COMPUTE_UNITS_PER_SECOND: u64 = 330;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rpc {
   pub url: String,
   pub chain_id: u64,
   /// False if the rpc is added by the user
   pub default: bool,
   pub enabled: bool,
   /// Be default this is true so we can measure and test the RPCs
   ///
   /// Later on we can set this to false if the RPC is not working
   pub working: bool,
   pub latency: Option<Duration>,
}

impl Rpc {
   pub fn new(url: impl Into<String>, chain_id: u64, default: bool, enabled: bool) -> Self {
      Self {
         url: url.into(),
         chain_id,
         default,
         enabled,
         working: true,
         latency: None,
      }
   }

   pub fn is_ws(&self) -> bool {
      self.url.starts_with("ws")
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
   /// Rpc endpoints by chain id
   pub rpcs: HashMap<u64, Vec<Rpc>>,
   /// Flag to disable the usage of default RPCs
   pub defaults_disabled: HashMap<u64, bool>,
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

   pub fn is_defaults_disabled(&self, chain: u64) -> bool {
      self.defaults_disabled.get(&chain).cloned().unwrap_or(false)
   }

   pub fn disable_defaults(&mut self, chain: u64) {
      // only disable the defaults if there is an rpc added by the user
      if let Ok(_) = self.get_fastest_user(chain) {
         for rpc in self.rpcs.get_mut(&chain).unwrap() {
            rpc.enabled = false;
         }
         self.defaults_disabled.insert(chain, true);
      }
   }

   pub fn enable_defaults(&mut self, chain: u64) {
      for rpc in self.rpcs.get_mut(&chain).unwrap() {
         rpc.enabled = true;
      }
      self.defaults_disabled.remove(&chain);
   }

   pub fn reset_latency(&mut self) {
      self.rpcs.iter_mut().for_each(|(_, rpcs)| {
         rpcs.iter_mut().for_each(|rpc| {
            rpc.latency = None;
         })
      });
   }

   pub fn reset_working(&mut self) {
      self.rpcs.iter_mut().for_each(|(_, rpcs)| {
         rpcs.iter_mut().for_each(|rpc| {
            rpc.working = true;
         })
      });
   }

   /// Add a user-provided RPC for a chain
   pub fn add_user_rpc(&mut self, chain_id: u64, url: String) {
      let new_rpc = Rpc::new(url.clone(), chain_id, false, true);
      let rpcs = self.get_all(chain_id);
      if rpcs.iter().any(|rpc| rpc.url == url) {
         return;
      } else {
         self
            .rpcs
            .entry(chain_id)
            .or_insert_with(Vec::new)
            .push(new_rpc);
      }
   }

   pub fn remove_rpc(&mut self, chain_id: u64, url: String) {
      self
         .rpcs
         .entry(chain_id)
         .or_insert_with(Vec::new)
         .retain(|rpc| rpc.url != url);
   }

   pub fn rpc_mut(&mut self, chain_id: u64, url: String) -> Option<&mut Rpc> {
      self
         .rpcs
         .get_mut(&chain_id)?
         .iter_mut()
         .find(|rpc| rpc.url == url)
   }

   /// Get the fastest RPC for a chain from the default list
   pub fn get_fastest(&self, chain_id: u64) -> Result<Rpc, anyhow::Error> {
      let rpcs = self
         .rpcs
         .get(&chain_id)
         .ok_or_else(|| anyhow!("No RPCs found for chain id {}", chain_id))?;
      let mut sorted_rpcs = rpcs
         .iter()
         .filter(|rpc| rpc.default && rpc.enabled && rpc.working)
         .collect::<Vec<_>>();
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
      let mut sorted_rpcs = rpcs
         .iter()
         .filter(|rpc| !rpc.default && rpc.enabled && rpc.working)
         .collect::<Vec<_>>();
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

   /// Get all RPCs for a chain from fastest to slowest
   pub fn get_all_fastest(&self, chain_id: u64) -> Vec<Rpc> {
      let mut rpcs = self.get_all(chain_id);
      rpcs.sort_by(|a, b| {
         let a_is_default = a.default;
         let b_is_default = b.default;

         if a_is_default != b_is_default {
            return if a_is_default {
               std::cmp::Ordering::Greater
            } else {
               std::cmp::Ordering::Less
            };
         }

         a.latency
            .unwrap_or(Duration::default())
            .partial_cmp(&b.latency.unwrap_or(Duration::default()))
            .unwrap_or(std::cmp::Ordering::Equal)
      });
      rpcs
   }

   pub fn get_all_fastest_user(&self, chain_id: u64) -> Vec<Rpc> {
      let rpcs = self.get_all_fastest(chain_id);
      let mut user_rpcs = Vec::new();
      for rpc in &rpcs {
         if !rpc.default {
            user_rpcs.push(rpc.clone());
         }
      }
      user_rpcs.sort_by(|a, b| {
         a.latency
            .unwrap_or(Duration::default())
            .partial_cmp(&b.latency.unwrap_or(Duration::default()))
            .unwrap_or(std::cmp::Ordering::Equal)
      });
      user_rpcs
   }

   /// Get all RPCs for a chain
   pub fn get_all(&self, chain_id: u64) -> Vec<Rpc> {
      self.rpcs.get(&chain_id).unwrap_or(&vec![]).to_vec()
   }
}

impl Default for RpcProviders {
   fn default() -> Self {
      let mut rpcs = HashMap::new();

      // Commented out RPCs that are not supporting at least one eth method on their public endpoint
      // For example some of the RPCs are not supporting eth_getLogs
      // TODO: make a blacklist?

      // Chain ID 1: Ethereum

      /*
      Rpc::new("wss://eth.drpc.org", 1, true, true),
      Rpc::new("https://eth-mainnet.public.blastapi.io", 1, true, true),
      Rpc::new("https://eth-pokt.nodies.app", 1, true, true),
      Rpc::new("https://eth.llamarpc.com", 1, true, true),
      Rpc::new("https://1rpc.io/eth", 1, true, true),

       */

      rpcs.insert(
         1,
         vec![
            Rpc::new("wss://eth.merkle.io", 1, true, true),
            Rpc::new("wss://ethereum-rpc.publicnode.com", 1, true, true),
            Rpc::new("wss://mainnet.gateway.tenderly.co", 1, true, true),
            Rpc::new("wss://0xrpc.io/eth", 1, true, true),
            Rpc::new("https://rpc.payload.de", 1, true, true),
            Rpc::new("https://eth.merkle.io", 1, true, true),
            Rpc::new(
               "https://ethereum-rpc.publicnode.com",
               1,
               true,
               true,
            ),
            Rpc::new("https://rpc.mevblocker.io", 1, true, true),
            Rpc::new("https://rpc.flashbots.net", 1, true, true),
            Rpc::new("https://rpc.flashbots.net/fast", 1, true, true),
         ],
      );

      // Chain ID 10: Optimism

      /*
      Rpc::new("https://optimism-mainnet.public.blastapi.io", 10, true, true),
                  Rpc::new("https://1rpc.io/op", 10, true, true),
                              Rpc::new("https://op-pokt.nodies.app", 10, true, true),
      */

      rpcs.insert(
         10,
         vec![
            Rpc::new(
               "wss://optimism.gateway.tenderly.co",
               10,
               true,
               true,
            ),
            Rpc::new("wss://optimism.drpc.org", 10, true, true),
            Rpc::new(
               "wss://optimism-rpc.publicnode.com",
               10,
               true,
               true,
            ),
            Rpc::new("wss://0xrpc.io/op", 10, true, true),
            Rpc::new(
               "https://optimism.blockpi.network/v1/rpc/public",
               10,
               true,
               true,
            ),
            Rpc::new("https://mainnet.optimism.io", 10, true, true),
            Rpc::new(
               "https://optimism-rpc.publicnode.com",
               10,
               true,
               true,
            ),
            Rpc::new("https://optimism.drpc.org", 10, true, true),
         ],
      );

      // Chain ID 56: BSC

      /*
      Rpc::new("https://bsc-mainnet.public.blastapi.io", 56, true, true),
      Rpc::new("https://binance.llamarpc.com", 56, true, true),
      Rpc::new("https://bsc-dataseed.bnbchain.org", 56, true, true),
      Rpc::new("https://bsc.drpc.org", 56, true, true),
                  Rpc::new("https://bsc-pokt.nodies.app", 56, true, true),
       */

      rpcs.insert(
         56,
         vec![
            Rpc::new("wss://bsc-rpc.publicnode.com", 56, true, true),
            Rpc::new("wss://0xrpc.io/bnb", 56, true, true),
            Rpc::new("https://bsc.blockrazor.xyz", 56, true, true),
            Rpc::new("https://rpc-bsc.48.club", 56, true, true),
         ],
      );

      // Chain ID 8453: Base

      /*
                  Rpc::new("https://base.llamarpc.com", 8453, true, true),
      Rpc::new(
               "https://base.api.onfinality.io/public",
               8453,
               true,
               true,
            ),
      Rpc::new("https://base-mainnet.public.blastapi.io", 8453, true, true),
                  Rpc::new(
               "https://developer-access-mainnet.base.org",
               8453,
               true,
               true,
            ),
            Rpc::new("https://base-pokt.nodies.app", 8453, true, true),

       */
      rpcs.insert(
         8453,
         vec![
            Rpc::new("wss://base-rpc.publicnode.com", 8453, true, true),
            Rpc::new("wss://0xrpc.io/base", 8453, true, true),
            Rpc::new(
               "wss://base.gateway.tenderly.co",
               8453,
               true,
               false,
            ),
            Rpc::new("https://mainnet.base.org", 8453, true, true),
            Rpc::new("https://1rpc.io/base", 8453, true, true),
            Rpc::new(
               "https://base-rpc.publicnode.com",
               8453,
               true,
               true,
            ),
         ],
      );

      // Chain ID 42161: Arbitrum

      /*
        Rpc::new("https://arb-pokt.nodies.app", 42161, true, true),
        Rpc::new("https://arbitrum-one-rpc.publicnode.com", 42161, true, true),
        Rpc::new("https://arbitrum-one.public.blastapi.io", 42161, true, true),
                    Rpc::new(
              "wss://arbitrum.callstaticrpc.com",
              42161,
              true,
              true,
           ),

      */
      rpcs.insert(
         42161,
         vec![
            Rpc::new(
               "wss://arbitrum-one-rpc.publicnode.com",
               42161,
               true,
               true,
            ),
            Rpc::new("https://arbitrum.meowrpc.com", 42161, true, true),
            Rpc::new("https://arb1.arbitrum.io/rpc", 42161, true, true),
            Rpc::new("https://1rpc.io/arb", 42161, true, true),
         ],
      );

      RpcProviders {
         rpcs,
         defaults_disabled: HashMap::new(),
      }
   }
}

pub async fn client_test(rpc: Rpc) -> Result<(), anyhow::Error> {
   let retry = client::retry_layer(
      MAX_RETRIES,
      INITIAL_BACKOFF,
      COMPUTE_UNITS_PER_SECOND,
   );
   let throttle = client::throttle_layer(CLIENT_RPS);
   let client = client::get_client(&rpc.url, retry, throttle).await?;

   let block = client.get_block_number().await?;
   let weth = ERC20Token::wrapped_native_token(rpc.chain_id);
   // Some providers do require an address to set for the filter
   let filter = Filter::new()
      .address(weth.address)
      .from_block(BlockNumberOrTag::Number(block));
   let _ = client.get_logs(&filter).await?;

   let usdc = ERC20Token::usdc_from_chain(rpc.chain_id);
   let dex = DexKind::UniswapV3;
   let factory = dex.factory(rpc.chain_id)?;

   let mut v3_pools = Vec::new();
   let pools = batch::get_v3_pools(
      client.clone(),
      weth.address,
      usdc.address,
      factory,
   )
   .await?;

   for pool in &pools {
      if !pool.addr.is_zero() {
         let fee: u32 = pool.fee.to_string().parse()?;
         v3_pools.push(UniswapV3Pool::new(
            rpc.chain_id,
            pool.addr,
            fee,
            weth.clone(),
            usdc.clone(),
            dex,
         ))
      }
   }

   let mut pools_info = Vec::new();
   for pool in &v3_pools {
      pools_info.push(V3Pool {
         pool: pool.address,
         base_token: weth.address,
         tickSpacing: pool.fee.tick_spacing(),
      });
   }

   for pools in pools_info.chunks(3) {
      let _ = batch::get_v3_state(client.clone(), None, pools.to_vec()).await?;
   }

   // request the latest block number 25 times concurrently
   // if the throttle and retry layers are working correctly
   // this should not fail
   let mut tasks: Vec<tokio::task::JoinHandle<Result<u64, anyhow::Error>>> = Vec::new();
   for _ in 0..24 {
      let client = client.clone();
      let task = tokio::task::spawn(async move {
         let block = client.get_block_number().await;
         block.map_err(|e| anyhow!("Error getting block number: {}", e))
      });
      tasks.push(task);
   }

   for task in tasks {
      task.await??;
   }

   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::utils::RT;
   use zeus_eth::types::SUPPORTED_CHAINS;

   #[tokio::test]
   async fn test_ws_providers() {
      let rpc = RpcProviders::default();

      let mut tasks = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let rpcs = rpc.get_all(chain);

         for rpc in rpcs {
            if !rpc.is_ws() {
               continue;
            }
            let rpc_clone = rpc.clone();
            let task = RT.spawn(async move {
               match client_test(rpc_clone.clone()).await {
                  Ok(_) => {
                     println!("RPC {} PASSED", rpc_clone.url);
                  }
                  Err(e) => {
                     println!("RPC {} failed: {:?}", rpc_clone.url, e);
                  }
               }
            });
            tasks.push(task);
         }
      }

      for task in tasks {
         task.await.unwrap();
      }
   }

   #[tokio::test]
   async fn test_providers() {
      let rpc = RpcProviders::default();

      let mut tasks = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let rpcs = rpc.get_all(chain);

         for rpc in rpcs {
            let rpc_clone = rpc.clone();
            let task = RT.spawn(async move {
               match client_test(rpc_clone.clone()).await {
                  Ok(_) => {
                     println!("RPC {} PASSED", rpc_clone.url);
                  }
                  Err(e) => {
                     println!("RPC {} failed: {:?}", rpc_clone.url, e);
                  }
               }
            });
            tasks.push(task);
         }
      }

      for task in tasks {
         let _ = task.await;
      }
   }
}
