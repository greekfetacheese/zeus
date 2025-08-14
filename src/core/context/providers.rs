use crate::core::{ZeusCtx, utils::data_dir};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use zeus_eth::{
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, BlockNumberOrTag, Filter},
   amm::uniswap::UniswapPool,
   currency::ERC20Token,
   utils::{batch, client},
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
   /// True if the rpc is functional at all
   pub working: bool,
   /// True if the rpc is fully functional
   /// All requests should work perfect
   /// 
   /// If false, some things like batch requests may not work
   pub fully_functional: bool,
   pub archive: bool,
   pub mev_protect: bool,
   pub latency: Option<Duration>,
}

impl Rpc {
   pub fn new(
      url: impl Into<String>,
      chain_id: u64,
      default: bool,
      enabled: bool,
      mev_protect: bool,
   ) -> Self {
      Self {
         url: url.into(),
         chain_id,
         default,
         enabled,
         working: false,
         fully_functional: false,
         archive: false,
         mev_protect,
         latency: None,
      }
   }

   pub fn is_ws(&self) -> bool {
      self.url.starts_with("ws")
   }

   pub fn is_archive(&self) -> bool {
      self.archive
   }

   pub fn is_working(&self) -> bool {
      self.working
   }

   pub fn is_fully_functional(&self) -> bool {
      self.fully_functional
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

   #[cfg(test)]
   pub fn all_working(&mut self) {
      self.rpcs.iter_mut().for_each(|(_, rpcs)| {
         rpcs.iter_mut().for_each(|rpc| {
            rpc.working = true;
         })
      });
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
            rpc.working = false;
         })
      });
   }

   /// Add a user-provided RPC for a chain
   ///
   /// By default we assume is not mev protect
   pub fn add_user_rpc(&mut self, chain_id: u64, url: String) {
      let new_rpc = Rpc::new(url.clone(), chain_id, false, true, false);
      let rpcs = self.get_all(chain_id);
      if rpcs.iter().any(|rpc| rpc.url == url) {
         return;
      } else {
         self.rpcs.entry(chain_id).or_default().push(new_rpc);
      }
   }

   pub fn remove_rpc(&mut self, chain_id: u64, url: String) {
      self.rpcs.entry(chain_id).or_default().retain(|rpc| rpc.url != url);
   }

   pub fn rpc_mut(&mut self, chain_id: u64, url: String) -> Option<&mut Rpc> {
      self.rpcs.get_mut(&chain_id)?.iter_mut().find(|rpc| rpc.url == url)
   }

   /// Get all RPCs for a chain from fastest to slowest
   pub fn get_all_fastest(&self, chain_id: u64) -> Vec<Rpc> {
      let mut rpcs = self.get_all(chain_id);
      rpcs.sort_by(|a, b| {
         a.latency
            .unwrap_or_default()
            .partial_cmp(&b.latency.unwrap_or_default())
            .unwrap_or(std::cmp::Ordering::Equal)
      });
      rpcs
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

      // Chain ID 1: Ethereum

      /*
      "wss://eth.drpc.org"
      "https://eth-mainnet.public.blastapi.io"
      "https://eth-pokt.nodies.app"
      "https://eth.llamarpc.com"
      "https://1rpc.io/eth"
      "wss://0xrpc.io/eth"
       */

      let not_mev_protect = false;
      let mev_protect = true;

      rpcs.insert(
         1,
         vec![
            Rpc::new(
               "wss://eth.merkle.io",
               1,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "wss://ethereum-rpc.publicnode.com",
               1,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "wss://mainnet.gateway.tenderly.co",
               1,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://reth-ethereum.ithaca.xyz/rpc",
               1,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://rpc.payload.de",
               1,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://eth.merkle.io",
               1,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://ethereum-rpc.publicnode.com",
               1,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://rpc.mevblocker.io",
               1,
               true,
               true,
               mev_protect,
            ),
            Rpc::new(
               "https://rpc.flashbots.net/fast",
               1,
               true,
               true,
               mev_protect,
            ),
         ],
      );

      // Chain ID 10: Optimism

      /*
      "https://optimism-mainnet.public.blastapi.io"
      "https://1rpc.io/op"
      "https://op-pokt.nodies.app
      "wss://0xrpc.io/op"
      "https://optimism.blockpi.network/v1/rpc/public"
      */

      rpcs.insert(
         10,
         vec![
            Rpc::new(
               "wss://optimism.gateway.tenderly.co",
               10,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "wss://optimism.drpc.org",
               10,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "wss://optimism-rpc.publicnode.com",
               10,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://mainnet.optimism.io",
               10,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://optimism-rpc.publicnode.com",
               10,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://optimism.drpc.org",
               10,
               true,
               true,
               not_mev_protect,
            ),
         ],
      );

      // Chain ID 56: BSC

      /*
      "wss://bsc-rpc.publicnode.com"
      "wss://0xrpc.io/bnb"
      "https://bsc.blockrazor.xyz"
      "https://rpc-bsc.48.club"
      "https://bsc-mainnet.public.blastapi.io"
      "https://bsc-dataseed.bnbchain.org"
      "https://bsc.drpc.org"
       */

      rpcs.insert(
         56,
         vec![
            Rpc::new(
               "wss://bsc-rpc.publicnode.com",
               56,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://binance.llamarpc.com",
               56,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://bsc-pokt.nodies.app",
               56,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://api.zan.top/bsc-mainnet",
               56,
               true,
               true,
               not_mev_protect,
            ),
         ],
      );

      // Chain ID 8453: Base

      /*
      "https://base.llamarpc.com"
      "https://base.api.onfinality.io/public"
      "https://base-mainnet.public.blastapi.io"
      "https://developer-access-mainnet.base.org"
      "https://base-pokt.nodies.app"
      "wss://0xrpc.io/base"
       */
      rpcs.insert(
         8453,
         vec![
            Rpc::new(
               "wss://base-rpc.publicnode.com",
               8453,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "wss://base.gateway.tenderly.co",
               8453,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://mainnet.base.org",
               8453,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://1rpc.io/base",
               8453,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://base-rpc.publicnode.com",
               8453,
               true,
               true,
               not_mev_protect,
            ),
         ],
      );

      // Chain ID 42161: Arbitrum

      /*
        "https://arb-pokt.nodies.app"
        "https://arbitrum-one-rpc.publicnode.com"
        "https://arbitrum-one.public.blastapi.io"
        "wss://arbitrum.callstaticrpc.com"
      */
      rpcs.insert(
         42161,
         vec![
            Rpc::new(
               "wss://arbitrum-one-rpc.publicnode.com",
               42161,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://arbitrum.meowrpc.com",
               42161,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://arb1.arbitrum.io/rpc",
               42161,
               true,
               true,
               not_mev_protect,
            ),
            Rpc::new(
               "https://1rpc.io/arb",
               42161,
               true,
               true,
               not_mev_protect,
            ),
         ],
      );

      RpcProviders {
         rpcs,
         defaults_disabled: HashMap::new(),
      }
   }
}

pub struct RpcTestResult {
   pub working: bool,
   pub fully_functional: bool,
   pub archive: bool,
}

/// Try to determine if the given RPC is working
///
/// Eg. Some free endpoints don't support `eth_getLogs` in the free tier
///
/// Others have a very low staticalll gas limit which cause the batch requests to fail
///
/// Returns [`RpcTestResult`]
pub async fn client_test(ctx: ZeusCtx, rpc: Rpc) -> Result<RpcTestResult, anyhow::Error> {
   let retry = client::retry_layer(
      MAX_RETRIES,
      INITIAL_BACKOFF,
      COMPUTE_UNITS_PER_SECOND,
   );

   let throttle = client::throttle_layer(CLIENT_RPS);
   let client = client::get_client(&rpc.url, retry, throttle).await?;

   let latest_block = client.get_block_number().await?;
   let block_to_query = latest_block - 100_000;
   let weth = ERC20Token::wrapped_native_token(rpc.chain_id);

   let mut result = RpcTestResult {
      working: true,
      fully_functional: true,
      archive: true,
   };

   // For MEV protect RPCs just check if its archive or not
   if rpc.mev_protect {
      let old_block = client
         .get_block(BlockId::Number(BlockNumberOrTag::Number(
            block_to_query,
         )))
         .await;

      match old_block {
         Ok(old_block) => {
            if old_block.is_some() {
               tracing::debug!("{} is Archive Node", rpc.url);
               return Ok(result);
            } else {
               tracing::debug!("{} is NOT Archive Node", rpc.url);
               result.archive = false;
               return Ok(result);
            }
         }
         Err(e) => {
            tracing::debug!("Error getting historical block: {:?}", e);
            result.archive = false;
            result.fully_functional = false;
            result.working = false;
            return Ok(result);
         }
      }
   }

   // Some providers do require an address to set for the filter
   let filter = Filter::new()
      .address(weth.address)
      .from_block(BlockNumberOrTag::Number(latest_block));

   let logs = client.get_logs(&filter).await;

   if logs.is_err() {
      result.fully_functional = false;
   }

   // request the latest block number 25 times concurrently
   // if the throttle and retry layers are working correctly
   // this should not fail
   // For some endpoints this actually fails
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
      let task = task.await;
      if task.is_err() {
         result.fully_functional = false;
      }
   }

   // Query an old block to determine if the RPC is archive or not
   // Since there is no official api for that this is an educated guess
   let old_block = client
      .get_block(BlockId::Number(BlockNumberOrTag::Number(
         block_to_query,
      )))
      .await;

   let is_archive = match old_block {
      Ok(old_block) => {
         if old_block.is_some() {
            tracing::debug!("{} is Archive Node", rpc.url);
            true
         } else {
            tracing::debug!("{} is NOT Archive Node", rpc.url);
            false
         }
      }
      Err(e) => {
         tracing::debug!("Error getting historical block: {:?}", e);
         false
      }
   };

   result.archive = is_archive;

   // This is actually important, A lot of providers have a very low staticalll gas limit
   // and requests like batch fetching the state for V3 pools can fail
   let pool_manager = ctx.pool_manager();

   let v3_pools = pool_manager.get_v3_pools_for_chain(rpc.chain_id);

   if v3_pools.len() >= 10 {
      let mut pools_to_update = Vec::new();
      for pool in &v3_pools {
         if pools_to_update.len() >= 10 {
            break;
         }
         pools_to_update.push(batch::V3Pool {
            pool: pool.address(),
            token0: pool.currency0().address(),
            token1: pool.currency1().address(),
            tickSpacing: pool.fee().tick_spacing(),
         });
      }

      let state_data_res = batch::get_v3_state(client, None, pools_to_update).await;
      if state_data_res.is_err() {
         result.fully_functional = false;
      }
   } else {
      // If we dont have at least 10 pools just skip this check and assume that the RPC is ok
      tracing::info!("Not enough V3 pools for testing {}", rpc.url);
      return Ok(result);
   }

   Ok(result)
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::utils::RT;
   use zeus_eth::types::SUPPORTED_CHAINS;

   #[tokio::test]
   async fn test_ws_providers() {
      let rpc = RpcProviders::default();
      let ctx = ZeusCtx::new();

      let mut tasks = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let rpcs = rpc.get_all(chain);

         for rpc in rpcs {
            if !rpc.is_ws() {
               continue;
            }
            let rpc_clone = rpc.clone();
            let ctx_clone = ctx.clone();
            let task = RT.spawn(async move {
               match client_test(ctx_clone, rpc_clone.clone()).await {
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
      let ctx = ZeusCtx::new();

      let mut tasks = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let rpcs = rpc.get_all(chain);

         for rpc in rpcs {
            let rpc_clone = rpc.clone();
            let ctx_clone = ctx.clone();
            let task = RT.spawn(async move {
               match client_test(ctx_clone, rpc_clone.clone()).await {
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
