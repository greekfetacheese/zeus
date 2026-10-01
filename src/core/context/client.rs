use crate::core::persisted::{PersistedFile, file_path};
use crate::core::{WalletStateKey, ZeusCtx};
use crate::utils::{RT, TimeStamp, simulate::STORAGE_FETCH_CHUNK_SIZE, write_private_atomic};
use zeus_eth::{
   abi::{
      weth9,
      zeus::ZeusStateViewV3::{V3Pool, V4Pool},
   },
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, BlockNumberOrTag},
   alloy_signer_local::PrivateKeySigner,
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
   time::{Duration, Instant},
};

use tokio::{sync::Semaphore, time::sleep};

/// Bound ciphertext to this logical slot (AAD).
const PROVIDER_AAD: &[u8] = b"zeus-providers-v1";

const CLIENT_SELECTION_TIMEOUT: u64 = 3;

/// Default timeout for sending a transaction or using an MEV protect rpc
pub const CLIENT_TIMEOUT_FOR_SENDING_TX: u64 = 60;

/// Default request timeout
const REQUEST_TIMEOUT: u64 = 15;

/// Default client timeout
const CLIENT_TIMEOUT: u64 = 5;

/// 8 hours in seconds
const EIGHT_HOURS: u64 = 28_800;

/// Request per second
const CLIENT_RPS: u32 = 10;

/// Max retries
const MAX_RETRIES: u32 = 10;

/// Initial backoff
const INITIAL_BACKOFF: u64 = 400;

/// Compute units per second
const COMPUTE_UNITS_PER_SECOND: u64 = 330;

/// Batch size for fetching ETH balance
const ETH_BALANCE_BATCH: usize = 20;

/// Batch size for fetching ERC20 balance
const ERC20_BALANCE_BATCH: usize = 20;

/// Batch size for fetching ERC20 info
const ERC20_INFO_BATCH: usize = 20;

/// Batch size for fetching V3 pools
const VALIDATE_V4_POOLS_BATCH: usize = 20;

/// Batch size for fetching the state of V2/V3/V4 pools
const POOL_STATE_UPDATE_BATCH: usize = 20;

/// Batch size for fetching V2 pool reserves
const V2_POOL_RESERVES_BATCH: usize = 20;

/// Batch size for fetching V3 pool state
const V3_POOL_STATE_BATCH: usize = 20;

/// Batch size for fetching V4 pool state
const V4_POOL_STATE_BATCH: usize = 20;

/// Batch size for probing Multicall3 via ERC-20 allowances
const MULTICALL_BATCH: usize = 20;

/// Account count for probing JSON-RPC batch `eth_getTransactionCount`
const JSON_RPC_BATCH: usize = 20;

/// For testing only
const DEFAULT_BLOCK_RANGE: u64 = 5_000;

async fn connect_rpc(url: &str, timeout: u64) -> Result<RpcClient, anyhow::Error> {
   get_client(
      url,
      retry_layer(
         MAX_RETRIES,
         INITIAL_BACKOFF,
         COMPUTE_UNITS_PER_SECOND,
      ),
      throttle_layer(CLIENT_RPS),
      timeout,
   )
   .await
}

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

   /// True if the rpc can call Multicall3
   #[serde(default)]
   pub multicall: bool,

   /// True if the rpc supports `eth_call` state overrides (StorageReader)
   #[serde(default)]
   pub state_override: bool,

   /// True if the rpc supports JSON-RPC batch requests
   #[serde(default)]
   pub json_rpc_batch: bool,

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

   /// Recommended batch size for fetching the state for V2/V3/V4 pools
   pub pool_state_update_batch: usize,

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

impl Default for RpcCheck {
   fn default() -> Self {
      Self {
         archive: false,
         working: false,
         fully_functional: false,
         multicall: false,
         state_override: false,
         json_rpc_batch: false,
         logs_block_range: DEFAULT_BLOCK_RANGE,
         static_gas_limit: 0,
         eth_balance_batch: ETH_BALANCE_BATCH,
         erc20_balance_batch: ERC20_BALANCE_BATCH,
         erc20_info_batch: ERC20_INFO_BATCH,
         validate_v4_pools_batch: VALIDATE_V4_POOLS_BATCH,
         pool_state_update_batch: POOL_STATE_UPDATE_BATCH,
         v2_pool_reserves_batch: V2_POOL_RESERVES_BATCH,
         v3_pool_state_batch: V3_POOL_STATE_BATCH,
         v4_pool_state_batch: V4_POOL_STATE_BATCH,
         last_check: None,
      }
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rpc {
   pub url: Arc<str>,
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
   /// Last time in UNIX timestamp this RPC failed to do a request
   #[serde(default)]
   pub last_failure: Option<u64>,

   #[serde(skip)]
   pub test_in_progress: bool,
}

impl Rpc {
   #[must_use]
   pub fn builder(url: impl Into<Arc<str>>, chain_id: u64) -> RpcBuilder {
      RpcBuilder {
         url: url.into(),
         chain_id,
         default: false,
         enabled: false,
         mev_protect: false,
      }
   }

   pub fn is_ws(&self) -> bool {
      self.url.starts_with("ws")
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

   pub fn is_multicall(&self) -> bool {
      self.check.multicall
   }

   pub fn is_state_override(&self) -> bool {
      self.check.state_override
   }

   pub fn is_json_rpc_batch(&self) -> bool {
      self.check.json_rpc_batch
   }

   pub fn is_mev_protect(&self) -> bool {
      self.mev_protect
   }

   pub fn latency_ms(&self) -> u128 {
      self.latency.map(|latency| latency.as_millis()).unwrap_or(0)
   }

   pub fn latency_str(&self) -> String {
      if let Some(latency) = self.latency {
         format!("{}ms", latency.as_millis())
      } else {
         "N/A".to_string()
      }
   }

   pub fn should_run_check(&self) -> bool {
      let now = TimeStamp::now_as_secs().unwrap_or_default();
      if let Some(last_check) = self.check.last_check {
         let passed = now.timestamp().saturating_sub(last_check);
         passed > EIGHT_HOURS
      } else {
         true
      }
   }
}

/// Builder for [Rpc]. `url` and `chain_id` are required; flags default to false.
#[must_use = "builders do nothing unless you call build()"]
pub struct RpcBuilder {
   url: Arc<str>,
   chain_id: u64,
   default: bool,
   enabled: bool,
   mev_protect: bool,
}

impl RpcBuilder {
   /// Bundled endpoint shipped with Zeus (not added by the user).
   #[must_use]
   pub fn builtin(mut self) -> Self {
      self.default = true;
      self
   }

   #[must_use]
   pub fn enabled(mut self) -> Self {
      self.enabled = true;
      self
   }

   #[must_use]
   pub fn mev_protect(mut self) -> Self {
      self.mev_protect = true;
      self
   }

   pub fn build(self) -> Rpc {
      Rpc {
         url: self.url,
         chain_id: self.chain_id,
         default: self.default,
         enabled: self.enabled,
         check: RpcCheck::default(),
         mev_protect: self.mev_protect,
         latency: None,
         last_used: 0,
         last_failure: None,
         test_in_progress: false,
      }
   }
}

/// Map from RPC URL to RPC, keyed by `Arc<str>`.
type RpcMapByUrl = HashMap<Arc<str>, Rpc>;

fn insert_chain_rpcs(
   map: &mut HashMap<u64, RpcMapByUrl>,
   chain_id: u64,
   urls: &[&str],
   mev_urls: &[&str],
) {
   let mut rpcs = RpcMapByUrl::new();
   for url in urls {
      let url: Arc<str> = Arc::from(*url);
      rpcs.insert(
         Arc::clone(&url),
         Rpc::builder(url, chain_id).builtin().build(),
      );
   }
   for url in mev_urls {
      let url: Arc<str> = Arc::from(*url);
      rpcs.insert(
         Arc::clone(&url),
         Rpc::builder(url, chain_id).builtin().mev_protect().build(),
      );
   }
   map.insert(chain_id, rpcs);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZeusClient {
   pub rpcs: Arc<RwLock<HashMap<u64, RpcMapByUrl>>>,
}

impl Default for ZeusClient {
   fn default() -> Self {
      let mut rpc_map_by_chain = HashMap::new();

      insert_chain_rpcs(
         &mut rpc_map_by_chain,
         1,
         &[
            "wss://ethereum-rpc.publicnode.com",
            "wss://mainnet.gateway.tenderly.co",
            "https://ethereum-rpc.publicnode.com",
            "https://eth.blockrazor.xyz",
         ],
         &[
            "https://rpc.mevblocker.io",
            "https://rpc.flashbots.net/fast",
         ],
      );

      insert_chain_rpcs(
         &mut rpc_map_by_chain,
         10,
         &[
            "wss://optimism.gateway.tenderly.co",
            "wss://optimism.drpc.org",
            "wss://optimism-rpc.publicnode.com",
            "https://mainnet.optimism.io",
            "https://optimism-rpc.publicnode.com",
            "https://optimism.drpc.org",
         ],
         &[],
      );

      insert_chain_rpcs(
         &mut rpc_map_by_chain,
         56,
         &[
            "wss://bsc-rpc.publicnode.com",
            "https://binance.llamarpc.com",
            "https://bsc-pokt.nodies.app",
            "https://api.zan.top/bsc-mainnet",
         ],
         &[],
      );

      insert_chain_rpcs(
         &mut rpc_map_by_chain,
         8453,
         &[
            "wss://base-rpc.publicnode.com",
            "wss://base.gateway.tenderly.co",
            "https://mainnet.base.org",
            "https://1rpc.io/base",
            "https://base-rpc.publicnode.com",
         ],
         &[],
      );

      insert_chain_rpcs(
         &mut rpc_map_by_chain,
         42161,
         &[
            "wss://arbitrum-one-rpc.publicnode.com",
            "https://arbitrum.meowrpc.com",
            "https://arb1.arbitrum.io/rpc",
            "https://1rpc.io/arb",
         ],
         &[],
      );

      insert_chain_rpcs(
         &mut rpc_map_by_chain,
         4663,
         &[
            "https://rpc.mainnet.chain.robinhood.com/",
            "https://robinhood-rpc.publicnode.com",
            "https://robinhood.drpc.org",
         ],
         &[],
      );

      Self {
         rpcs: Arc::new(RwLock::new(rpc_map_by_chain)),
      }
   }
}

impl ZeusClient {
   pub fn read<R>(&self, reader: impl FnOnce(&HashMap<u64, RpcMapByUrl>) -> R) -> R {
      reader(&self.rpcs.read().unwrap())
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut HashMap<u64, RpcMapByUrl>) -> R) -> R {
      writer(&mut self.rpcs.write().unwrap())
   }

   pub fn load_from_file(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let dir = Self::dir()?;
      let sealed = std::fs::read(&dir)?;
      let rpcs: HashMap<u64, RpcMapByUrl> = key.open_json(&sealed, PROVIDER_AAD)?;
      self.write(|map| *map = rpcs);
      Ok(())
   }

   pub fn save_to_file(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let sealed = self.read(|rpcs| key.seal_json(rpcs, PROVIDER_AAD))?;
      write_private_atomic(&Self::dir()?, &sealed)?;
      Ok(())
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      file_path(PersistedFile::Providers)
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }

   pub fn get_rpcs(&self, chain: u64) -> RpcMapByUrl {
      self.read(|rpcs| rpcs.get(&chain).cloned().unwrap_or_default())
   }

   pub fn add_rpc(&self, chain: u64, rpc: Rpc) {
      self.write(|rpcs| {
         rpcs.entry(chain).or_default().insert(rpc.url.clone(), rpc);
      });
   }

   pub fn set_test_in_progress(&self, chain: u64, rpc: &Rpc, in_progress: bool) {
      self.write(|rpcs_map| {
         if let Some(rpc) = rpcs_map.get_mut(&chain).and_then(|rpcs| rpcs.get_mut(&rpc.url)) {
            rpc.test_in_progress = in_progress;
         }
      });
   }

   pub fn remove_rpc(&self, chain: u64, url: Arc<str>) {
      self.write(|rpcs| {
         rpcs.entry(chain).or_default().remove(&*url);
      });
   }

   fn update_rpc(&self, chain: u64, url: &str, f: impl FnOnce(&mut Rpc)) {
      self.write(|rpcs_map| {
         if let Some(rpc) = rpcs_map.get_mut(&chain).and_then(|rpcs| rpcs.get_mut(url)) {
            f(rpc);
         }
      });
   }

   pub async fn run_latency_check_for(&self, rpc: Rpc) {
      let client = connect_rpc(&rpc.url, REQUEST_TIMEOUT).await;

      let client = match client {
         Ok(client) => client,
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::error!(
               "Error connecting to client using {} {}",
               rpc.url,
               _e
            );
            return;
         }
      };

      let time = Instant::now();
      match client.get_block_number().await {
         Ok(_) => {
            let latency = time.elapsed();
            self.update_rpc(rpc.chain_id, &rpc.url, |rpc| {
               rpc.check.working = true;
               rpc.latency = Some(latency);
            });
         }
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::error!(
               "Error latency checking for RPC: {} {}",
               rpc.url,
               _e
            );
            self.update_rpc(rpc.chain_id, &rpc.url, |rpc| {
               rpc.check.working = false;
            });
         }
      }
   }

   pub async fn run_latency_checks(&self, ctx: ZeusCtx) {
      let mut tasks = Vec::new();

      for chain in SUPPORTED_CHAINS {
         if ctx.is_chain_disabled(chain) {
            continue;
         }

         let rpcs = self.get_rpcs(chain);
         let semaphore = Arc::new(Semaphore::new(2));

         for (_url, rpc) in rpcs {
            if !rpc.is_enabled() {
               continue;
            }

            let semaphore = semaphore.clone();
            let zeus_client = self.clone();

            let task = RT.spawn(async move {
               let _permit = semaphore.acquire().await.unwrap();
               zeus_client.run_latency_check_for(rpc).await;
            });
            tasks.push(task);
         }
      }

      for task in tasks {
         let _r = task.await;
      }
   }

   pub async fn run_check_for(&self, ctx: ZeusCtx, rpc: Rpc) {
      self.set_test_in_progress(rpc.chain_id, &rpc, true);

      match rpc_test(ctx, rpc.clone()).await {
         Ok((latency, result)) => {
            self.update_rpc(rpc.chain_id, &rpc.url, |rpc| {
               rpc.check = result;
               rpc.latency = Some(latency);
            });
         }
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::error!("Error testing RPC {} {:?}", rpc.url, _e);
            self.update_rpc(rpc.chain_id, &rpc.url, |rpc| {
               rpc.check.working = false;
            });
         }
      }

      self.set_test_in_progress(rpc.chain_id, &rpc, false);
   }

   pub async fn run_rpc_checks(&self, ctx: ZeusCtx) {
      let mut tasks = Vec::new();

      for chain in SUPPORTED_CHAINS {
         if ctx.is_chain_disabled(chain) {
            continue;
         }

         let rpcs = self.get_rpcs(chain);
         let semaphore = Arc::new(Semaphore::new(2));

         for (_url, rpc) in rpcs {
            if !rpc.is_enabled() {
               continue;
            }

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
   }

   /// Mark every RPC as working
   pub fn mark_all_as_working(&self) {
      self.write(|rpcs_map| {
         for rpcs_by_url in rpcs_map.values_mut() {
            for rpc in rpcs_by_url.values_mut() {
               rpc.check.working = true;
            }
         }
      });
   }

   /// Is there any available RPC for a chain
   pub fn rpc_available(&self, chain: u64) -> bool {
      self.get_rpcs(chain).values().any(|rpc| rpc.is_enabled() && rpc.is_working())
   }

   /// Returns true if every enabled RPC that has been checked is fully functional.
   ///
   /// Unchecked endpoints are ignored so a first-run probe does not look like a malfunction.
   pub fn rpcs_fully_functional(&self, chain: u64) -> bool {
      self
         .get_rpcs(chain)
         .values()
         .filter(|rpc| rpc.is_enabled() && rpc.check.last_check.is_some())
         .all(|rpc| rpc.is_fully_functional())
   }

   pub fn rpc_archive_available(&self, chain: u64) -> bool {
      self.get_rpcs(chain).values().any(|rpc| rpc.is_working() && rpc.is_archive())
   }

   pub fn mev_protect_available(&self, chain: u64) -> bool {
      self
         .get_rpcs(chain)
         .values()
         .any(|rpc| rpc.is_working() && rpc.is_enabled() && rpc.is_mev_protect())
   }

   pub async fn connect_to(&self, rpc: &Rpc) -> Result<RpcClient, anyhow::Error> {
      self.connect_with_timeout(rpc, CLIENT_TIMEOUT).await
   }

   pub async fn connect_with_timeout(
      &self,
      rpc: &Rpc,
      timeout: u64,
   ) -> Result<RpcClient, anyhow::Error> {
      connect_rpc(&rpc.url, timeout).await
   }

   pub async fn get_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_SELECTION_TIMEOUT);

      loop {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get client for chain {} Timeout exceeded",
               chain
            ));
         }

         if let Some(rpc) = self.get_best_rpc(chain) {
            let c = match self.connect_to(&rpc).await {
               Ok(client) => client,
               Err(_e) => {
                  #[cfg(feature = "dev")]
                  tracing::error!(
                     "Error connecting to client using {} for chain {}: {:?}",
                     rpc.url,
                     chain,
                     _e
                  );
                  self.penalize(chain, &rpc);
                  sleep(Duration::from_millis(100)).await;
                  continue;
               }
            };
            return Ok(c);
         } else {
            sleep(Duration::from_millis(100)).await;
         }
      }
   }

   pub async fn get_mev_protect_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_SELECTION_TIMEOUT);

      while !self.mev_protect_available(chain) {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get MEV protect client for chain {} Timeout exceeded",
               chain
            ));
         }
         sleep(Duration::from_millis(100)).await;
      }

      let rpcs = self.get_rpcs(chain);

      for rpc in rpcs.values() {
         if !rpc.mev_protect || !rpc.is_working() || !rpc.is_enabled() {
            continue;
         }

         match self.connect_with_timeout(rpc, CLIENT_TIMEOUT_FOR_SENDING_TX).await {
            Ok(client) => return Ok(client),
            Err(_e) => {
               #[cfg(feature = "dev")]
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  _e
               );
            }
         }
      }

      Err(anyhow!(
         "No MEV protect clients found for chain {}",
         chain
      ))
   }

   pub async fn get_archive_client(
      &self,
      chain: u64,
      http: bool,
   ) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_SELECTION_TIMEOUT);

      while !self.rpc_archive_available(chain) {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get archive client for chain {} Timeout exceeded",
               chain
            ));
         }
         sleep(Duration::from_millis(100)).await;
      }

      let rpcs = self.get_rpcs(chain);

      for rpc in rpcs.values() {
         if !rpc.is_working() || !rpc.is_enabled() || !rpc.is_archive() {
            continue;
         }

         if http && rpc.is_ws() {
            continue;
         }

         match self.connect_to(rpc).await {
            Ok(client) => return Ok(client),
            Err(_e) => {
               #[cfg(feature = "dev")]
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  _e
               );
            }
         }
      }

      Err(anyhow!(
         "No archive clients found for chain {}",
         chain
      ))
   }

   fn penalize(&self, chain: u64, rpc: &Rpc) {
      let now = TimeStamp::now_as_millis().unwrap_or_default();
      self.update_rpc(chain, &rpc.url, |rpc| {
         rpc.last_failure = Some(now.timestamp());
      });
   }

   /// Select the best RPC for the given chain
   pub fn get_best_rpc(&self, chain: u64) -> Option<Rpc> {
      let cooldown_ms: u64 = 1000 / CLIENT_RPS as u64;
      let failure_penalty_max: u128 = 10_000;
      let failure_decay_secs: u64 = 60;

      self.write(|rpcs_map| {
         let Some(rpcs) = rpcs_map.get_mut(&chain) else {
            return None;
         };
         let now_ms = TimeStamp::now_as_millis().unwrap_or_default().timestamp();
         let mut best_key = None;
         let mut best_fully = false;
         let mut best_score = u128::MAX;

         for (url, rpc) in rpcs.iter_mut() {
            if !rpc.is_enabled() || !rpc.is_working() {
               continue;
            }

            let time_since_used = now_ms.saturating_sub(rpc.last_used);
            let usage_penalty = cooldown_ms.saturating_sub(time_since_used) as u128;

            let mut score = rpc.latency_ms() + usage_penalty;

            if let Some(lf) = rpc.last_failure {
               let time_since_fail = now_ms.saturating_sub(lf);
               if time_since_fail < failure_decay_secs * 1000 {
                  let remaining = (failure_decay_secs * 1000 - time_since_fail) as u128;
                  let fail_penalty =
                     failure_penalty_max * remaining / (failure_decay_secs as u128 * 1000);
                  score += fail_penalty;
               } else {
                  rpc.last_failure = None;
               }
            }

            let fully = rpc.is_fully_functional();
            let better = match best_key {
               None => true,
               Some(_) => (fully && !best_fully) || (fully == best_fully && score < best_score),
            };

            if better {
               best_score = score;
               best_fully = fully;
               best_key = Some(url.clone());
            }
         }

         let Some(key) = best_key else {
            return None;
         };

         rpcs.get_mut(&key).map(|rpc| {
            rpc.last_used = now_ms;
            rpc.clone()
         })
      })
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
      let mut attempts = 0;
      let start = Instant::now();

      while attempts < MAX_RETRIES as usize {
         let rpc = self.get_best_rpc(chain);

         let rpc = match rpc {
            Some(rpc) => rpc,
            None => {
               attempts += 1;
               sleep(Duration::from_millis(INITIAL_BACKOFF)).await;
               continue;
            }
         };

         let client = match self.connect_to(&rpc).await {
            Ok(client) => client,
            Err(_e) => {
               #[cfg(feature = "dev")]
               tracing::warn!("Failed to connect to {}: {:?}", rpc.url, _e);
               // Do not mark it as not working, could be a network issue
               attempts += 1;
               self.penalize(chain, &rpc);
               continue;
            }
         };

         match f(client).await {
            Ok(res) => return Ok(res),
            Err(_e) => {
               self.penalize(chain, &rpc);
               #[cfg(feature = "dev")]
               tracing::warn!("Request failed on {}: {:?}", rpc.url, _e);
               attempts += 1;
               sleep(Duration::from_millis(INITIAL_BACKOFF)).await;
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
async fn rpc_test(ctx: ZeusCtx, rpc: Rpc) -> Result<(Duration, RpcCheck), anyhow::Error> {
   #[cfg(feature = "dev")]
   tracing::debug!("Testing {}", rpc.url);

   let client = connect_rpc(&rpc.url, CLIENT_TIMEOUT_FOR_SENDING_TX).await?;
   let chain = rpc.chain_id;

   let time = Instant::now();
   let latest_block = client.get_block_number().await?;
   let latency = time.elapsed();

   let result = Arc::new(Mutex::new(RpcCheck::default()));

   // If it can return at least the latest block is considered functional
   {
      let mut guard = result.lock().unwrap();
      guard.working = true;
   }

   let block_to_query = if latest_block > 100_000 {
      latest_block - 100_000
   } else {
      return Err(anyhow!("Latest block is < 100_000"));
   };

   let weth = ERC20Token::wrapped_native_token(rpc.chain_id);

   archive_check(client.clone(), block_to_query, result.clone()).await;

   get_logs_check(
      client.clone(),
      weth.address,
      latest_block,
      result.clone(),
   )
   .await;

   multicall_check(client.clone(), weth.address, result.clone()).await;
   state_override_check(client.clone(), weth.address, result.clone()).await;
   json_rpc_batch_check(client.clone(), result.clone()).await;
   v2_pool_reserves_check(ctx.clone(), client.clone(), chain, result.clone()).await;
   v3_pool_state_check(ctx.clone(), client.clone(), chain, result.clone()).await;
   v4_pool_state_check(ctx.clone(), client.clone(), chain, result.clone()).await;
   validate_v4_pools_check(ctx.clone(), client.clone(), chain, result.clone()).await;

   {
      let now = TimeStamp::now_as_secs()?;
      let mut guard = result.lock().unwrap();
      guard.last_check = Some(now.timestamp());
      // V3 state batch calls are the most expensive in terms of gas
      // So will use this as a reference for the pool state update batch
      guard.pool_state_update_batch = guard.v3_pool_state_batch;
      guard.fully_functional = guard.logs_block_range > 0
         && guard.v2_pool_reserves_batch > 0
         && guard.v3_pool_state_batch > 0
         && guard.v4_pool_state_batch > 0
         && guard.validate_v4_pools_batch > 0
         && guard.multicall
         && guard.state_override
         && guard.json_rpc_batch;
   }

   let result = result.lock().unwrap().clone();

   #[cfg(feature = "dev")]
   tracing::debug!(
      "Tested {} in {}secs",
      rpc.url,
      time.elapsed().as_secs_f32()
   );

   Ok((latency, result))
}

async fn multicall_check(client: RpcClient, weth: Address, result: Arc<Mutex<RpcCheck>>) {
   let owner = Address::ZERO;
   let pairs: Vec<(Address, Address)> = (1..=MULTICALL_BATCH)
      .map(|_i| {
         let signer = PrivateKeySigner::random();
         (weth, signer.address())
      })
      .collect();

   let ok = match batch::get_erc20_allowances(client, owner, pairs, None).await {
      Ok(rows) => rows.len() == MULTICALL_BATCH,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("Multicall Check Error: {:?}", _e);
         false
      }
   };

   let mut guard = result.lock().unwrap();
   guard.multicall = ok;
}

async fn state_override_check(client: RpcClient, account: Address, result: Arc<Mutex<RpcCheck>>) {
   let slots: Vec<U256> = (0..STORAGE_FETCH_CHUNK_SIZE).map(U256::from).collect();

   let ok = match batch::get_account_storage(client, account, slots, None).await {
      Ok(read) => read.values.len() == STORAGE_FETCH_CHUNK_SIZE,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("State Override Check Error: {:?}", _e);
         false
      }
   };

   let mut guard = result.lock().unwrap();
   guard.state_override = ok;
}

async fn json_rpc_batch_check(client: RpcClient, result: Arc<Mutex<RpcCheck>>) {
   let accounts: Vec<Address> =
      (0..JSON_RPC_BATCH).map(|_| PrivateKeySigner::random().address()).collect();

   let ok = match batch::get_account_nonces(client, accounts, None).await {
      Ok(nonces) => nonces.len() == JSON_RPC_BATCH,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("JSON-RPC Batch Check Error: {:?}", _e);
         false
      }
   };

   let mut guard = result.lock().unwrap();
   guard.json_rpc_batch = ok;
}

async fn archive_check(client: RpcClient, block_to_query: u64, result: Arc<Mutex<RpcCheck>>) {
   let old_block = client
      .get_block(BlockId::Number(BlockNumberOrTag::Number(
         block_to_query,
      )))
      .await;

   let is_archive = matches!(old_block, Ok(Some(_)));

   let mut guard = result.lock().unwrap();
   guard.archive = is_archive;
}

async fn get_logs_check(
   client: RpcClient,
   weth_address: Address,
   latest_block: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   // Weth deposit is a very frequent event, we only query 10 blocks back
   let block_range = 10;
   let from_block = latest_block.saturating_sub(block_range);
   let mut success = false;

   let res = get_logs_for(
      client,
      vec![weth_address],
      vec![weth9::Deposit::SIGNATURE],
      from_block,
      1,
      block_range,
   )
   .await;

   match res {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("eth_getLogs Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   if success {
      guard.logs_block_range = DEFAULT_BLOCK_RANGE;
   } else {
      guard.logs_block_range = 0;
   }
}

async fn v2_pool_reserves_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let sample: Vec<_> = ctx
      .pool_manager()
      .get_v2_pools_for_chain(chain)
      .into_iter()
      .take(V2_POOL_RESERVES_BATCH)
      .collect();

   let mut success = false;

   let client = client.clone();
   let pools: Vec<_> =
      sample.iter().take(V2_POOL_RESERVES_BATCH).map(|pool| pool.address()).collect();

   match batch::get_v2_reserves(client, chain, pools).await {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("V2 Reserves Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   guard.v2_pool_reserves_batch = if success { V2_POOL_RESERVES_BATCH } else { 0 };
}

async fn v3_pool_state_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let sample: Vec<_> = ctx
      .pool_manager()
      .get_v3_pools_for_chain(chain)
      .into_iter()
      .take(V3_POOL_STATE_BATCH)
      .collect();

   let mut success = false;

   let client = client.clone();
   let pools: Vec<_> = sample
      .iter()
      .take(V3_POOL_STATE_BATCH)
      .map(|pool| V3Pool {
         addr: pool.address(),
         tokenA: pool.currency0().address(),
         tokenB: pool.currency1().address(),
         fee: pool.fee().fee_u24(),
      })
      .collect();

   match batch::get_v3_state(client, chain, pools).await {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("V3 State Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   guard.v3_pool_state_batch = if success { V3_POOL_STATE_BATCH } else { 0 };
}

async fn v4_pool_state_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let sample: Vec<_> = ctx
      .pool_manager()
      .get_v4_pools_for_chain(chain)
      .into_iter()
      .take(V4_POOL_STATE_BATCH)
      .collect();

   let mut success = false;

   let client = client.clone();
   let pools: Vec<_> = sample
      .iter()
      .take(V4_POOL_STATE_BATCH)
      .map(|pool| V4Pool {
         pool: pool.id(),
         tickSpacing: pool.tick_spacing(),
      })
      .collect();

   match batch::get_v4_pool_state(client, chain, pools).await {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("V4 State Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   guard.v4_pool_state_batch = if success { V4_POOL_STATE_BATCH } else { 0 };
}

async fn validate_v4_pools_check(
   ctx: ZeusCtx,
   client: RpcClient,
   chain: u64,
   result: Arc<Mutex<RpcCheck>>,
) {
   let sample: Vec<_> = ctx
      .pool_manager()
      .get_v4_pools_for_chain(chain)
      .into_iter()
      .take(VALIDATE_V4_POOLS_BATCH)
      .collect();

   let mut success = false;

   let client = client.clone();
   let pools: Vec<_> = sample.iter().take(VALIDATE_V4_POOLS_BATCH).map(|pool| pool.id()).collect();

   match batch::validate_v4_pools(client, chain, pools).await {
      Ok(_) => success = true,
      Err(_e) => {
         #[cfg(feature = "dev")]
         tracing::debug!("V4 Validate Pools Check Error: {:?}", _e);
      }
   }

   let mut guard = result.lock().unwrap();
   guard.validate_v4_pools_batch = if success { VALIDATE_V4_POOLS_BATCH } else { 0 };
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::alloy_provider::Provider;

   #[tokio::test]
   async fn test_rpcs() {
      let zeus_client = ZeusClient::default();
      zeus_client.mark_all_as_working();

      let chain = 1;
      let time = Instant::now();
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

   #[test]
   fn test_seal_open_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let client = ZeusClient::default();
      let sealed = client.read(|rpcs| key.seal_json(rpcs, PROVIDER_AAD)).unwrap();
      let loaded: HashMap<u64, RpcMapByUrl> = key.open_json(&sealed, PROVIDER_AAD).unwrap();
      assert!(!loaded.is_empty());
      assert!(key.open_json::<HashMap<u64, RpcMapByUrl>>(&sealed, b"wrong-aad").is_err());
   }

   fn rpc_for_select(url: &str, fully: bool, latency_ms: u64) -> Rpc {
      let mut rpc = Rpc::builder(url, 1).enabled().build();
      rpc.check.working = true;
      rpc.check.fully_functional = fully;
      rpc.latency = Some(Duration::from_millis(latency_ms));
      rpc
   }

   fn client_with(rpcs: impl IntoIterator<Item = Rpc>) -> ZeusClient {
      let client = ZeusClient {
         rpcs: Arc::new(RwLock::new(HashMap::new())),
      };
      for rpc in rpcs {
         client.add_rpc(rpc.chain_id, rpc);
      }
      client
   }

   #[test]
   fn get_best_rpc_prefers_fully_functional_over_faster_partial() {
      let client = client_with([
         rpc_for_select("http://partial", false, 10),
         rpc_for_select("http://full", true, 100),
      ]);
      let best = client.get_best_rpc(1).unwrap();
      assert_eq!(&*best.url, "http://full");
   }

   #[test]
   fn get_best_rpc_falls_back_to_partial_when_none_are_fully_functional() {
      let client = client_with([
         rpc_for_select("http://slow", false, 80),
         rpc_for_select("http://fast", false, 10),
      ]);
      let best = client.get_best_rpc(1).unwrap();
      assert_eq!(&*best.url, "http://fast");
   }

   fn rpc_checked(url: &str, fully: bool) -> Rpc {
      let mut rpc = rpc_for_select(url, fully, 10);
      rpc.check.last_check = Some(1);
      rpc
   }

   #[test]
   fn rpcs_fully_functional_ignores_unchecked_and_disabled() {
      let mut unchecked = rpc_for_select("http://unchecked", false, 10);
      unchecked.check.last_check = None;
      let mut disabled = rpc_checked("http://disabled-partial", false);
      disabled.enabled = false;
      let client = client_with([unchecked, disabled, rpc_checked("http://ok", true)]);
      assert!(client.rpcs_fully_functional(1));
   }

   #[test]
   fn rpcs_fully_functional_false_when_a_checked_enabled_rpc_is_partial() {
      let client = client_with([
         rpc_checked("http://ok", true),
         rpc_checked("http://partial", false),
      ]);
      assert!(!client.rpcs_fully_functional(1));
   }
}
