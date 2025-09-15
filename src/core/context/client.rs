use crate::core::{ZeusCtx, context::data_dir, utils::RT};
use zeus_eth::{
   abi::{weth9, zeus::ZeusStateView},
   alloy_primitives::Address,
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, BlockNumberOrTag},
   alloy_sol_types::SolEvent,
   amm::uniswap::UniswapPool,
   currency::ERC20Token,
   types::SUPPORTED_CHAINS,
   utils::{batch, client::*, get_logs_for},
};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::{
   collections::HashMap,
   sync::{Arc, Mutex, RwLock},
   time::Duration,
};

use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::{sync::Semaphore, time::sleep};

const PROVIDER_DATA_FILE: &str = "providers.json";

const CLIENT_TIMEOUT: u64 = 30;
const REQUEST_TIMEOUT: u64 = 60;

/// 3 Days in seconds
const THREE_DAYS: u64 = 259_200;

/// Request per second
pub const CLIENT_RPS: u32 = 10;
/// Max retries
pub const MAX_RETRIES: u32 = 10;
/// Initial backoff
pub const INITIAL_BACKOFF: u64 = 400;
/// Compute units per second
pub const COMPUTE_UNITS_PER_SECOND: u64 = 330;

/// An estimation of the gas needed to query the state of 20 V3 pools
///
/// This is depends on the specific pools and tokens
const _V3_POOL_STATE_GAS_FOR_20_POOLS: u64 = 812_000;

const _V4_POOL_STATE_GAS_FOR_20_POOLS: u64 = 692_000;

/// Batch size for fetching ETH balance
const ETH_BALANCE_BATCH: usize = 30;

/// Batch size for fetching ERC20 balance
const ERC20_BALANCE_BATCH: usize = 30;

/// Batch size for fetching ERC20 info
const ERC20_INFO_BATCH: usize = 30;

/// Batch size for fetching V3 pools
const VALIDATE_V4_POOLS_BATCH: usize = 60;

/// Batch size for fetching V2 pool reserves
const V2_POOL_RESERVES_BATCH: usize = 50;

/// Batch size for fetching V3 pool state
const V3_POOL_STATE_BATCH: usize = 20;

/// Batch size for fetching V4 pool state
const V4_POOL_STATE_BATCH: usize = 25;

/// A default value for the block range to query for logs
///
/// Should work for most endpoints
const DEFAULT_BLOCK_RANGE: u64 = 50_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A check for rpc functionality
pub struct RpcCheck {
   /// True if the rpc is archive
   pub archive: bool,

   /// True if the rpc is functional at all
   ///
   /// It should at least be able to return the latest block number but it could fail on more intensive requests like `eth_getLogs`
   pub working: bool,

   /// True if the rpc is fully functional
   ///
   /// All requests should work perfect
   pub fully_functional: bool,

   /// The block range to query for logs that the specific rpc can take it
   pub logs_block_range: u64,

   /// This is an estimation of the staticalll gas limit
   ///
   /// It means that we can make an `ethCall` that can at least use this much gas without getting an `evm timeout` error
   ///
   /// Each provider sets their own limits
   pub static_gas_limit: u64,

   /// Recommended batch size for fetching ETH balance
   pub eth_balance_batch: usize,

   /// Recommended batch size for fetching ERC20 balance
   pub erc20_balance_batch: usize,

   /// Recommended batch size for fetching ERC20 info
   pub erc20_info_batch: usize,

   /// Recommended batch size for fetching V3 pools
   pub validate_v4_pools_batch: usize,

   /// Recommended batch size for fetching V2 pool reserves
   pub v2_pool_reserves_batch: usize,

   /// Recommended batch size for fetching V3 pool state
   pub v3_pool_state_batch: usize,

   /// Recommended batch size for fetching V4 pool state
   pub v4_pool_state_batch: usize,

   /// Last time in UNIX timestamp we ran a check for this RPC
   pub last_check: Option<u64>,
}

impl RpcCheck {}

impl Default for RpcCheck {
   fn default() -> Self {
      Self {
         archive: true,
         working: true,
         fully_functional: true,
         logs_block_range: DEFAULT_BLOCK_RANGE,
         static_gas_limit: 0,
         eth_balance_batch: ETH_BALANCE_BATCH,
         erc20_balance_batch: ERC20_BALANCE_BATCH,
         erc20_info_batch: ERC20_INFO_BATCH,
         validate_v4_pools_batch: VALIDATE_V4_POOLS_BATCH,
         v2_pool_reserves_batch: V2_POOL_RESERVES_BATCH,
         v3_pool_state_batch: V3_POOL_STATE_BATCH,
         v4_pool_state_batch: V4_POOL_STATE_BATCH,
         last_check: None,
      }
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rpc {
   pub url: String,
   pub chain_id: u64,
   /// False if the rpc is added by the user
   pub default: bool,
   pub enabled: bool,
   pub check: RpcCheck,
   pub mev_protect: bool,
   #[serde(skip)]
   pub latency: Option<Duration>,
   /// Last time in UNIX timestamp we used this RPC
   pub last_used: u64,
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
         check: RpcCheck::default(),
         mev_protect,
         latency: None,
         last_used: 0,
      }
   }

   pub fn is_ws(&self) -> bool {
      self.url.starts_with("ws")
   }

   pub fn is_http(&self) -> bool {
      self.url.starts_with("http")
   }

   pub fn is_default(&self) -> bool {
      self.default
   }

   pub fn is_archive(&self) -> bool {
      self.check.archive
   }

   pub fn is_enabled(&self) -> bool {
      self.enabled
   }

   pub fn is_working(&self) -> bool {
      self.check.working
   }

   pub fn is_fully_functional(&self) -> bool {
      self.check.fully_functional
   }

   pub fn is_mev_protect(&self) -> bool {
      self.mev_protect
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

   pub fn last_check(&self) -> Option<u64> {
      self.check.last_check
   }

   pub fn should_run_check(&self) -> bool {
      let now = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap()
         .as_secs();
      if let Some(last_check) = self.check.last_check {
         let passed = now - last_check;
         passed > THREE_DAYS
      } else {
         true
      }
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZeusClient {
   pub rpcs: Arc<RwLock<HashMap<u64, Vec<Rpc>>>>,
}

impl Default for ZeusClient {
   fn default() -> Self {
      let mut rpcs = HashMap::new();

      // Chain ID 1: Ethereum

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

      Self {
         rpcs: Arc::new(RwLock::new(rpcs)),
      }
   }
}

impl ZeusClient {
   pub fn read<R>(&self, reader: impl FnOnce(&HashMap<u64, Vec<Rpc>>) -> R) -> R {
      reader(&self.rpcs.read().unwrap())
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut HashMap<u64, Vec<Rpc>>) -> R) -> R {
      writer(&mut self.rpcs.write().unwrap())
   }

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(PROVIDER_DATA_FILE);
      let data = std::fs::read(&dir)?;
      let rpcs: HashMap<u64, Vec<Rpc>> = serde_json::from_slice(&data)?;
      Ok(Self {
         rpcs: Arc::new(RwLock::new(rpcs)),
      })
   }

   pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
      let rpcs = self.read(|rpcs| rpcs.clone());
      let data = serde_json::to_vec(&rpcs)?;
      let dir = data_dir()?.join(PROVIDER_DATA_FILE);
      std::fs::write(&dir, data)?;
      Ok(())
   }

   pub fn get_rpcs(&self, chain: u64) -> Vec<Rpc> {
      self.read(|rpcs| rpcs.get(&chain).unwrap_or(&vec![]).to_vec())
   }

   pub fn add_rpc(&self, chain: u64, rpc: Rpc) {
      self.write(|rpcs| {
         rpcs.entry(chain).or_default().push(rpc);
      });
   }

   pub fn remove_rpc(&self, chain: u64, url: String) {
      self.write(|rpcs| {
         rpcs.entry(chain).or_default().retain(|rpc| rpc.url != url);
      });
   }

   pub async fn run_latency_check_for(&self, rpc: Rpc) {
      let retry_layer = retry_layer(
         MAX_RETRIES,
         INITIAL_BACKOFF,
         COMPUTE_UNITS_PER_SECOND,
      );
      let throttle_layer = throttle_layer(CLIENT_RPS);
      let client = get_client(&rpc.url, retry_layer, throttle_layer).await;

      let client = match client {
         Ok(client) => client,
         Err(e) => {
            tracing::error!(
               "Error connecting to client using {} {}",
               rpc.url,
               e
            );
            return;
         }
      };

      let time = Instant::now();
      match client.get_block_number().await {
         Ok(_) => {
            let latency = time.elapsed();
            self.write(|rpcs| {
               if let Some(rpcs) = rpcs.get_mut(&rpc.chain_id) {
                  for rpc_mut in rpcs.iter_mut() {
                     if rpc_mut.url == rpc.url {
                        rpc_mut.latency = Some(latency);
                     }
                  }
               }
            });
         }
         Err(e) => {
            tracing::error!("Error testing RPC: {} {}", rpc.url, e);
            return;
         }
      }
   }

   pub async fn run_latency_checks(&self) {
      let mut tasks = Vec::new();

      for chain in SUPPORTED_CHAINS {
         let rpcs = self.read(|rpcs| rpcs.get(&chain).unwrap_or(&vec![]).to_vec());
         let sempahore = Arc::new(Semaphore::new(5));

         for rpc in rpcs {
            let rpc = rpc.clone();
            let sempahore = sempahore.clone();
            let zeus_client = self.clone();

            let task = RT.spawn(async move {
               let _permit = sempahore.acquire().await.unwrap();
               zeus_client.run_latency_check_for(rpc).await;
            });
            tasks.push(task);
         }
      }

      for task in tasks {
         let _r = task.await;
      }

      self.sort_by_fastest();
   }

   pub async fn run_check_for(&self, ctx: ZeusCtx, rpc: Rpc) {
      match rpc_test(ctx, rpc.clone()).await {
         Ok((latency, result)) => {
            self.write(|rpcs| {
               if let Some(rpcs) = rpcs.get_mut(&rpc.chain_id) {
                  for rpc_mut in rpcs.iter_mut() {
                     if rpc_mut.url == rpc.url {
                        rpc_mut.check = result.clone();
                        rpc_mut.latency = Some(latency);
                     }
                  }
               }
            });
         }
         Err(e) => {
            tracing::error!("Error testing RPC {} {:?}", rpc.url, e);
            self.write(|rpcs| {
               if let Some(rpcs) = rpcs.get_mut(&rpc.chain_id) {
                  for rpc_mut in rpcs.iter_mut() {
                     if rpc_mut.url == rpc.url {
                        rpc_mut.check.working = false;
                     }
                  }
               }
            });
         }
      }
   }

   pub async fn run_rpc_checks(&self, ctx: ZeusCtx) {
      let mut tasks = Vec::new();

      for chain in SUPPORTED_CHAINS {
         let rpcs = self.read(|rpcs| rpcs.get(&chain).unwrap_or(&vec![]).to_vec());
         let semaphore = Arc::new(Semaphore::new(5));

         for rpc in rpcs {
            let rpc = rpc.clone();
            let ctx_clone = ctx.clone();
            let semaphore = semaphore.clone();
            let zeus_client = self.clone();

            let task = RT.spawn(async move {
               let _permit = semaphore.acquire().await.unwrap();
               zeus_client.run_check_for(ctx_clone, rpc).await;
            });
            tasks.push(task);
         }
      }

      for task in tasks {
         let _r = task.await;
      }

      self.sort_by_fastest();
   }

   /// Mark every RPC as working
   pub fn mark_all_as_working(&self) {
      self.write(|rpcs| {
         for (_, rpcs) in rpcs.iter_mut() {
            for rpc in rpcs {
               rpc.check.working = true;
            }
         }
      });
   }

   /// Mark every RPC as fully functional
   pub fn mark_all_as_fully_functional(&self) {
      self.write(|rpcs| {
         for (_, rpcs) in rpcs.iter_mut() {
            for rpc in rpcs {
               rpc.check.fully_functional = true;
            }
         }
      });
   }

   pub fn sort_by_fastest(&self) {
      self.write(|rpcs| {
         for (_, rpcs) in rpcs.iter_mut() {
            rpcs.sort_by(|a, b| {
               a.latency
                  .unwrap_or_default()
                  .partial_cmp(&b.latency.unwrap_or_default())
                  .unwrap_or(std::cmp::Ordering::Equal)
            });
         }
      });
   }

   /// Get the first available RPC for a chain
   pub fn get_first_available(&self, chain: u64) -> Option<Rpc> {
      self.read(|rpcs| rpcs.get(&chain)?.first().cloned())
   }

   /// Is there any available RPC for a chain
   pub fn rpc_available(&self, chain: u64) -> bool {
      self.get_first_available(chain).is_some()
   }

   pub fn rpc_archive_available(&self, chain: u64) -> bool {
      self.read(|rpcs| {
         let rpcs = rpcs.get(&chain);
         if rpcs.is_none() {
            return false;
         }
         let rpcs = rpcs.as_ref().unwrap();
         rpcs.iter().any(|rpc| rpc.is_working() && rpc.is_archive())
      })
   }

   pub fn mev_protect_available(&self, chain: u64) -> bool {
      self.read(|rpcs| {
         let rpcs = rpcs.get(&chain);
         if rpcs.is_none() {
            return false;
         }
         let rpcs = rpcs.as_ref().unwrap();
         rpcs.iter().any(|rpc| rpc.is_working() && rpc.is_mev_protect())
      })
   }

   pub async fn connect_to(&self, rpc: &Rpc) -> Result<RpcClient, anyhow::Error> {
      let (retry, throttle) = if rpc.is_default() {
         (
            retry_layer(
               MAX_RETRIES,
               INITIAL_BACKOFF,
               COMPUTE_UNITS_PER_SECOND,
            ),
            throttle_layer(CLIENT_RPS),
         )
      } else {
         (retry_layer(10, 300, 1000), throttle_layer(1000))
      };

      get_client(&rpc.url, retry, throttle).await
   }

   pub async fn get_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_TIMEOUT);
      let mut client = None;

      while !self.rpc_available(chain) {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get client for chain {} Timeout exceeded",
               chain
            ));
         }
         sleep(Duration::from_millis(100)).await;
      }

      let rpcs = self.read(|rpcs| rpcs.get(&chain).unwrap_or(&vec![]).to_vec());

      for rpc in &rpcs {
         // prioritize fully functional RPCs
         if !rpc.is_fully_functional() {
            continue;
         }

         let c = match self.connect_to(rpc).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  e
               );
               continue;
            }
         };

         client = Some(c);
         break;
      }

      if client.is_none() {
         for rpc in &rpcs {
            // skip them since we tried already
            if rpc.is_fully_functional() {
               continue;
            }

            let c = match self.connect_to(rpc).await {
               Ok(client) => client,
               Err(e) => {
                  tracing::error!(
                     "Error connecting to client using {} for chain {}: {:?}",
                     rpc.url,
                     chain,
                     e
                  );
                  continue;
               }
            };

            client = Some(c);
            break;
         }
      }

      match client {
         Some(client) => Ok(client),
         None => Err(anyhow!("No clients found for chain {}", chain)),
      }
   }

   pub async fn get_mev_protect_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_TIMEOUT);
      let mut client = None;

      while !self.mev_protect_available(chain) {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get MEV protect client for chain {} Timeout exceeded",
               chain
            ));
         }
         sleep(Duration::from_millis(100)).await;
      }

      let rpcs = self.read(|rpcs| rpcs.get(&chain).unwrap_or(&vec![]).to_vec());

      for rpc in &rpcs {
         if !rpc.mev_protect || !rpc.is_working() {
            continue;
         }

         let c = match self.connect_to(rpc).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  e
               );
               continue;
            }
         };
         client = Some(c);
         tracing::info!("Using MEV protect RPC: {}", rpc.url);
         break;
      }

      match client {
         Some(client) => Ok(client),
         None => Err(anyhow!(
            "No MEV protect clients found for chain {}",
            chain
         )),
      }
   }

   pub async fn get_archive_client(
      &self,
      chain: u64,
      http: bool,
   ) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_TIMEOUT);
      let mut client = None;

      while !self.rpc_archive_available(chain) {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get archive client for chain {} Timeout exceeded",
               chain
            ));
         }
         sleep(Duration::from_millis(100)).await;
      }

      let rpcs = self.read(|rpcs| rpcs.get(&chain).unwrap_or(&vec![]).to_vec());

      for rpc in &rpcs {
         if !rpc.is_working() || !rpc.is_enabled() || !rpc.is_archive() {
            continue;
         }

         if http && rpc.is_ws() {
            continue;
         }

         let c = match self.connect_to(rpc).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  e
               );
               continue;
            }
         };
         client = Some(c);
         break;
      }

      match client {
         Some(client) => Ok(client),
         None => Err(anyhow!(
            "No archive clients found for chain {}",
            chain
         )),
      }
   }

   /// Execute a request with automatic RPC selection, retries, and load balancing.
   ///
   /// The closure `f` receives a connected Provider (RpcClient) and returns a future with the result.
   /// Retries across RPCs on failure, up to MAX_RETRIES total attempts.
   /// Selects RPC based on latency + usage cooldown to spread concurrent load.
   pub async fn request<F, Fut, R>(&self, chain: u64, f: F) -> Result<R, anyhow::Error>
   where
      F: Fn(RpcClient) -> Fut,
      Fut: core::future::Future<Output = Result<R, anyhow::Error>>,
   {
      let cooldown_ms: u64 = 1000 / CLIENT_RPS as u64;
      let mut attempts = 0;
      let start = Instant::now();

      while attempts < MAX_RETRIES as usize {
         // Select best RPC: lowest effective latency (real + usage penalty)

         let rpc = self.write(|rpcs| {
            let mut empty = Vec::new();
            let rpcs = rpcs.get_mut(&chain).unwrap_or(&mut empty);

            let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
            let mut best_idx = None;
            let mut best_score = u128::MAX;

            for (idx, rpc) in rpcs.iter_mut().enumerate() {
               if !rpc.is_enabled() || !rpc.is_working() {
                  continue;
               }

               let time_since_used = now_ms.saturating_sub(rpc.last_used);
               let penalty = cooldown_ms.saturating_sub(time_since_used) as u128;
               let score = rpc.latency_ms() + penalty;
               if score < best_score {
                  best_score = score;
                  best_idx = Some(idx);
               }
            }

            let Some(idx) = best_idx else {
               return None;
            };

            rpcs[idx].last_used = now_ms;
            let rpc = rpcs[idx].clone();
            Some(rpc)
         });

         let rpc = match rpc {
            Some(rpc) => rpc,
            None => {
               return Err(anyhow!("No available RPCs for chain {}", chain));
            }
         };

         // eprintln!("Selected RPC {} for chain {}", rpc.url, chain);

         // Connect and execute
         let client = match self.connect_to(&rpc).await {
            Ok(client) => client,
            Err(e) => {
               tracing::warn!("Failed to connect to {}: {:?}", rpc.url, e);
               // Do not mark it as not working, could be a network issue
               attempts += 1;
               continue;
            }
         };

         match f(client).await {
            Ok(res) => return Ok(res),
            Err(e) => {
               tracing::warn!("Request failed on {}: {:?}", rpc.url, e);
               attempts += 1;
               // Exponential backoff (double each time, cap at 5s)
               let backoff =
                  Duration::from_millis(INITIAL_BACKOFF * 2u64.pow(attempts as u32 - 1).min(5000));
               sleep(backoff).await;
            }
         }

         if start.elapsed() > Duration::from_secs(REQUEST_TIMEOUT) {
            return Err(anyhow!("Request timed out for chain {}", chain));
         }
      }

      Err(anyhow!("Exhausted retries for chain {}", chain))
   }
}

/// Try to determine if the given RPC is working
///
/// Eg. Some free endpoints don't support `eth_getLogs` in the free tier
///
/// Others have a very low staticalll gas limit which cause the batch requests to fail
pub async fn rpc_test(ctx: ZeusCtx, rpc: Rpc) -> Result<(Duration, RpcCheck), anyhow::Error> {
   let retry = retry_layer(
      MAX_RETRIES,
      INITIAL_BACKOFF,
      COMPUTE_UNITS_PER_SECOND,
   );

   let throttle = throttle_layer(CLIENT_RPS);
   let client = get_client(&rpc.url, retry, throttle).await?;
   let chain = rpc.chain_id;

   let time = std::time::Instant::now();
   let latest_block = client.get_block_number().await?;
   let latency = time.elapsed();

   let block_to_query = if latest_block > 100_000 {
      latest_block - 100_000
   } else {
      return Err(anyhow!("Latest block is < 100_000"));
   };

   let weth = ERC20Token::wrapped_native_token(rpc.chain_id);

   let rpc_check = RpcCheck::default();
   let result = Arc::new(Mutex::new(rpc_check));

   let mut tasks = Vec::new();

   let client_clone = client.clone();
   let result_clone = result.clone();
   let task = RT.spawn(async move {
      archive_check(client_clone, block_to_query, result_clone).await;
   });

   tasks.push(task);

   let client_clone = client.clone();
   let result_clone = result.clone();
   let task = RT.spawn(async move {
      get_logs_check(
         client_clone,
         weth.address,
         latest_block,
         result_clone,
      )
      .await;
   });

   tasks.push(task);

   sleep(Duration::from_millis(100)).await;

   let client_clone = client.clone();
   let result_clone = result.clone();
   let ctx_clone = ctx.clone();
   let task = RT.spawn(async move {
      v2_pool_reserves_check(ctx_clone, client_clone, chain, result_clone).await;
   });

   tasks.push(task);

   let client_clone = client.clone();
   let result_clone = result.clone();
   let ctx_clone = ctx.clone();
   let task = RT.spawn(async move {
      v3_pool_state_check(ctx_clone, client_clone, chain, result_clone).await;
   });

   tasks.push(task);

   sleep(Duration::from_millis(100)).await;

   let client_clone = client.clone();
   let result_clone = result.clone();
   let ctx_clone = ctx.clone();
   let task = RT.spawn(async move {
      v4_pool_state_check(ctx_clone, client_clone, chain, result_clone).await;
   });

   tasks.push(task);

   let client_clone = client.clone();
   let result_clone = result.clone();
   let ctx_clone = ctx.clone();
   let task = RT.spawn(async move {
      validate_v4_pools_check(ctx_clone, client_clone, chain, result_clone).await;
   });

   tasks.push(task);

   for task in tasks {
      let _task = task.await;
   }

   {
      let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
      let mut guard = result.lock().unwrap();
      guard.last_check = Some(now);
   }

   let guard = result.lock().unwrap();
   let result = guard.clone();

   Ok((latency, result))
}

async fn archive_check(client: RpcClient, block_to_query: u64, result: Arc<Mutex<RpcCheck>>) {
   let old_block = client
      .get_block(BlockId::Number(BlockNumberOrTag::Number(
         block_to_query,
      )))
      .await;

   let is_archive = match old_block {
      Ok(old_block) => {
         if old_block.is_some() {
            true
         } else {
            false
         }
      }
      Err(_e) => false,
   };

   let mut guard = result.lock().unwrap();
   guard.archive = is_archive;
}

async fn get_logs_check(
   client: RpcClient,
   weth_address: Address,
   latest_block: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let mut block_range = DEFAULT_BLOCK_RANGE;
   let mut success = false;

   while !success {
      if block_range == 0 {
         break;
      }

      let client = client.clone();

      let res = get_logs_for(
         client,
         vec![weth_address],
         vec![weth9::Deposit::SIGNATURE],
         latest_block,
         1,
         block_range,
      )
      .await;

      match res {
         Ok(_) => {
            success = true;
         }
         Err(e) => {
            block_range -= 5_000;
            tracing::trace!("eth_getLogs Check Error: {:?}", e);
         }
      }
   }

   match success {
      true => {
         let mut guard = result.lock().unwrap();
         guard.logs_block_range = block_range;
      }
      false => {
         let mut guard = result.lock().unwrap();
         guard.fully_functional = false;
         guard.logs_block_range = 0;
      }
   }
}

async fn v2_pool_reserves_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let pool_manager = ctx.pool_manager();
   let all_v2_pools = pool_manager.get_v2_pools_for_chain(chain);
   let mut v2_pools = Vec::with_capacity(V2_POOL_RESERVES_BATCH);

   for pool in all_v2_pools {
      if v2_pools.len() == V2_POOL_RESERVES_BATCH {
         break;
      }
      v2_pools.push(pool);
   }

   let mut batch_size = V2_POOL_RESERVES_BATCH;
   let mut success = false;

   while !success {
      if batch_size == 0 {
         break;
      }

      let client = client.clone();

      let mut pools = Vec::new();
      for pool in &v2_pools {
         if pools.len() == batch_size {
            break;
         }
         pools.push(pool.address());
      }

      let res = batch::get_v2_reserves(client, chain, pools).await;
      match res {
         Ok(_) => {
            success = true;
         }
         Err(e) => {
            batch_size -= 5;
            tracing::trace!("V2 Reserves Check Error: {:?}", e);
         }
      }
   }

   match success {
      true => {
         let mut guard = result.lock().unwrap();
         guard.v2_pool_reserves_batch = batch_size;
      }
      false => {
         let mut guard = result.lock().unwrap();
         guard.fully_functional = false;
         guard.v2_pool_reserves_batch = 0;
      }
   }
}

async fn v3_pool_state_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let pool_manager = ctx.pool_manager();

   let all_v3_pools = pool_manager.get_v3_pools_for_chain(chain);
   let mut v3_pools = Vec::with_capacity(V3_POOL_STATE_BATCH);

   for pool in all_v3_pools {
      if v3_pools.len() == V3_POOL_STATE_BATCH {
         break;
      }
      v3_pools.push(pool);
   }

   let mut batch_size = V3_POOL_STATE_BATCH;
   let mut success = false;

   while !success {
      if batch_size == 0 {
         break;
      }

      let client = client.clone();

      let mut pools = Vec::new();
      for pool in &v3_pools {
         if pools.len() == batch_size {
            break;
         }
         pools.push(ZeusStateView::V3Pool {
            addr: pool.address(),
            tokenA: pool.currency0().address(),
            tokenB: pool.currency1().address(),
            fee: pool.fee().fee_u24(),
         });
      }

      let res = batch::get_v3_state(client, chain, pools).await;
      match res {
         Ok(_) => {
            success = true;
         }
         Err(e) => {
            batch_size -= 5;
            tracing::trace!("V3 State Check Error: {:?}", e);
         }
      }
   }

   match success {
      true => {
         let mut guard = result.lock().unwrap();
         guard.v3_pool_state_batch = batch_size;
      }
      false => {
         let mut guard = result.lock().unwrap();
         guard.fully_functional = false;
         guard.v3_pool_state_batch = 0;
      }
   }
}

async fn v4_pool_state_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let pool_manager = ctx.pool_manager();
   let all_v4_pools = pool_manager.get_v4_pools_for_chain(chain);
   let mut v4_pools = Vec::with_capacity(V4_POOL_STATE_BATCH);

   for pool in all_v4_pools {
      if v4_pools.len() == V4_POOL_STATE_BATCH {
         break;
      }
      v4_pools.push(pool);
   }

   let mut batch_size = V4_POOL_STATE_BATCH;
   let mut success = false;

   while !success {
      if batch_size == 0 {
         break;
      }

      let client = client.clone();

      let mut pools = Vec::new();
      for pool in &v4_pools {
         if pools.len() == batch_size {
            break;
         }
         let p = ZeusStateView::V4Pool {
            pool: pool.id(),
            tickSpacing: pool.fee().tick_spacing(),
         };
         pools.push(p);
      }

      let res = batch::get_v4_pool_state(client, chain, pools).await;
      match res {
         Ok(_) => {
            success = true;
         }
         Err(e) => {
            batch_size -= 5;
            tracing::trace!("V4 State Check Error: {:?}", e);
         }
      }
   }

   match success {
      true => {
         let mut guard = result.lock().unwrap();
         guard.v4_pool_state_batch = batch_size;
      }
      false => {
         let mut guard = result.lock().unwrap();
         guard.fully_functional = false;
         guard.v4_pool_state_batch = 0;
      }
   }
}

async fn validate_v4_pools_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let pool_manager = ctx.pool_manager();
   let all_v4_pools = pool_manager.get_v4_pools_for_chain(chain);
   let mut v4_pools = Vec::with_capacity(VALIDATE_V4_POOLS_BATCH);

   for pool in all_v4_pools {
      if v4_pools.len() == VALIDATE_V4_POOLS_BATCH {
         break;
      }
      v4_pools.push(pool);
   }

   let mut batch_size = VALIDATE_V4_POOLS_BATCH;
   let mut success = false;

   while !success {
      if batch_size == 0 {
         break;
      }

      let client = client.clone();

      let mut pools = Vec::new();
      for pool in &v4_pools {
         if pools.len() == batch_size {
            break;
         }
         pools.push(pool.id());
      }

      let res = batch::validate_v4_pools(client, chain, pools).await;
      match res {
         Ok(_) => {
            success = true;
         }
         Err(e) => {
            batch_size -= 5;
            tracing::trace!("V4 Validate Pools Check Error: {:?}", e);
         }
      }
   }

   match success {
      true => {
         let mut guard = result.lock().unwrap();
         guard.validate_v4_pools_batch = batch_size;
      }
      false => {
         let mut guard = result.lock().unwrap();
         guard.fully_functional = false;
         guard.validate_v4_pools_batch = 0;
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::utils::RT;
   use zeus_eth::alloy_provider::Provider;

   #[tokio::test]
   async fn test_rpcs() {
      let zeus_client = ZeusClient::default();
      zeus_client.mark_all_as_working();

      let chain = 1;
      let time = std::time::Instant::now();
      let mut tasks = Vec::new();

      for _ in 0..30 {
         let zeus_client = zeus_client.clone();

         tasks.push(RT.spawn(async move {
            let res = zeus_client
               .request(chain, |client| async move {
                  let block = client.get_block_number().await?;
                  Ok(block)
               })
               .await;
            match res {
               Ok(_block) => {}
               Err(e) => {
                  eprintln!("error: {:?}", e);
               }
            }
         }));
      }

      for task in tasks {
         task.await.unwrap();
      }

      let elapsed = time.elapsed();
      println!("Time: {}secs", elapsed.as_secs_f32());
   }
}
