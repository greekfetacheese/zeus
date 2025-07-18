use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::trace;

use crate::core::{ZeusCtx, utils::RT};
use zeus_eth::{
   alloy_primitives::{Address, B256},
   amm::{
      DexKind, PoolID,
      sync::*,
      uniswap::{
         AnyUniswapPool, FEE_TIERS, FeeAmount, UniswapPool, UniswapV2Pool, UniswapV3Pool, state::*,
      },
   },
   currency::{Currency, ERC20Token},
   types::{ARBITRUM, BASE, BSC, OPTIMISM},
   utils::{NumericValue, batch, price_feed::get_base_token_price},
};

const POOL_MANAGER_DEFAULT: &str = include_str!("../../../pool_data.json");

// Timeout for pool sync in seconds (10 minutes)
const POOL_SYNC_TIMEOUT: u64 = 600;

/// Thread-safe handle to the [PoolManager]
#[derive(Clone)]
pub struct PoolManagerHandle(Arc<RwLock<PoolManager>>);

impl Default for PoolManagerHandle {
   fn default() -> Self {
      Self(Arc::new(RwLock::new(PoolManager::default())))
   }
}

impl PoolManagerHandle {
   pub fn new(pool_manager: PoolManager) -> Self {
      Self(Arc::new(RwLock::new(pool_manager)))
   }

   /// Shared access to the pool manager
   pub fn read<R>(&self, reader: impl FnOnce(&PoolManager) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   /// Exclusive mutable access to the pool manager
   pub fn write<R>(&self, writer: impl FnOnce(&mut PoolManager) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   /// Deserialize the [PoolManager] from a JSON string
   pub fn from_string(json: &str) -> Result<Self, serde_json::Error> {
      let manager = serde_json::from_str(json)?;
      Ok(Self(Arc::new(RwLock::new(manager))))
   }

   /// Serialize the [PoolManager] to a JSON string
   pub fn to_string(&self) -> Result<String, serde_json::Error> {
      self.read(|manager| serde_json::to_string(manager))
   }

   /// Deserialize the [PoolManager] from a JSON file
   pub fn from_dir(dir: &std::path::PathBuf) -> Result<Self, anyhow::Error> {
      let data = std::fs::read(dir)?;
      let manager = serde_json::from_slice(&data)?;
      Ok(Self(Arc::new(RwLock::new(manager))))
   }

   /// Serialize the [PoolManager] to a JSON file
   pub fn save_to_dir(&self, dir: &std::path::PathBuf) -> Result<(), anyhow::Error> {
      let data = self.read(|manager| serde_json::to_string(manager))?;
      std::fs::write(dir, data)?;
      Ok(())
   }

   pub fn reset_token_prices(&self) {
      self.write(|manager| manager.token_prices.clear());
   }

   pub fn concurrency(&self) -> usize {
      self.read(|manager| manager.concurrency)
   }

   pub fn batch_size_for_updating_pools_state(&self) -> usize {
      self.read(|manager| manager.batch_size_for_updating_pool_state)
   }

   pub fn batch_size_for_syncing_pools(&self) -> usize {
      self.read(|manager| manager.batch_size_for_syncing_pools)
   }

   pub fn do_we_sync_v4_pools(&self) -> bool {
      self.read(|manager| manager.sync_v4_pools)
   }

   pub fn ignore_chains(&self) -> IgnoreChains {
      self.read(|manager| manager.ignore_chains.clone())
   }

   pub fn set_ignore_chains(&self, ignore_chains: IgnoreChains) {
      self.write(|manager| manager.ignore_chains = ignore_chains);
   }

   pub fn set_sync_v4_pools(&self, sync_v4_pools: bool) {
      self.write(|manager| manager.sync_v4_pools = sync_v4_pools);
   }

   pub fn set_concurrency(&self, concurrency: usize) {
      self.write(|manager| manager.concurrency = concurrency);
   }

   pub fn set_batch_size_for_updating_pools_state(&self, batch_size: usize) {
      self.write(|manager| manager.batch_size_for_updating_pool_state = batch_size);
   }

   pub fn set_batch_size_for_syncing_pools(&self, batch_size: usize) {
      self.write(|manager| manager.batch_size_for_syncing_pools = batch_size);
   }

   /// Get all pools that include the given currency
   pub fn get_pools_that_have_currency(&self, currency: &Currency) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_pools_that_have_currency(currency))
   }

   pub fn get_pools_from_pair(
      &self,
      currency_a: &Currency,
      currency_b: &Currency,
   ) -> Vec<AnyUniswapPool> {
      self
         .read(|manager| manager.get_pools_from_pair(currency_a.chain_id(), currency_a, currency_b))
   }

   pub fn get_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_pools_for_chain(chain_id))
   }

   pub fn v2_pools_len(&self, chain: u64) -> usize {
      self.read(|manager| manager.v2_pools_len(chain))
   }

   pub fn v3_pools_len(&self, chain: u64) -> usize {
      self.read(|manager| manager.v3_pools_len(chain))
   }

   pub fn v4_pools_len(&self, chain: u64) -> usize {
      self.read(|manager| manager.v4_pools_len(chain))
   }

   pub fn get_v2_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_v2_pools_for_chain(chain_id))
   }

   pub fn get_v3_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_v3_pools_for_chain(chain_id))
   }

   pub fn get_v4_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_v4_pools_for_chain(chain_id))
   }

   pub fn get_pool(
      &self,
      chain_id: u64,
      dex: DexKind,
      fee: u32,
      currency0: &Currency,
      currency1: &Currency,
   ) -> Option<AnyUniswapPool> {
      self.read(|manager| {
         manager
            .get_pool(chain_id, dex, fee, currency0, currency1)
            .cloned()
      })
   }

   pub fn get_pool_from_address(&self, chain_id: u64, address: Address) -> Option<AnyUniswapPool> {
      self.read(|manager| manager.get_pool_from_address(chain_id, address).cloned())
   }

   pub fn get_v2_pool_from_address(
      &self,
      chain_id: u64,
      address: Address,
   ) -> Option<AnyUniswapPool> {
      self.read(|manager| manager.get_v2_pool_from_address(chain_id, address).cloned())
   }

   pub fn get_v3_pool_from_address(
      &self,
      chain_id: u64,
      address: Address,
   ) -> Option<AnyUniswapPool> {
      self.read(|manager| manager.get_v3_pool_from_address(chain_id, address).cloned())
   }

   pub fn get_v4_pool_from_id(&self, chain_id: u64, pool_id: B256) -> Option<AnyUniswapPool> {
      self.read(|manager| manager.get_v4_pool_from_id(chain_id, pool_id).cloned())
   }

   pub fn get_v3_pool_from_token_addresses_and_fee(
      &self,
      chain_id: u64,
      fee: u32,
      token_a: Address,
      token_b: Address,
   ) -> Option<AnyUniswapPool> {
      self.read(|manager| {
         manager
            .get_v3_pool_from_token_addresses_and_fee(chain_id, fee, token_a, token_b)
            .cloned()
      })
   }

   pub fn add_checkpoint(&self, chain: u64, dex: DexKind, checkpoint: Checkpoint) {
      self.write(|manager| manager.add_checkpoint(chain, dex, checkpoint));
   }

   pub fn add_pool(&self, pool: impl UniswapPool) {
      self.write(|manager| manager.add_pool(pool));
   }

   pub fn add_pools(&self, pools: Vec<AnyUniswapPool>) {
      self.write(|manager| {
         for pool in pools {
            manager.add_pool(pool);
         }
      });
   }

   pub fn remove_pool(
      &self,
      chain_id: u64,
      dex: DexKind,
      fee: u32,
      currency0: Currency,
      currency1: Currency,
   ) {
      self.write(|manager| manager.remove_pool(chain_id, dex, fee, currency0, currency1));
   }

   pub fn get_token_price(&self, token: &ERC20Token) -> NumericValue {
      self
         .read(|manager| manager.get_token_price(token))
         .unwrap_or_default()
   }

   /// Update the state of the manager for the given chain
   pub async fn update(&self, ctx: ZeusCtx, chain: u64) -> Result<(), anyhow::Error> {
      self.update_pool_state(ctx.clone(), chain).await?;
      self.update_base_token_prices(ctx.clone(), chain).await?;
      self.calculate_prices();
      Ok(())
   }

   /// Update the state of the manager based on the given currencies and chain
   pub async fn update_for_currencies(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      currencies: Vec<Currency>,
   ) -> Result<(), anyhow::Error> {
      let mut pools_to_update = Vec::new();
      let mut inserted = HashSet::new();
      for currency in currencies {
         let pools = self.get_pools_that_have_currency(&currency);
         for pool in pools {
            let id = PoolID::new(pool.chain_id(), pool.address(), pool.pool_id());
            if inserted.contains(&id) {
               continue;
            }
            inserted.insert(id);
            pools_to_update.push(pool);
         }
      }

      let concurrency = self.read(|manager| manager.concurrency);
      let batch_size = self.read(|manager| manager.batch_size_for_updating_pool_state);
      let client = ctx.get_client(chain).await?;

      let pools = batch_update_state(
         client.clone(),
         chain,
         concurrency,
         batch_size,
         pools_to_update,
      )
      .await?;

      self.write(|manager| {
         for pool in pools {
            manager.add_pool(pool.clone());
         }
      });

      self.update_base_token_prices(ctx, chain).await?;
      self.calculate_prices();
      Ok(())
   }

   async fn update_pool_state(&self, ctx: ZeusCtx, chain_id: u64) -> Result<(), anyhow::Error> {
      let pools = self.get_pools_for_chain(chain_id);
      let concurrency = self.read(|manager| manager.concurrency);
      let batch_size = self.read(|manager| manager.batch_size_for_updating_pool_state);
      let client = ctx.get_client(chain_id).await?;

      let pools = batch_update_state(client, chain_id, concurrency, batch_size, pools).await?;

      self.write(|manager| {
         for pool in pools {
            manager.add_pool(pool);
         }
      });

      Ok(())
   }

   /// Update the state for the given pools
   pub async fn update_state_for_pools(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      pools: Vec<impl UniswapPool>,
   ) -> Result<(), anyhow::Error> {
      let pools = pools
         .into_iter()
         .map(|p| AnyUniswapPool::from_pool(p))
         .collect::<Vec<_>>();
      let concurrency = self.read(|manager| manager.concurrency);
      let batch_size = self.read(|manager| manager.batch_size_for_updating_pool_state);
      let client = ctx.get_client(chain).await?;

      let pools = batch_update_state(
         client.clone(),
         chain,
         concurrency,
         batch_size,
         pools,
      )
      .await?;

      self.write(|manager| {
         for pool in pools {
            manager.add_pool(pool);
         }
      });

      self.update_base_token_prices(ctx, chain).await?;
      self.calculate_prices();
      Ok(())
   }

   /// Update the base token prices for the given tokens
   pub async fn update_base_token_prices(
      &self,
      ctx: ZeusCtx,
      chain: u64,
   ) -> Result<(), anyhow::Error> {
      let prices = PoolManager::fetch_base_token_prices(ctx, chain).await?;
      self.write(|manager| manager.set_token_prices(prices));
      Ok(())
   }

   /// Cleanup pools that do not have sufficient liquidity
   pub fn cleanup_pools(&self) {
      self.write(|manager| manager.cleanup_pools())
   }

   /// Cleanup V4 pools that do not have sufficient liquidity
   pub fn cleanup_v4_pools(&self) {
      self.write(|manager| manager.cleanup_v4_pools())
   }

   pub fn calculate_prices(&self) {
      self.write(|manager| manager.calculate_prices())
   }

   pub fn add_token_last_sync_time(
      &self,
      chain: u64,
      dex: DexKind,
      token_a: Address,
      token_b: Address,
   ) {
      self.write(|manager| manager.add_token_last_sync(chain, dex, token_a, token_b))
   }

   pub fn add_v4_pool_last_sync_time(&self, chain: u64, dex: DexKind) {
      self.write(|manager| manager.add_v4_pool_last_sync(chain, dex))
   }

   fn get_pool_last_sync(
      &self,
      chain: u64,
      dex: DexKind,
      token_a: Address,
      token_b: Address,
   ) -> Option<Instant> {
      self.read(|manager| manager.get_pool_last_sync_time(chain, dex, token_a, token_b))
   }

   fn get_v4_pool_last_sync(&self, chain: u64, dex: DexKind) -> Option<Instant> {
      self.read(|manager| manager.get_v4_pool_last_sync_time(chain, dex))
   }

   pub fn get_all_checkpoints(&self) -> Vec<Checkpoint> {
      self.read(|manager| manager.checkpoints.values().cloned().collect())
   }

   pub fn remove_checkpoint(&self, chain: u64, dex: DexKind) {
      self.write(|manager| manager.remove_checkpoint(chain, dex));
   }

   fn should_sync_pools(
      &self,
      chain: u64,
      dex: DexKind,
      token_a: Address,
      token_b: Address,
   ) -> bool {
      let now = Instant::now();
      let last_sync = self.get_pool_last_sync(chain, dex, token_a, token_b);
      if last_sync.is_none() {
         return true;
      }

      let last_sync = last_sync.unwrap();
      let timeout = Duration::from_secs(POOL_SYNC_TIMEOUT);
      let time_passed = now - last_sync;
      time_passed > timeout
   }

   fn should_sync_v4_pools(&self, chain: u64, dex: DexKind) -> bool {
      let now = Instant::now();
      let last_sync = self.get_v4_pool_last_sync(chain, dex);
      if last_sync.is_none() {
         return true;
      }

      let last_sync = last_sync.unwrap();
      let timeout = Duration::from_secs(POOL_SYNC_TIMEOUT);
      let time_passed = now - last_sync;
      time_passed > timeout
   }

   /// Sync pools for the given tokens based on:
   ///
   /// - The token's chain id
   /// - The [DexKind]
   /// - Base Tokens [ERC20Token::base_tokens()]
   ///
   /// `sync_v4` whether to sync v4 pools or not (`Archive node required)`
   pub async fn sync_pools_for_tokens(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      tokens: Vec<ERC20Token>,
      dex_kinds: Vec<DexKind>,
      sync_v4: bool,
   ) -> Result<(), anyhow::Error> {
      for token in tokens {
         self
            .sync_v2_pools_for_token(ctx.clone(), token.clone(), dex_kinds.clone())
            .await?;

         self
            .sync_v3_pools_for_token(ctx.clone(), token.clone(), dex_kinds.clone())
            .await?;
      }

      if sync_v4 {
         let dexes = vec![DexKind::UniswapV4];
         trace!(target: "zeus_eth::amm::pool_manager", "Syncing V4 pools for chain {}", chain);
         self.sync_pools(ctx.clone(), chain, dexes.clone()).await?;
      }

      Ok(())
   }

   /// Sync all V2 pools for the given token that are paired with [ERC20Token::base_tokens()]
   pub async fn sync_v2_pools_for_token(
      &self,
      ctx: ZeusCtx,
      token: ERC20Token,
      dex_kinds: Vec<DexKind>,
   ) -> Result<(), anyhow::Error> {
      let client = ctx.get_client(token.chain_id).await?;
      let chain = token.chain_id;
      let base_tokens = ERC20Token::base_tokens(chain);

      let concurrency = self.concurrency();
      let semaphore = Arc::new(Semaphore::new(concurrency));
      let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

      for base_token in base_tokens {
         if base_token.address == token.address {
            continue;
         }

         let manager = self.clone();
         let base_token = base_token.clone();
         let token = token.clone();
         let semaphore = semaphore.clone();
         let dex_kinds = dex_kinds.clone();
         let client = client.clone();

         let task = RT.spawn(async move {
            let _permit = semaphore.acquire().await?;

         let currency_a: Currency = base_token.clone().into();
         let currency_b: Currency = token.clone().into();

         for dex in &dex_kinds {
            if !dex.is_v2() {
               continue;
            }

            if !manager.should_sync_pools(
               token.chain_id,
               *dex,
               token.address,
               base_token.address,
            ) {
               continue;
            }

            let cached_pool = manager.get_pool(
               chain,
               *dex,
               FeeAmount::MEDIUM.fee(),
               &currency_a,
               &currency_b,
            );
            if cached_pool.is_some() {
               continue;
            }

            let pool_res = UniswapV2Pool::from(
               client.clone(),
               token.chain_id,
               token.clone(),
               base_token.clone(),
               *dex,
            )
            .await;

            if let Ok(pool) = pool_res {
               trace!(
                  target: "zeus_eth::amm::pool_manager", "Got {} pool {} for {}-{} for Chain Id: {}",
                  dex.as_str(),
                  pool.address(),
                  pool.token0().symbol,
                  pool.token1().symbol,
                  token.chain_id
               );
               manager.add_pool(pool);
            }


            manager.add_token_last_sync_time(
               token.chain_id,
               *dex,
               token.address,
               base_token.address,
            );
         }
         Ok(())
      });
         tasks.push(task);
      }

      for task in tasks {
         match task.await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error syncing pools: {:?}", e),
         }
      }
      Ok(())
   }

   /// Sync all the V3 pools for the given token that are paired with [ERC20Token::base_tokens()]
   pub async fn sync_v3_pools_for_token(
      &self,
      ctx: ZeusCtx,
      token: ERC20Token,
      dex_kinds: Vec<DexKind>,
   ) -> Result<(), anyhow::Error> {
      let client = ctx.get_client(token.chain_id).await?;
      let chain = token.chain_id;
      let base_tokens = ERC20Token::base_tokens(chain);

      let concurrency = self.concurrency();
      let semaphore = Arc::new(Semaphore::new(concurrency));
      let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

      for base_token in &base_tokens {
         if base_token.address == token.address {
            continue;
         }

         let manager = self.clone();
         let base_token = base_token.clone();
         let token = token.clone();
         let semaphore = semaphore.clone();
         let dex_kinds = dex_kinds.clone();
         let client = client.clone();
         let task = RT.spawn(async move {
            let _permit = semaphore.acquire().await?;

            for dex in &dex_kinds {
               if !dex.is_v3() {
                  continue;
               }

               if !manager.should_sync_pools(
                  token.chain_id,
                  *dex,
                  token.address,
                  base_token.address,
               ) {
                  continue;
               }

               let currency_a: Currency = base_token.clone().into();
               let currency_b: Currency = token.clone().into();

               let mut pools_exists = [false; FEE_TIERS.len()];
               for (i, fee) in FEE_TIERS.iter().enumerate() {
                  let pool = manager.get_pool(chain, *dex, *fee, &currency_a, &currency_b);
                  if pool.is_some() {
                     pools_exists[i] = true;
                  }
               }

               if pools_exists.iter().all(|b| *b == true) {
                  continue;
               }

               let factory = dex.factory(token.chain_id)?;
               let pools = batch::get_v3_pools(
                  client.clone(),
                  token.address,
                  base_token.address,
                  factory,
               )
               .await?;

               for pool in &pools {
                  if !pool.addr.is_zero() {
                     let fee = pool.fee.to_string().parse()?;
                     let v3_pool = UniswapV3Pool::new(
                        token.chain_id,
                        pool.addr,
                        fee,
                        token.clone(),
                        base_token.clone(),
                        *dex,
                     );

                     trace!(
                        target: "zeus_eth::amm::pool_manager", "Got {} pool {} for {}/{} - Fee: {}",
                        dex.as_str(),
                        v3_pool.address,
                        v3_pool.token0().symbol,
                        v3_pool.token1().symbol,
                        v3_pool.fee.fee()
                     );

                     manager.add_pool(v3_pool);
                  }
               }
               manager.add_token_last_sync_time(
                  token.chain_id,
                  *dex,
                  token.address,
                  base_token.address,
               );
            }
            Ok(())
         });
         tasks.push(task);
      }

      for task in tasks {
         match task.await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error syncing pools: {:?}", e),
         }
      }

      Ok(())
   }

   /// Sync pools from logs
   ///
   /// Archive node is required
   pub async fn sync_pools(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      dexes: Vec<DexKind>,
   ) -> Result<(), anyhow::Error> {
      let ignore_chains = self.read(|manager| manager.ignore_chains.clone());
      if ignore_chains.contains(&chain) {
         return Ok(());
      }

      let client = ctx.get_client(chain).await?;
      let concurrency = self.read(|manager| manager.concurrency);
      let batch_size = self.read(|manager| manager.batch_size_for_syncing_pools);
      let semaphore = Arc::new(Semaphore::new(concurrency.into()));
      let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
      let results = Arc::new(Mutex::new(Vec::new()));

      for dex in &dexes {
         if !self.should_sync_v4_pools(chain, *dex) {
            trace!(target: "zeus_eth::amm::pool_manager", "Skipping syncing V4 pools for chain {}", chain);
            continue;
         }

         let client = client.clone();
         let results = results.clone();
         let semaphore = semaphore.clone();
         let dex = dex.clone();

         let checkpoint = self.read(|manager| manager.get_checkpoint(chain, dex));
         let manager = self.clone();

         if checkpoint.is_some() {
            let task = RT.spawn(async move {
               let checkpoint = checkpoint.unwrap();
               let _permit = semaphore.acquire().await?;
               let synced = sync_from_checkpoints(
                  client.clone(),
                  concurrency,
                  batch_size,
                  vec![checkpoint],
               )
               .await?;
               manager.add_v4_pool_last_sync_time(chain, dex);
               results.lock().await.extend(synced);
               Ok(())
            });
            tasks.push(task);
         } else {
            let task = RT.spawn(async move {
               let config = SyncConfig::new(chain, vec![dex], concurrency, batch_size, None);
               let _permit = semaphore.acquire().await?;
               let synced = sync_pools(client.clone(), config).await?;
               manager.add_v4_pool_last_sync_time(chain, dex);
               results.lock().await.extend(synced);
               Ok(())
            });
            tasks.push(task);
         }
      }

      for task in tasks {
         match task.await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error syncing pools: {:?}", e),
         }
      }

      let results = Arc::try_unwrap(results).unwrap().into_inner();

      for res in results {
         self.write(|manager| {
            manager.add_checkpoint(chain, res.checkpoint.dex, res.checkpoint.clone());
         });
         for pool in res.pools {
            self.add_pool(pool);
         }
      }

      Ok(())
   }
}

/// Key: (chain_id, dex, tokenA, tokenB) -> Value: Time since last sync
type PoolLastSync = HashMap<(u64, DexKind, Address, Address), Instant>;

type V4PoolLastSync = HashMap<(u64, DexKind), Instant>;

/// Key: (chain_id, dex_kind, fee, tokenA, tokenB) -> Value: Pool
type Pools = HashMap<(u64, DexKind, u32, Currency, Currency), AnyUniswapPool>;

/// Token Prices
///
/// Key: (chain_id, token) -> Value: Price
type TokenPrices = HashMap<(u64, Address), NumericValue>;

/// Key: (chain_id, dex) -> Value: Checkpoint
type CheckpointMap = HashMap<(u64, DexKind), Checkpoint>;

/// Ignore chains for V4 pool historic sync
type IgnoreChains = HashSet<u64>;

fn default_batch_size() -> usize {
   10
}

fn default_batch_size_for_syncing_pools() -> usize {
   30
}

fn default_concurrency() -> usize {
   2
}

fn default_sync_v4_pools() -> bool {
   true
}

fn default_ignore_chains() -> IgnoreChains {
   let mut chains = HashSet::new();
   chains.insert(BASE);
   chains.insert(OPTIMISM);
   chains.insert(BSC);
   chains.insert(ARBITRUM);
   chains
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PoolManager {
   #[serde(with = "serde_hashmap")]
   pub pools: Pools,

   #[serde(with = "serde_hashmap")]
   pub token_prices: TokenPrices,

   /// Last time we requested to sync a specific pool
   #[serde(skip)]
   pub pool_last_sync: PoolLastSync,

   /// V4 Pools are synced by using the `eth_get_logs` method so they get a different map
   #[serde(skip)]
   pub v4_pool_last_sync: V4PoolLastSync,

   #[serde(with = "serde_hashmap")]
   pub checkpoints: CheckpointMap,

   /// Set to 1 for no concurrency
   #[serde(default = "default_concurrency")]
   pub concurrency: usize,

   /// Batch size when syncing the pools state
   #[serde(default = "default_batch_size")]
   pub batch_size_for_updating_pool_state: usize,

   /// Batch size when syncing pools from logs
   #[serde(default = "default_batch_size_for_syncing_pools")]
   pub batch_size_for_syncing_pools: usize,

   #[serde(default = "default_sync_v4_pools")]
   pub sync_v4_pools: bool,

   #[serde(default = "default_ignore_chains")]
   pub ignore_chains: IgnoreChains,
}

impl Default for PoolManager {
   fn default() -> Self {
      let manager: PoolManager = serde_json::from_str(POOL_MANAGER_DEFAULT).unwrap();
      Self {
         pools: manager.pools,
         token_prices: manager.token_prices,
         pool_last_sync: HashMap::new(),
         v4_pool_last_sync: HashMap::new(),
         checkpoints: manager.checkpoints,
         concurrency: default_concurrency(),
         batch_size_for_updating_pool_state: default_batch_size(),
         batch_size_for_syncing_pools: default_batch_size_for_syncing_pools(),
         sync_v4_pools: default_sync_v4_pools(),
         ignore_chains: default_ignore_chains(),
      }
   }
}

impl PoolManager {
   fn add_token_last_sync(&mut self, chain: u64, dex: DexKind, token_a: Address, token_b: Address) {
      let key = (chain, dex, token_a, token_b);
      self.pool_last_sync.insert(key, Instant::now());
   }

   fn add_v4_pool_last_sync(&mut self, chain: u64, dex: DexKind) {
      let key = (chain, dex);
      self.v4_pool_last_sync.insert(key, Instant::now());
   }

   fn add_checkpoint(&mut self, chain: u64, dex: DexKind, checkpoint: Checkpoint) {
      let key = (chain, dex);
      self.checkpoints.insert(key, checkpoint);
   }

   fn remove_checkpoint(&mut self, chain: u64, dex: DexKind) {
      let key = (chain, dex);
      self.checkpoints.remove(&key);
   }

   fn get_checkpoint(&self, chain: u64, dex: DexKind) -> Option<Checkpoint> {
      let key = (chain, dex);
      self.checkpoints.get(&key).cloned()
   }

   fn get_pool_last_sync_time(
      &self,
      chain: u64,
      dex: DexKind,
      token_a: Address,
      token_b: Address,
   ) -> Option<Instant> {
      let time1 = self
         .pool_last_sync
         .get(&(chain, dex, token_a, token_b))
         .cloned();
      let time2 = self
         .pool_last_sync
         .get(&(chain, dex, token_b, token_a))
         .cloned();
      time1.or(time2)
   }

   fn get_v4_pool_last_sync_time(&self, chain: u64, dex: DexKind) -> Option<Instant> {
      self.v4_pool_last_sync.get(&(chain, dex)).cloned()
   }

   pub fn get_token_price(&self, token: &ERC20Token) -> Option<NumericValue> {
      self
         .token_prices
         .get(&(token.chain_id, token.address))
         .cloned()
   }

   pub fn set_token_prices(&mut self, prices: TokenPrices) {
      for (key, price) in prices {
         self.token_prices.insert(key, price);
      }
   }

   pub fn cleanup_pools(&mut self) {
      self.pools.retain(|_, pool| {
         pool.enough_liquidity() || pool.currency0().is_base() && pool.currency1().is_base()
      });

      self.pools.shrink_to_fit();
   }

   /// Removes V4 pools that have no liquidity
   pub fn cleanup_v4_pools(&mut self) {
      self.pools.retain(|_key, pool| {
         // Keep the pool if it's NOT a V4 pool, OR
         // if it IS a V4 pool AND it has enough liquidity.
         // Also ignore pools that are base pairs
         !pool.dex_kind().is_v4()
            || pool.enough_liquidity()
            || pool.currency0().is_base() && pool.currency1().is_base()
      });

      self.pools.shrink_to_fit();
   }

   pub fn add_pool(&mut self, pool: impl UniswapPool) {
      let any_pool = AnyUniswapPool::from_pool(pool);
      let key = (
         any_pool.chain_id(),
         any_pool.dex_kind(),
         any_pool.fee().fee(),
         any_pool.currency0().clone(),
         any_pool.currency1().clone(),
      );

      self.pools.insert(key, any_pool);
   }

   pub fn remove_pool(
      &mut self,
      chain_id: u64,
      dex: DexKind,
      fee: u32,
      currency0: Currency,
      currency1: Currency,
   ) {
      self
         .pools
         .remove(&(chain_id, dex, fee, currency0, currency1));
   }

   /// Get any pools that includes the given currency
   pub fn get_pools_that_have_currency(&self, currency: &Currency) -> Vec<AnyUniswapPool> {
      let mut pools = Vec::new();
      for (_, pool) in &self.pools {
         if pool.chain_id() != currency.chain_id() {
            continue;
         }
         if pool.have(currency) {
            pools.push(pool.clone());
         }
      }
      pools
   }

   /// Get all pools for this currency pair
   pub fn get_pools_from_pair(
      &self,
      chain_id: u64,
      currency_a: &Currency,
      currency_b: &Currency,
   ) -> Vec<AnyUniswapPool> {
      let mut pools = Vec::new();
      for (_, pool) in &self.pools {
         if pool.chain_id() != chain_id {
            continue;
         }
         if pool.is_currency0(currency_a) && pool.is_currency1(currency_b) {
            pools.push(pool.clone());
         } else if pool.is_currency0(currency_b) && pool.is_currency1(currency_a) {
            pools.push(pool.clone());
         }
      }
      pools
   }

   /// Get all pools for the given chain
   pub fn get_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self
         .pools
         .values()
         .filter(|p| p.chain_id() == chain_id)
         .cloned()
         .collect()
   }

   pub fn v2_pools_len(&self, chain: u64) -> usize {
      self
         .pools
         .values()
         .filter(|p| p.chain_id() == chain && p.dex_kind().is_v2())
         .count()
   }

   pub fn v3_pools_len(&self, chain: u64) -> usize {
      self
         .pools
         .values()
         .filter(|p| p.chain_id() == chain && p.dex_kind().is_v3())
         .count()
   }

   pub fn v4_pools_len(&self, chain: u64) -> usize {
      self
         .pools
         .values()
         .filter(|p| p.chain_id() == chain && p.dex_kind().is_v4())
         .count()
   }

   pub fn get_v2_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self
         .pools
         .values()
         .filter(|p| p.chain_id() == chain_id && p.dex_kind().is_v2())
         .cloned()
         .collect()
   }

   pub fn get_v3_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self
         .pools
         .values()
         .filter(|p| p.chain_id() == chain_id && p.dex_kind().is_v3())
         .cloned()
         .collect()
   }

   pub fn get_v4_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self
         .pools
         .values()
         .filter(|p| p.chain_id() == chain_id && p.dex_kind().is_v4())
         .cloned()
         .collect()
   }

   pub fn get_pool(
      &self,
      chain_id: u64,
      dex: DexKind,
      fee: u32,
      currency_a: &Currency,
      currency_b: &Currency,
   ) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self.pools.get(&(
         chain_id,
         dex,
         fee,
         currency_a.clone(),
         currency_b.clone(),
      )) {
         return Some(pool);
      } else if let Some(pool) = self.pools.get(&(
         chain_id,
         dex,
         fee,
         currency_b.clone(),
         currency_a.clone(),
      )) {
         return Some(pool);
      } else {
         return None;
      }
   }

   pub fn get_v2_pool_from_address(
      &self,
      chain_id: u64,
      address: Address,
   ) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self
         .pools
         .iter()
         .find(|(_, p)| p.address() == address && p.chain_id() == chain_id && p.dex_kind().is_v2())
      {
         Some(pool.1)
      } else {
         None
      }
   }

   pub fn get_v3_pool_from_address(
      &self,
      chain_id: u64,
      address: Address,
   ) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self
         .pools
         .iter()
         .find(|(_, p)| p.address() == address && p.chain_id() == chain_id && p.dex_kind().is_v3())
      {
         Some(pool.1)
      } else {
         None
      }
   }

   pub fn get_v4_pool_from_id(&self, chain_id: u64, pool_id: B256) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self
         .pools
         .iter()
         .find(|(_, p)| p.pool_id() == pool_id && p.chain_id() == chain_id)
      {
         Some(pool.1)
      } else {
         None
      }
   }

   pub fn get_v3_pool_from_token_addresses_and_fee(
      &self,
      chain_id: u64,
      fee: u32,
      token_a: Address,
      token_b: Address,
   ) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self.pools.iter().find(|(_, p)| {
         p.currency0().address() == token_a
            && p.currency1().address() == token_b
            && p.fee().fee() == fee
            && p.chain_id() == chain_id
            && p.dex_kind().is_v3()
      }) {
         Some(pool.1)
      } else if let Some(pool) = self.pools.iter().find(|(_, p)| {
         p.currency1().address() == token_b
            && p.currency0().address() == token_a
            && p.fee().fee() == fee
            && p.chain_id() == chain_id
            && p.dex_kind().is_v3()
      }) {
         Some(pool.1)
      } else {
         None
      }
   }

   pub fn get_pool_from_address(&self, chain_id: u64, address: Address) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self
         .pools
         .iter()
         .find(|(_, p)| p.address() == address && p.chain_id() == chain_id)
      {
         Some(pool.1)
      } else {
         None
      }
   }

   pub fn calculate_prices(&mut self) {
      for (_, pool) in &self.pools {
         if !pool.enough_liquidity() {
            continue;
         }

         let chain = pool.chain_id();
         let quote_token = pool.quote_currency();
         let base_token = pool.base_currency();

         // if both tokens are base tokens, skip
         if quote_token.is_base() && base_token.is_base() {
            continue;
         }

         let base_price = self
            .get_token_price(&base_token.to_erc20())
            .unwrap_or_default();
         let quote_price = pool.quote_price(base_price.f64()).unwrap_or_default();
         if quote_price == 0.0 {
            continue;
         }

         let key = (chain, quote_token.address());
         let quote_price = NumericValue::currency_price(quote_price);
         self.token_prices.insert(key, quote_price);
      }
   }

   async fn fetch_base_token_prices(
      ctx: ZeusCtx,
      chain_id: u64,
   ) -> Result<TokenPrices, anyhow::Error> {
      let client = ctx.get_client(chain_id).await?;
      let mut prices = HashMap::new();
      let tokens = ERC20Token::base_tokens(chain_id);
      for token in tokens {
         let price = get_base_token_price(
            client.clone(),
            token.chain_id,
            token.address,
            None,
         )
         .await?;
         prices.insert(
            (token.chain_id, token.address),
            NumericValue::currency_price(price),
         );
      }
      Ok(prices)
   }
}

mod serde_hashmap {
   use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
   use std::collections::HashMap;

   pub fn serialize<S, K, V>(map: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: Serializer,
      K: Serialize,
      V: Serialize,
   {
      let stringified_map: HashMap<String, &V> = map
         .iter()
         .map(|(k, v)| (serde_json::to_string(k).unwrap(), v))
         .collect();
      stringified_map.serialize(serializer)
   }

   pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
   where
      D: Deserializer<'de>,
      K: DeserializeOwned + std::cmp::Eq + std::hash::Hash,
      V: DeserializeOwned,
   {
      let stringified_map: HashMap<String, V> = HashMap::deserialize(deserializer)?;
      stringified_map
         .into_iter()
         .map(|(k, v)| {
            let key = serde_json::from_str(&k).map_err(serde::de::Error::custom)?;
            Ok((key, v))
         })
         .collect()
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::amm::UniswapV4Pool;

   #[test]
   fn serde_works() {
      let pool = UniswapV2Pool::weth_uni();
      let pool2 = UniswapV4Pool::eth_uni();
      let pool_manager = PoolManager::default();
      let handle = PoolManagerHandle::new(pool_manager);

      handle.add_pool(pool);
      handle.add_pool(pool2);
      let checkpoint = Checkpoint::default();
      handle.add_checkpoint(1, DexKind::UniswapV2, checkpoint);
      let json = handle.to_string().unwrap();

      let _handle2 = PoolManagerHandle::from_string(&json).unwrap();
   }
}
