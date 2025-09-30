use alloy_contract::private::{Network, Provider};
use alloy_primitives::Address;
use alloy_rpc_types::Log;
use alloy_sol_types::SolEvent;

use std::{collections::HashMap, sync::Arc};
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};

use super::{AnyUniswapPool, DexKind, FeeAmount, State, UniswapPool, UniswapV2Pool, UniswapV3Pool, UniswapV4Pool};

use crate::abi::uniswap::{v2::factory::IUniswapV2Factory, v3::factory::IUniswapV3Factory, v4::IPoolManager};

use crate::currency::{Currency, NativeCurrency, erc20::ERC20Token};

use serde::{Deserialize, Serialize};

use crate::utils::{address_book, get_logs_for};
use anyhow::anyhow;
use tracing::error;

/// Sync pools from the last checkpoint
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

/// Configuration for syncing pools
///
/// - `chain_id` The chain id to sync from
///
/// - `dex` Which dexes to sync pools from, use [DexKind::all()] to sync all dexes
///
/// - `concurrency` Do concurrent requests, set 1 for no concurrency
///
/// - `batch_size` The number of pools to sync in a single request
///
/// - `from_block` From which block to start syncing, If None it will start from the dex creation block
///
/// - `to_block` To which block to sync, If None it will sync to the latest block
#[derive(Debug, Clone)]
pub struct SyncConfig {
   pub chain_id: u64,
   pub dex: Vec<DexKind>,
   pub concurrency: usize,
   pub batch_size: usize,
   pub from_block: Option<u64>,
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
pub async fn sync_pools<P, N>(client: P, config: SyncConfig, block_range: u64) -> Result<Vec<SyncResult>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let chain = client.get_chain_id().await?;

   if chain != config.chain_id {
      anyhow::bail!(
         "Chain ID mismatch, your Chain Id {}, but client returned: {}",
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

   tracing::trace!(
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
      concurrency,
      block_range,
   )
   .await?;

   tracing::trace!(target: "zeus_eth::amm::sync", "Found {} logs for chain {} DEX: {}", logs.len(), chain_id, dex.as_str());
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
            tracing::trace!(target: "zeus_eth::amm::sync", "Synced pool for {}-{} ChainId: {}", pool.currency0().symbol(), pool.currency1().symbol(), chain_id);
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
   tracing::trace!(target: "zeus_eth::amm::sync", "Synced {} pools for {} - on ChainId {}", pools.len(), dex.as_str(), chain_id);
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
      v4_pools_from_log_batch(client, chain_id, dex, logs).await
   } else {
      return Err(anyhow::anyhow!("Unknown dex: {:?}", dex));
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

#[derive(Clone)]
struct V4PoolInfo {
   currency0: Address,
   currency1: Address,
   fee: u32,
   hooks: Address,
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
         return Err(anyhow!("Missing ERC-20 data for token0: {}", pool.token0));
      };

      let token1 = if let Some(token) = ERC20Token::base_token(chain, pool.token1) {
         token
      } else if let Some(token) = token_map.get(&pool.token1) {
         token.clone()
      } else {
         return Err(anyhow!("Missing ERC-20 data for token1: {}", pool.token1));
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
         return Err(anyhow!("Missing ERC-20 data for token0: {}", pool.token0));
      };

      let token1 = if let Some(token) = ERC20Token::base_token(chain, pool.token1) {
         token
      } else if let Some(token) = token_map.get(&pool.token1) {
         token.clone()
      } else {
         return Err(anyhow!("Missing ERC-20 data for token1: {}", pool.token1));
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
      let IPoolManager::Initialize {
         currency0,
         currency1,
         fee,
         hooks,
         ..
      } = log.log_decode()?.inner.data;

      let fee: u32 = fee.to_string().parse()?;
      let pool = V4PoolInfo {
         currency0,
         currency1,
         fee,
         hooks,
      };
      pools_info.push(pool.clone());

      let currency0_is_native = currency0.is_zero();
      let currency0_is_base = ERC20Token::base_token(chain, currency0).is_some();
      let currency1_is_native = currency1.is_zero();
      let currency1_is_base = ERC20Token::base_token(chain, currency1).is_some();

      if !currency0_is_native && !currency0_is_base {
         token_addr.push(currency0);
      }
      if !currency1_is_native && !currency1_is_base {
         token_addr.push(currency1);
      }
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

      let fee = FeeAmount::CUSTOM(pool.fee);
      let p = UniswapV4Pool::new(
         chain,
         fee,
         dex,
         currency0,
         currency1,
         State::none(),
         pool.hooks,
      );
      v4_pools.push(p);
   }

   let any_pools = v4_pools.into_iter().map(AnyUniswapPool::V4).collect();
   Ok(any_pools)
}
