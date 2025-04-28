use alloy_rpc_types::Log;

use alloy_contract::private::{Network, Provider};

use std::sync::Arc;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};

use crate::{
   DexKind,
   uniswap::{v2::pool::UniswapV2Pool, v3::pool::UniswapV3Pool},
};
use abi::{
   alloy_sol_types::SolEvent,
   uniswap::{v2::factory::IUniswapV2Factory, v3::factory::IUniswapV3Factory},
};
use currency::erc20::ERC20Token;

use serde::{Deserialize, Serialize};

use tracing::error;
use types::{BlockTime, ChainId};
use utils::get_logs_for;

/// Sync pools from the last checkpoint
///
/// Useful to save all the synced pools in a json file and then sync from that checkpoint again
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
   pub chain_id: u64,
   pub block: u64,
   pub dex: DexKind,
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
///
/// If you are going to save them in a file, you probably want to use this struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncedPools {
   pub checkpoint: Checkpoint,
   pub v2_pools: Vec<UniswapV2Pool>,
   pub v3_pools: Vec<UniswapV3Pool>,
}

impl SyncedPools {
   pub fn new(checkpoint: Checkpoint, v2_pools: Vec<UniswapV2Pool>, v3_pools: Vec<UniswapV3Pool>) -> Self {
      Self {
         checkpoint,
         v2_pools,
         v3_pools,
      }
   }

   pub fn file_name(&self) -> String {
      let chain = ChainId::new(self.checkpoint.chain_id).expect("Unsupported ChainId");
      format!("{}.{}", self.checkpoint.dex.to_str(), chain.name())
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
/// - `from_block` From which block to start syncing, if None,
///  it will start from the dex factory creation block
#[derive(Debug, Clone)]
pub struct SyncConfig {
   pub chain_id: u64,
   pub dex: Vec<DexKind>,
   pub concurrency: usize,
   pub from_block: Option<BlockTime>,
}

impl SyncConfig {
   pub fn new(chain_id: u64, dex: Vec<DexKind>, concurrency: usize, from_block: Option<BlockTime>) -> Self {
      Self {
         chain_id,
         dex,
         concurrency,
         from_block,
      }
   }
}

/// Sync pools from the given checkpoint
pub async fn sync_from_checkpoint<P, N>(
   client: P,
   concurrency: usize,
   checkpoint: Checkpoint,
) -> Result<SyncedPools, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let from_block = BlockTime::Block(checkpoint.block);
   let config = SyncConfig::new(
      checkpoint.chain_id,
      vec![checkpoint.dex],
      concurrency,
      Some(from_block),
   );

   let synced = sync_pools(client, config).await?;

   let synced = if synced.get(0).is_none() {
      anyhow::bail!("No synced pools found")
   } else {
      synced.get(0).unwrap().clone()
   };

   Ok(synced)
}

/// Sync pools with the given configuration
///
/// See [SyncConfig]
pub async fn sync_pools<P, N>(client: P, config: SyncConfig) -> Result<Vec<SyncedPools>, anyhow::Error>
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
         error!("Chain sync task failed: {}", e);
      }
   }

   Ok(Arc::try_unwrap(results).unwrap().into_inner())
}

async fn do_sync_pools<P, N>(
   client: P,
   chain_id: u64,
   dex: DexKind,
   concurrency: usize,
   from_block: Option<BlockTime>,
) -> Result<SyncedPools, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let dex = dex;
   let factory = dex.factory(chain_id)?;
   let events = match dex {
      DexKind::UniswapV2 => vec![IUniswapV2Factory::PairCreated::SIGNATURE],
      DexKind::PancakeSwapV2 => vec![IUniswapV2Factory::PairCreated::SIGNATURE],
      DexKind::UniswapV3 => vec![IUniswapV3Factory::PoolCreated::SIGNATURE],
      DexKind::PancakeSwapV3 => vec![IUniswapV3Factory::PoolCreated::SIGNATURE],
      DexKind::UniswapV4 => panic!("Uniswap V4 not supported"),
   };

   let from_block = if from_block.is_none() {
      let block = dex.factory_creation_block(chain_id)?;
      BlockTime::Block(block)
   } else {
      from_block.unwrap()
   };

   let synced_block = client.get_block_number().await?;

   let logs = get_logs_for(
      client.clone(),
      chain_id,
      vec![factory],
      events,
      from_block,
      concurrency,
   )
   .await?;

   let semaphore = Arc::new(Semaphore::new(concurrency));
   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

   if dex.is_uniswap_v2() || dex.is_pancakeswap_v2() {
      let pools = Arc::new(Mutex::new(Vec::new()));

      for log in logs {
         let client = client.clone();
         let pools = pools.clone();
         let semaphore = semaphore.clone();
         let chain_id = chain_id;

         let task = tokio::spawn(async move {
            let _permit = semaphore.acquire().await?;
            let pool = v2_pool_from_log(client, chain_id, dex, &log).await?;
            pools.lock().await.push(pool);
            Ok(())
         });

         tasks.push(task);
      }

      for task in tasks {
         if let Err(e) = task.await {
            error!("Error syncing pool: {:?}", e);
         }
      }

      let pools = Arc::try_unwrap(pools).unwrap().into_inner();
      let checkpoint = Checkpoint::new(chain_id, synced_block, dex);
      let synced = SyncedPools::new(checkpoint, pools, Vec::new());
      return Ok(synced);
   } else if dex.is_uniswap_v3() || dex.is_pancakeswap_v3() {
      let pools = Arc::new(Mutex::new(Vec::new()));

      for log in logs {
         let client = client.clone();
         let pools = pools.clone();
         let semaphore = semaphore.clone();
         let chain_id = chain_id;

         let task = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let pool = v3_pool_from_log(client, chain_id, dex, &log).await?;
            pools.lock().await.push(pool);
            Ok(())
         });

         tasks.push(task);
      }

      for task in tasks {
         if let Err(e) = task.await {
            error!("Error syncing pool: {:?}", e);
         }
      }

      let pools = Arc::try_unwrap(pools).unwrap().into_inner();
      let checkpoint = Checkpoint::new(chain_id, synced_block, dex);
      let synced = SyncedPools::new(checkpoint, Vec::new(), pools);
      return Ok(synced);
   } else {
      anyhow::bail!("Unknown dex: {:?}", dex);
   }
}

async fn v2_pool_from_log<P, N>(
   client: P,
   chain_id: u64,
   dex: DexKind,
   log: &Log,
) -> Result<UniswapV2Pool, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let IUniswapV2Factory::PairCreated {
      token0,
      token1,
      pair,
      ..
   } = log.log_decode()?.inner.data;

   let token0_erc = if let Some(token) = ERC20Token::base_token(chain_id, token0) {
      token
   } else {
      ERC20Token::new(client.clone(), token0, chain_id).await?
   };

   let token1_erc = if let Some(token) = ERC20Token::base_token(chain_id, token1) {
      token
   } else {
      ERC20Token::new(client.clone(), token1, chain_id).await?
   };

   let pool = UniswapV2Pool::new(chain_id, pair, token0_erc, token1_erc, dex);
   Ok(pool)
}

async fn v3_pool_from_log<P, N>(
   client: P,
   chain_id: u64,
   dex: DexKind,
   log: &Log,
) -> Result<UniswapV3Pool, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let IUniswapV3Factory::PoolCreated {
      token0,
      token1,
      pool,
      fee,
      ..
   } = log.log_decode()?.inner.data;

   let token0_erc = if let Some(token) = ERC20Token::base_token(chain_id, token0) {
      token
   } else {
      ERC20Token::new(client.clone(), token0, chain_id).await?
   };

   let token1_erc = if let Some(token) = ERC20Token::base_token(chain_id, token1) {
      token
   } else {
      ERC20Token::new(client.clone(), token1, chain_id).await?
   };

   let fee: u32 = fee.to_string().parse()?;
   let pool = UniswapV3Pool::new(chain_id, pool, fee, token0_erc, token1_erc, dex);
   Ok(pool)
}
