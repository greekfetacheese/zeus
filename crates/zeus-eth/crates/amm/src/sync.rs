use alloy_contract::private::{Network, Provider};
use alloy_primitives::Address;
use alloy_rpc_types::Log;

use std::{collections::HashMap, sync::Arc};
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};

use crate::{
   AnyUniswapPool, DexKind, UniswapPool,
   uniswap::{
      state::State,
      v2::pool::UniswapV2Pool,
      v3::pool::UniswapV3Pool,
      v4::{FeeAmount, pool::UniswapV4Pool},
   },
};
use abi::{
   alloy_sol_types::SolEvent,
   uniswap::{v2::factory::IUniswapV2Factory, v3::factory::IUniswapV3Factory, v4::IPoolManager},
};
use currency::{Currency, NativeCurrency, erc20::ERC20Token};

use serde::{Deserialize, Serialize};

use anyhow::anyhow;
use tracing::error;
use types::ChainId;
use utils::{address, get_logs_for};

pub const RECOMMENDED_BATCH_SIZE: usize = 30;

/// Sync pools from the last checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
   pub chain_id: u64,
   pub block: u64,
   pub dex: DexKind,
   pub pool_len: usize,
}

impl Default for Checkpoint {
   fn default() -> Self {
      Self {
         chain_id: 0,
         block: 0,
         dex: DexKind::UniswapV2,
         pool_len: 0,
      }
   }
}

impl Checkpoint {
   pub fn new(chain_id: u64, block: u64, dex: DexKind, pool_len: usize) -> Self {
      Self {
         chain_id,
         block,
         dex,
         pool_len,
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

   pub fn file_name(chain: ChainId, dex: DexKind) -> String {
      format!("{}-{}", dex.to_str(), chain.name())
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
/// - `from_block` From which block to start syncing, if None,
///  it will start from the dex creation block
#[derive(Debug, Clone)]
pub struct SyncConfig {
   pub chain_id: u64,
   pub dex: Vec<DexKind>,
   pub concurrency: u8,
   pub batch_size: usize,
   pub from_block: Option<u64>,
}

impl SyncConfig {
   pub fn new(chain_id: u64, dex: Vec<DexKind>, concurrency: u8, batch_size: usize, from_block: Option<u64>) -> Self {
      Self {
         chain_id,
         dex,
         concurrency,
         batch_size,
         from_block,
      }
   }
}

/// Sync pools from the given checkpoints
pub async fn sync_from_checkpoints<P, N>(
   client: P,
   concurrency: u8,
   batch_size: usize,
   checkpoints: Vec<Checkpoint>,
) -> Result<Vec<SyncResult>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let chain = client.get_chain_id().await?;

   for check in &checkpoints {
      if check.chain_id != chain {
         return Err(anyhow::anyhow!(
            "Chain mismatch, At least one of the checkpoints is not for the given chain {}",
            chain
         ));
      }
   }

   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let results = Arc::new(Mutex::new(Vec::new()));
   let semaphore = Arc::new(Semaphore::new(concurrency.into()));

   for checkpoint in checkpoints {
      let client = client.clone();
      let semaphore = semaphore.clone();
      let results = results.clone();

      let config = SyncConfig::new(
         checkpoint.chain_id,
         vec![checkpoint.dex],
         concurrency,
         batch_size,
         Some(checkpoint.block),
      );

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire().await?;
         let synced = sync_pools(client, config).await?;
         results.lock().await.push(synced);
         Ok(())
      });

      tasks.push(task);
   }

   for task in tasks {
      if let Err(e) = task.await? {
         error!("sync task failed: {}", e);
      }
   }

   let synced = Arc::try_unwrap(results).unwrap().into_inner();
   let synced = synced.get(0).unwrap().clone();

   Ok(synced)
}

/// Sync pools with the given configuration
///
/// See [SyncConfig]
pub async fn sync_pools<P, N>(client: P, config: SyncConfig) -> Result<Vec<SyncResult>, anyhow::Error>
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

   let semaphore = Arc::new(Semaphore::new(config.concurrency.into()));
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
            config.concurrency.into(),
            config.batch_size,
            config.from_block,
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
) -> Result<SyncResult, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let target_addr = if dex.is_v4() {
      address::uniswap_v4_pool_manager(chain_id)?
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

   let from_block = if from_block.is_none() {
      dex.creation_block(chain_id)?
   } else {
      from_block.unwrap()
   };

   tracing::info!(
      target: "zeus_eth::amm::sync",
      "Syncing pools for chain {} DEX: {} from block {}",
      chain_id,
      dex.to_str(),
      from_block
   );

   let logs = get_logs_for(
      client.clone(),
      chain_id,
      vec![target_addr],
      events,
      from_block,
      concurrency,
   )
   .await?;

   tracing::info!(target: "zeus_eth::amm::sync", "Found {} logs for chain {} DEX: {}", logs.len(), chain_id, dex.to_str());
   let semaphore = Arc::new(Semaphore::new(concurrency));
   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let pools = Arc::new(Mutex::new(Vec::new()));

   for log_batch in logs.chunks(batch_size) {
      let client = client.clone();
      let pools = pools.clone();
      let semaphore = semaphore.clone();
      let chain_id = chain_id;
      let logs = log_batch.to_vec();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire().await?;
         let fetched_pools = sync_pools_from_log_batch(client, chain_id, dex, logs).await?;
         for pool in &fetched_pools {
         tracing::debug!(target: "zeus_eth::amm::sync", "Synced pool for {}-{} ChainId: {}", pool.currency0().symbol(), pool.currency1().symbol(), chain_id);
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
   let checkpoint = Checkpoint::new(chain_id, synced_block, dex, pools.len());
   tracing::info!(target: "zeus_eth::amm::sync", "Synced {} pools for {} - on ChainId {}", pools.len(), dex.to_str(), chain_id);
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