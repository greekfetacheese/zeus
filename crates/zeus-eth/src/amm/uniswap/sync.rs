use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, B256};
use alloy_rpc_types::Log;
use alloy_sol_types::SolEvent;

use std::{collections::HashMap, sync::Arc};
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};

use super::{
   AnyUniswapPool, DexKind, FeeAmount, State, UniswapPool, UniswapV2Pool, UniswapV3Pool,
   UniswapV4Pool, v4_pool_id,
};

use crate::abi::uniswap::{
   v2::factory::IUniswapV2Factory, v3::factory::IUniswapV3Factory, v4::IPoolManager,
};

use crate::currency::{Currency, NativeCurrency, erc20::ERC20Token};

use serde::{Deserialize, Serialize};

use crate::utils::{GET_LOGS_MAX_RETRIES, address_book, get_logs_for};
use anyhow::anyhow;
use tracing::{error, trace};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
   pub chain_id: u64,
   pub block: u64,
   pub dex: DexKind,
}

impl Default for Checkpoint {
   fn default() -> Self {
      Self {
         chain_id: 0,
         block: 0,
         dex: DexKind::UniswapV2,
      }
   }
}

impl Checkpoint {
   pub fn new(chain_id: u64, block: u64, dex: DexKind) -> Self {
      Self {
         chain_id,
         block,
         dex,
      }
   }
}

/// Synced Pools along with the checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
   pub checkpoint: Checkpoint,
   pub pools: Vec<AnyUniswapPool>,
}

impl Default for SyncResult {
   fn default() -> Self {
      Self::new(Checkpoint::default(), Vec::new())
   }
}

impl SyncResult {
   pub fn new(checkpoint: Checkpoint, pools: Vec<AnyUniswapPool>) -> Self {
      Self { checkpoint, pools }
   }
}

/// A decoded `IPoolManager::Initialize` — the key parameters of a V4 pool, before any token data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V4PoolInit {
   pub currency0: Address,
   pub currency1: Address,
   pub fee: u32,
   pub tick_spacing: i32,
   pub hooks: Address,
}

impl V4PoolInit {
   /// Pool id, computed from the raw key (no token metadata needed).
   pub fn id(&self) -> B256 {
      v4_pool_id(
         self.currency0,
         self.currency1,
         FeeAmount::new(self.fee),
         self.tick_spacing,
         self.hooks,
      )
   }

   /// Materialise the pool once both currencies are resolved. Currencies are ordered by address
   /// inside `new_with_spacing`.
   pub fn into_pool(
      self,
      chain_id: u64,
      currency0: Currency,
      currency1: Currency,
   ) -> UniswapV4Pool {
      UniswapV4Pool::new_with_spacing(
         chain_id,
         FeeAmount::new(self.fee),
         self.tick_spacing,
         DexKind::UniswapV4,
         currency0,
         currency1,
         State::none(),
         self.hooks,
      )
   }
}

/// Fetch and decode `IPoolManager::Initialize` logs for one inclusive block range.
///
/// `block_range` is the largest span sent to a single `eth_getLogs`; the range is split into
/// `block_range` chunks, `concurrency` in flight. Unlike [`sync_pools`] this never reads the chain
/// tip — the caller owns the range and the checkpoint.
pub async fn v4_pools_from_logs<P, N>(
   client: P,
   chain_id: u64,
   from_block: u64,
   to_block: u64,
   concurrency: usize,
   block_range: u64,
   max_retries: usize,
) -> Result<Vec<V4PoolInit>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let target = address_book::uniswap_v4_pool_manager(chain_id)?;
   let logs = get_logs_for(
      client,
      vec![target],
      vec![IPoolManager::Initialize::SIGNATURE],
      from_block,
      to_block,
      concurrency,
      block_range,
      max_retries,
   )
   .await?;

   let mut out = Vec::with_capacity(logs.len());
   for log in logs {
      let IPoolManager::Initialize {
         currency0,
         currency1,
         fee,
         tickSpacing,
         hooks,
         ..
      } = log.log_decode()?.inner.data;
      out.push(V4PoolInit {
         currency0,
         currency1,
         fee: fee.to_string().parse()?,
         tick_spacing: tickSpacing.to_string().parse()?,
         hooks,
      });
   }
   Ok(out)
}

/// Configuration for syncing pools
#[derive(Debug, Clone)]
pub struct SyncConfig {
   /// The chain id to sync from
   pub chain_id: u64,

   /// Which DEX to sync pools from, use [DexKind::all()] to sync all dexes
   pub dex: Vec<DexKind>,

   /// Do concurrent requests, set 1 for no concurrency
   pub concurrency: usize,

   /// The number of pools to costruct in a single request
   pub batch_size: usize,

   /// From which block to start syncing, If None it will start from the dex creation block
   pub from_block: Option<u64>,

   /// Up to which block to sync, If None it will sync to the latest block
   pub to_block: Option<u64>,
}

impl SyncConfig {
   pub fn new(
      chain_id: u64,
      dex: Vec<DexKind>,
      concurrency: usize,
      batch_size: usize,
      from_block: Option<u64>,
      to_block: Option<u64>,
   ) -> Self {
      Self {
         chain_id,
         dex,
         concurrency,
         batch_size,
         from_block,
         to_block,
      }
   }
}

/// Sync pools with the given configuration
///
/// See [SyncConfig]
pub async fn sync_pools<P, N>(
   client: P,
   config: SyncConfig,
   block_range: u64,
) -> Result<Vec<SyncResult>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let chain = client.get_chain_id().await?;

   if chain != config.chain_id {
      anyhow::bail!(
         "Chain ID mismatch, your Chain Id is {}, but client returned: {}",
         config.chain_id,
         chain
      );
   }

   let semaphore = Arc::new(Semaphore::new(config.concurrency));
   let results = Arc::new(Mutex::new(Vec::new()));

   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let dexes = config.dex.clone();

   for dex in dexes {
      let client = client.clone();
      let semaphore = semaphore.clone();
      let results = results.clone();
      let config = config.clone();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire().await?;
         let synced = do_sync_pools(
            client.clone(),
            config.chain_id,
            dex,
            config.concurrency,
            config.batch_size,
            config.from_block,
            block_range,
         )
         .await?;

         results.lock().await.push(synced);
         Ok(())
      });

      tasks.push(task);
   }

   for task in tasks {
      if let Err(e) = task.await {
         error!("sync task failed: {}", e);
      }
   }

   Ok(Arc::try_unwrap(results).unwrap().into_inner())
}

async fn do_sync_pools<P, N>(
   client: P,
   chain_id: u64,
   dex: DexKind,
   concurrency: usize,
   batch_size: usize,
   from_block: Option<u64>,
   block_range: u64,
) -> Result<SyncResult, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let target_addr = if dex.is_v4() {
      address_book::uniswap_v4_pool_manager(chain_id)?
   } else {
      dex.factory(chain_id)?
   };

   let events = match dex {
      DexKind::UniswapV2 => vec![IUniswapV2Factory::PairCreated::SIGNATURE],
      DexKind::PancakeSwapV2 => vec![IUniswapV2Factory::PairCreated::SIGNATURE],
      DexKind::UniswapV3 => vec![IUniswapV3Factory::PoolCreated::SIGNATURE],
      DexKind::PancakeSwapV3 => vec![IUniswapV3Factory::PoolCreated::SIGNATURE],
      DexKind::UniswapV4 => vec![IPoolManager::Initialize::SIGNATURE],
   };

   let synced_block = client.get_block_number().await?;

   let from_block = if let Some(block) = from_block {
      block
   } else {
      dex.creation_block(chain_id)?
   };

   trace!(
      target: "zeus_eth::amm::sync",
      "Syncing pools for chain {} DEX: {} from block {}",
      chain_id,
      dex.as_str(),
      from_block
   );

   let logs = get_logs_for(
      client.clone(),
      vec![target_addr],
      events,
      from_block,
      synced_block,
      concurrency,
      block_range,
      GET_LOGS_MAX_RETRIES,
   )
   .await?;

   trace!(target: "zeus_eth::amm::sync", "Found {} logs for chain {} DEX: {}", logs.len(), chain_id, dex.as_str());
   let semaphore = Arc::new(Semaphore::new(concurrency));
   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let pools = Arc::new(Mutex::new(Vec::new()));

   for log_batch in logs.chunks(batch_size) {
      let client = client.clone();
      let pools = pools.clone();
      let semaphore = semaphore.clone();
      let logs = log_batch.to_vec();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire().await?;
         let fetched_pools = sync_pools_from_log_batch(client, chain_id, dex, logs).await?;
         for pool in &fetched_pools {
            trace!(target: "zeus_eth::amm::sync", "Synced pool for {}-{} ChainId: {}", pool.currency0().symbol(), pool.currency1().symbol(), chain_id);
         }
         pools.lock().await.extend(fetched_pools);
         Ok(())
      });

      tasks.push(task);
   }

   for task in tasks {
      if let Err(e) = task.await {
         error!(target: "zeus_eth::amm::sync", "Error syncing pool: {:?}", e);
      }
   }

   let pools = Arc::try_unwrap(pools).unwrap().into_inner();
   let checkpoint = Checkpoint::new(chain_id, synced_block, dex);
   trace!(target: "zeus_eth::amm::sync", "Synced {} pools for {} - on ChainId {}", pools.len(), dex.as_str(), chain_id);

   let synced = SyncResult::new(checkpoint, pools);
   Ok(synced)
}

async fn sync_pools_from_log_batch<P, N>(
   client: P,
   chain_id: u64,
   dex: DexKind,
   logs: Vec<Log>,
) -> Result<Vec<AnyUniswapPool>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if dex.is_v2() {
      v2_pools_from_log_batch(client, chain_id, dex, logs).await
   } else if dex.is_v3() {
      v3_pools_from_log_batch(client, chain_id, dex, logs).await
   } else if dex.is_v4() {
      v4_pools_from_log_batch(client, chain_id, logs).await
   } else {
      Err(anyhow::anyhow!("Unknown dex: {:?}", dex))
   }
}

#[derive(Clone)]
struct V2PoolInfo {
   address: Address,
   token0: Address,
   token1: Address,
}

#[derive(Clone)]
struct V3PoolInfo {
   address: Address,
   token0: Address,
   token1: Address,
   fee: u32,
}

async fn v2_pools_from_log_batch<P, N>(
   client: P,
   chain: u64,
   dex: DexKind,
   logs: Vec<Log>,
) -> Result<Vec<AnyUniswapPool>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let mut pools_info = Vec::new();
   let mut token_addr = Vec::new();

   // Parse logs and identify tokens to fetch
   for log in logs {
      let IUniswapV2Factory::PairCreated {
         token0,
         token1,
         pair,
         ..
      } = log.log_decode()?.inner.data;

      let pool = V2PoolInfo {
         address: pair,
         token0,
         token1,
      };

      pools_info.push(pool.clone());

      let token0_is_base = ERC20Token::base_token(chain, token0).is_some();
      let token1_is_base = ERC20Token::base_token(chain, token1).is_some();

      if !token0_is_base {
         token_addr.push(token0);
      }

      if !token1_is_base {
         token_addr.push(token1);
      }
   }

   // Fetch ERC20 data and map tokens by address
   let tokens_erc20 = ERC20Token::from_batch(client.clone(), chain, token_addr).await?;
   let mut token_map = HashMap::new();
   for token in tokens_erc20 {
      token_map.insert(token.address, token);
   }

   // Reconstruct pools
   let mut v2_pools = Vec::new();
   for pool in &pools_info {
      let token0 = if let Some(token) = ERC20Token::base_token(chain, pool.token0) {
         token
      } else if let Some(token) = token_map.get(&pool.token0) {
         token.clone()
      } else {
         return Err(anyhow!(
            "Missing ERC-20 data for token0: {}",
            pool.token0
         ));
      };

      let token1 = if let Some(token) = ERC20Token::base_token(chain, pool.token1) {
         token
      } else if let Some(token) = token_map.get(&pool.token1) {
         token.clone()
      } else {
         return Err(anyhow!(
            "Missing ERC-20 data for token1: {}",
            pool.token1
         ));
      };

      let p = UniswapV2Pool::new(chain, pool.address, token0, token1, dex);
      v2_pools.push(p);
   }

   let any_pools = v2_pools.into_iter().map(AnyUniswapPool::V2).collect();
   Ok(any_pools)
}

async fn v3_pools_from_log_batch<P, N>(
   client: P,
   chain: u64,
   dex: DexKind,
   logs: Vec<Log>,
) -> Result<Vec<AnyUniswapPool>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let mut pools_info = Vec::new();
   let mut token_addr = Vec::new();

   // Parse logs and identify tokens to fetch
   for log in logs {
      let IUniswapV3Factory::PoolCreated {
         token0,
         token1,
         pool,
         fee,
         ..
      } = log.log_decode()?.inner.data;

      let fee: u32 = fee.to_string().parse()?;
      let pool = V3PoolInfo {
         address: pool,
         token0,
         token1,
         fee,
      };

      pools_info.push(pool.clone());

      let token0_is_base = ERC20Token::base_token(chain, token0).is_some();
      let token1_is_base = ERC20Token::base_token(chain, token1).is_some();

      if !token0_is_base {
         token_addr.push(token0);
      }

      if !token1_is_base {
         token_addr.push(token1);
      }
   }

   // Fetch ERC20 data and map tokens by address
   let tokens_erc20 = ERC20Token::from_batch(client.clone(), chain, token_addr).await?;
   let mut token_map = HashMap::new();
   for token in tokens_erc20 {
      token_map.insert(token.address, token);
   }

   // Reconstruct pools
   let mut v3_pools = Vec::new();
   for pool in &pools_info {
      let token0 = if let Some(token) = ERC20Token::base_token(chain, pool.token0) {
         token
      } else if let Some(token) = token_map.get(&pool.token0) {
         token.clone()
      } else {
         return Err(anyhow!(
            "Missing ERC-20 data for token0: {}",
            pool.token0
         ));
      };

      let token1 = if let Some(token) = ERC20Token::base_token(chain, pool.token1) {
         token
      } else if let Some(token) = token_map.get(&pool.token1) {
         token.clone()
      } else {
         return Err(anyhow!(
            "Missing ERC-20 data for token1: {}",
            pool.token1
         ));
      };

      let p = UniswapV3Pool::new(chain, pool.address, pool.fee, token0, token1, dex);
      v3_pools.push(p);
   }

   let any_pools = v3_pools.into_iter().map(AnyUniswapPool::V3).collect();
   Ok(any_pools)
}

async fn v4_pools_from_log_batch<P, N>(
   client: P,
   chain: u64,
   logs: Vec<Log>,
) -> Result<Vec<AnyUniswapPool>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let mut pools_info = Vec::new();
   let mut token_addr = Vec::new();

   // Parse logs and identify tokens to fetch
   for log in logs {
      let IPoolManager::Initialize {
         currency0,
         currency1,
         fee,
         tickSpacing,
         hooks,
         ..
      } = log.log_decode()?.inner.data;

      let init = V4PoolInit {
         currency0,
         currency1,
         fee: fee.to_string().parse()?,
         tick_spacing: tickSpacing.to_string().parse()?,
         hooks,
      };

      let currency0_is_native = init.currency0.is_zero();
      let currency0_is_base = ERC20Token::base_token(chain, init.currency0).is_some();
      let currency1_is_native = init.currency1.is_zero();
      let currency1_is_base = ERC20Token::base_token(chain, init.currency1).is_some();

      if !currency0_is_native && !currency0_is_base {
         token_addr.push(init.currency0);
      }
      if !currency1_is_native && !currency1_is_base {
         token_addr.push(init.currency1);
      }

      pools_info.push(init);
   }

   // Fetch ERC20 data and map tokens by address
   let tokens_erc20 = ERC20Token::from_batch(client.clone(), chain, token_addr).await?;
   let mut token_map = HashMap::new();
   for token in tokens_erc20 {
      token_map.insert(token.address, token);
   }

   // Reconstruct pools
   let mut v4_pools = Vec::new();
   for pool in &pools_info {
      let currency0 = if pool.currency0.is_zero() {
         Currency::from(NativeCurrency::from(chain))
      } else if let Some(base_token) = ERC20Token::base_token(chain, pool.currency0) {
         Currency::from(base_token)
      } else if let Some(token) = token_map.get(&pool.currency0) {
         Currency::from(token.clone())
      } else {
         return Err(anyhow!(
            "Missing ERC-20 data for currency0: {}",
            pool.currency0
         ));
      };

      let currency1 = if pool.currency1.is_zero() {
         Currency::from(NativeCurrency::from(chain))
      } else if let Some(base_token) = ERC20Token::base_token(chain, pool.currency1) {
         Currency::from(base_token)
      } else if let Some(token) = token_map.get(&pool.currency1) {
         Currency::from(token.clone())
      } else {
         return Err(anyhow!(
            "Missing ERC-20 data for currency1: {}",
            pool.currency1
         ));
      };

      v4_pools.push(pool.into_pool(chain, currency0, currency1));
   }

   let any_pools = v4_pools.into_iter().map(AnyUniswapPool::V4).collect();
   Ok(any_pools)
}
