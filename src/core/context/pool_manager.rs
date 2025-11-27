use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::{sync::Semaphore, task::JoinHandle};
use tracing::{info, trace};
use zeus_eth::abi::zeus::ZeusStateViewV2::PoolsState;
use zeus_eth::alloy_primitives::FixedBytes;
use zeus_eth::amm::uniswap::UniswapV4Pool;

use crate::core::{ZeusCtx, context::pool_data_dir, serde_hashmap};
use crate::utils::RT;
use zeus_eth::{
   abi::zeus::ZeusStateViewV2::{V3Pool, V4Pool},
   alloy_primitives::{Address, B256},
   alloy_provider::Provider,
   amm::uniswap::{
      AnyUniswapPool, DexKind, FEE_TIERS, FeeAmount, UniswapPool, UniswapV2Pool, UniswapV3Pool,
      state::{State, V3PoolState},
      sync::*,
      v4::pool::MAX_FEE,
   },
   currency::{Currency, ERC20Token},
   types::*,
   utils::{
      address_book::{uniswap_v2_factory, uniswap_v3_factory, uniswap_v4_stateview},
      batch,
   },
};

use anyhow::anyhow;

const POOL_MANAGER_DEFAULT: &str = include_str!("../../../pool_data.json");

// Timeout for pool sync in seconds (10 minutes)
const POOL_SYNC_TIMEOUT: u64 = 600;

/// A simple struct to identify a V2/V3/V4 pool
#[derive(PartialEq, Eq, Hash)]
pub struct PoolID {
   pub chain_id: u64,
   /// For V4 this is zero
   pub address: Address,
   /// For V2/V3 this is zero
   pub pool_id: B256,
}

impl PoolID {
   pub fn new(chain_id: u64, address: Address, pool_id: B256) -> Self {
      Self {
         chain_id,
         address,
         pool_id,
      }
   }
}

/// Thread-safe handle to the [PoolManager]
#[derive(Clone, Serialize, Deserialize)]
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

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = pool_data_dir()?;
      let data = std::fs::read(dir)?;
      let manager = serde_json::from_slice(&data)?;
      Ok(Self(Arc::new(RwLock::new(manager))))
   }

   pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
      let data = self.to_string()?;
      let dir = pool_data_dir()?;
      std::fs::write(dir, data)?;
      Ok(())
   }

   pub fn reset_default_settings(&self) {
      self.write(|manager| {
         manager.concurrency = default_concurrency();
         manager.batch_size_for_updating_pool_state = default_batch_size_for_updating_pool_state();
         manager.batch_size_for_syncing_pools = default_batch_size_for_syncing_pools();
         manager.sync_v4_pools = default_sync_v4_pools();
         manager.ignore_chains = default_ignore_chains();
      });
   }

   pub fn concurrency(&self) -> usize {
      let concurrency = self.read(|manager| manager.concurrency);
      if concurrency == 0 {
         default_concurrency()
      } else {
         concurrency
      }
   }

   pub fn batch_size_for_updating_pools_state(&self) -> usize {
      let size = self.read(|manager| manager.batch_size_for_updating_pool_state);
      if size == 0 {
         default_batch_size_for_updating_pool_state()
      } else {
         size
      }
   }

   pub fn batch_size_for_syncing_pools(&self) -> usize {
      let size = self.read(|manager| manager.batch_size_for_syncing_pools);
      if size == 0 {
         default_batch_size_for_syncing_pools()
      } else {
         size
      }
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
      self.read(|manager| manager.get_pool(chain_id, dex, fee, currency0, currency1).cloned())
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

   /// Update the state of the manager based on the given currencies and chain
   ///
   /// It also updates the token prices
   pub async fn update_for_currencies(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      currencies: Vec<Currency>,
   ) -> Result<(), anyhow::Error> {
      let mut pools_to_update = Vec::new();
      let mut inserted = HashSet::new();
      for currency in &currencies {
         let pools = self.get_pools_that_have_currency(currency);
         for pool in pools {
            let id = PoolID::new(pool.chain_id(), pool.address(), pool.id());
            if inserted.contains(&id) {
               continue;
            }
            inserted.insert(id);
            pools_to_update.push(pool);
         }
      }

      let _p = self._update_state_for_pools(ctx.clone(), chain, pools_to_update).await?;

      let tokens = currencies.iter().map(|c| c.to_erc20().into_owned()).collect();
      let price_manager = ctx.price_manager();
      price_manager.calculate_prices(ctx.clone(), chain, self.clone(), tokens).await?;

      Ok(())
   }

   /// Update the state of the manager for the given chain
   pub async fn update(&self, ctx: ZeusCtx, chain_id: u64) -> Result<(), anyhow::Error> {
      let pools = self.get_pools_for_chain(chain_id);
      let updated_pools = self.update_state_for_pools(ctx.clone(), chain_id, pools).await?;

      self.write(|manager| {
         for pool in updated_pools {
            manager.add_pool(pool);
         }
      });

      Ok(())
   }

   pub(crate) async fn _update_state_for_pools(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      pools: Vec<AnyUniswapPool>,
   ) -> Result<Vec<AnyUniswapPool>, anyhow::Error> {
      let concurrency = self.read(|manager| manager.concurrency);
      let batch_size = self.read(|manager| manager.batch_size_for_updating_pool_state);

      let updated_pools =
         batch_update_state(ctx.clone(), chain, concurrency, batch_size, pools).await?;

      self.write(|manager| {
         for pool in &updated_pools {
            manager.add_pool(pool.clone());
         }
      });

      Ok(updated_pools)
   }

   /// Update the state for the given pools
   ///
   /// It also updates the token prices, this is not ideal but this function is called
   /// from a lot of places and i dont want to forget calling the price manager manually
   pub async fn update_state_for_pools(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      pools: Vec<AnyUniswapPool>,
   ) -> Result<Vec<AnyUniswapPool>, anyhow::Error> {
      let pools = self._update_state_for_pools(ctx.clone(), chain, pools).await?;

      // ignore on tests
      if !cfg!(test) {
         let mut tokens_to_update = Vec::new();
         let mut inserted = HashSet::new();
         for pool in &pools {
            if !pool.currency0().is_base() {
               let token = pool.currency0().to_erc20().into_owned();

               if inserted.contains(&token.address) {
                  continue;
               }

               inserted.insert(token.address);
               tokens_to_update.push(token);
            }

            if !pool.currency1().is_base() {
               let token = pool.currency1().to_erc20().into_owned();

               if inserted.contains(&token.address) {
                  continue;
               }

               inserted.insert(token.address);
               tokens_to_update.push(token);
            }
         }

         let price_manager = ctx.price_manager();
         price_manager
            .calculate_prices(ctx.clone(), chain, self.clone(), tokens_to_update)
            .await?;
      }

      Ok(pools)
   }

   pub fn remove_v4_pools_with_no_base_token(&self) {
      self.write(|manager| manager.remove_v4_pools_with_no_base_token())
   }

   pub fn remove_v4_pools_with_high_fee(&self) {
      self.write(|manager| manager.remove_v4_pools_with_high_fee())
   }

   pub fn add_last_sync_time(&self, chain: u64, token_a: Address, token_b: Address) {
      self.write(|manager| manager.add_last_sync(chain, token_a, token_b))
   }

   pub fn add_v4_pool_last_sync_time(&self, chain: u64, dex: DexKind) {
      self.write(|manager| manager.add_v4_pool_last_sync(chain, dex))
   }

   fn get_last_sync(&self, chain: u64, token_a: Address, token_b: Address) -> Option<Instant> {
      self.read(|manager| manager.get_last_sync_time(chain, token_a, token_b))
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

   fn should_sync_pools(&self, chain: u64, token_a: Address, token_b: Address) -> bool {
      let now = Instant::now();
      let last_sync = self.get_last_sync(chain, token_a, token_b);
      if last_sync.is_none() {
         return true;
      }

      let last_sync = last_sync.unwrap();
      let timeout = Duration::from_secs(POOL_SYNC_TIMEOUT);
      let time_passed = now.duration_since(last_sync);
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
      let time_passed = now.duration_since(last_sync);
      time_passed > timeout
   }

   /// Sync all the possible V2/V3/V4 pools for the given tokens
   pub async fn sync_pools_for_tokens(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      tokens: Vec<ERC20Token>,
   ) -> Result<(), anyhow::Error> {
      if tokens.is_empty() {
         return Ok(());
      }

      let mut tasks: Vec<JoinHandle<Result<ERC20Token, anyhow::Error>>> = Vec::new();
      let semaphore = Arc::new(Semaphore::new(self.concurrency()));

      #[cfg(feature = "dev")]
      {
         let symbols = tokens.iter().map(|t| t.symbol.clone()).collect::<Vec<_>>();
         let addresses = tokens.iter().map(|t| t.address.to_string()).collect::<Vec<_>>();
         info!(
            "Syncing pools for {} {} Chain {}",
            symbols.join(", "),
            addresses.join(", "),
            chain
         );
      }

      for token in tokens {
         let semaphore = semaphore.clone();
         let ctx = ctx.clone();
         let manager = self.clone();
         let base_tokens = ERC20Token::base_tokens(chain);
         let token = token.clone();

         let task = RT.spawn(async move {
            let _permit = semaphore.acquire().await?;

            let v2_factory = uniswap_v2_factory(chain)?;
            let v3_factory = uniswap_v3_factory(chain)?;
            let state_view = uniswap_v4_stateview(chain)?;

            let mut v4_pools_map = HashMap::new();
            let mut v4_pool_ids = Vec::new();
            let mut bases_to_sync = Vec::new();

            for base_token in &base_tokens {
               if base_token.address == token.address {
                  continue;
               }

               let should_sync =
                  manager.should_sync_pools(chain, base_token.address, token.address);

               #[cfg(feature = "dev")]
               tracing::info!(
                  "Should Sync {} for {} {}-{}",
                  should_sync,
                  chain,
                  base_token.symbol,
                  token.symbol
               );

               if !should_sync {
                  continue;
               }

               bases_to_sync.push(base_token.clone());

               for fee in FEE_TIERS.iter() {
                  let fee_amount = FeeAmount::CUSTOM(*fee);
                  let pool = UniswapV4Pool::new(
                     chain,
                     fee_amount,
                     DexKind::UniswapV4,
                     Currency::from(base_token.clone()),
                     Currency::from(token.clone()),
                     State::none(),
                     Address::ZERO,
                  );

                  v4_pool_ids.push(pool.id());
                  v4_pools_map.insert(pool.id(), pool);
               }
            }

            if bases_to_sync.is_empty() {
               return Ok(token);
            }

            let mut tokens_map = HashMap::new();

            for base_token in &bases_to_sync {
               tokens_map.insert(base_token.address, base_token.clone());
            }

            tokens_map.insert(token.address, token.clone());

            let base_tokens_addr = bases_to_sync.iter().map(|t| t.address).collect::<Vec<_>>();
            let quote_token = token.address;
            let zeus_client = ctx.get_zeus_client();

            let pools = zeus_client
               .request(chain, |client| {
                  let v4_pool_ids = v4_pool_ids.clone();
                  let base_tokens_addr = base_tokens_addr.clone();
                  async move {
                     batch::get_pools(
                        client,
                        chain,
                        v2_factory,
                        v3_factory,
                        state_view,
                        v4_pool_ids,
                        base_tokens_addr,
                        quote_token,
                     )
                     .await
                     .map_err(|e| anyhow!("{:?}", e))
                  }
               })
               .await?;

            let v2_pools = &pools.v2Pools;
            let v3_pools = &pools.v3Pools;
            let v4_pools = &pools.v4Pools;

            for v2_pool in v2_pools {
               if v2_pool.addr.is_zero() {
                  continue;
               }

               let exists = manager.get_v2_pool_from_address(chain, v2_pool.addr).is_some();

               if exists {
                  continue;
               }

               let token_a = tokens_map.get(&v2_pool.tokenA);
               let token_b = tokens_map.get(&v2_pool.tokenB);

               if token_a.is_none() {
                  tracing::error!("V2Pool Token not found: {}", v2_pool.tokenA);
                  continue;
               }

               if token_b.is_none() {
                  tracing::error!("V2Pool Token not found: {}", v2_pool.tokenB);
                  continue;
               }

               let token_a = token_a.unwrap();
               let token_b = token_b.unwrap();

               let pool = UniswapV2Pool::new(
                  chain,
                  v2_pool.addr,
                  token_a.clone(),
                  token_b.clone(),
                  DexKind::UniswapV2,
               );

               manager.add_pool(pool);
            }

            for v3_pool in v3_pools {
               if v3_pool.addr.is_zero() {
                  continue;
               }

               let exists = manager.get_v3_pool_from_address(chain, v3_pool.addr).is_some();

               if exists {
                  continue;
               }

               let token_a = tokens_map.get(&v3_pool.tokenA);
               let token_b = tokens_map.get(&v3_pool.tokenB);

               if token_a.is_none() {
                  tracing::error!("V3Pool Token not found: {}", v3_pool.tokenA);
                  continue;
               }

               if token_b.is_none() {
                  tracing::error!("V3Pool Token not found: {}", v3_pool.tokenB);
                  continue;
               }

               let token_a = token_a.unwrap();
               let token_b = token_b.unwrap();
               let fee = v3_pool.fee.to_string().parse()?;

               let pool = UniswapV3Pool::new(
                  chain,
                  v3_pool.addr,
                  fee,
                  token_a.clone(),
                  token_b.clone(),
                  DexKind::UniswapV3,
               );

               manager.add_pool(pool);
            }

            for v4_pool in v4_pools {
               if *v4_pool == FixedBytes::<32>::ZERO {
                  continue;
               }

               let exists = manager.get_v4_pool_from_id(chain, *v4_pool).is_some();

               if exists {
                  continue;
               }

               let pool_full = v4_pools_map.get(v4_pool);

               if pool_full.is_none() {
                  tracing::error!("V4Pool not found: {}", v4_pool);
                  continue;
               }

               let pool_full = pool_full.unwrap();

               manager.add_pool(pool_full.clone());
            }

            for base_token in &bases_to_sync {
               manager.add_last_sync_time(chain, base_token.address, token.address);
            }

            Ok(token)
         });
         tasks.push(task);
      }

      for task in tasks {
         let time = Instant::now();
         let res = match task.await {
            Ok(res) => res,
            Err(e) => {
               tracing::error!("Error syncing pools: {:?}", e);
               continue;
            }
         };

         match res {
            Ok(token) => {
               info!(
                  "Synced Pools for {} in {} ms Chain {}",
                  token.symbol,
                  time.elapsed().as_millis(),
                  chain
               );
            }
            Err(e) => {
               tracing::error!("Error syncing pools: {:?}", e);
            }
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
      chain: ChainId,
      dex: DexKind,
      dir: Option<PathBuf>,
   ) -> Result<(), anyhow::Error> {
      let ignore_chains = self.read(|manager| manager.ignore_chains.clone());
      if ignore_chains.contains(&chain.id()) {
         return Ok(());
      }

      // For Base use an http endpoint because at some point the Ws just fails
      let http = chain.is_base();
      let client = ctx.get_archive_client(chain.id(), http).await?;
      let concurrency = self.read(|manager| manager.concurrency);
      let batch_size = self.read(|manager| manager.batch_size_for_syncing_pools);

      let latest_block = client.get_block_number().await?;

      if !self.should_sync_v4_pools(chain.id(), dex) {
         trace!(target: "zeus_eth::amm::pool_manager", "Skipping syncing V4 pools for chain {}", chain.id());
         return Ok(());
      }

      let checkpoint_opt = self.read(|manager| manager.get_checkpoint(chain.id(), dex));
      let mut from_block = if let Some(checkpoint) = &checkpoint_opt {
         checkpoint.block
      } else {
         dex.creation_block(chain.id())?
      };

      let chunk_size = BlockTime::Days(1);

      // Sync in incremental chunks, updating checkpoint after each
      while from_block < latest_block {
         let chunk_blocks = chunk_size.go_forward(chain.id(), from_block)? - from_block;
         let temp_to = std::cmp::min(from_block + chunk_blocks, latest_block);
         let dir = dir.clone();

         let config = SyncConfig::new(
            chain.id(),
            vec![dex],
            concurrency,
            batch_size,
            Some(from_block),
            Some(temp_to),
         );

         let synced = sync_pools(client.clone(), config, 50_000).await?;
         let mut pool_len = 0;

         for res in synced {
            self.write(|manager| {
               manager.add_checkpoint(
                  chain.id(),
                  res.checkpoint.dex,
                  res.checkpoint.clone(),
               );
            });

            for pool in res.pools {
               pool_len += 1;
               self.add_pool(pool);
            }
         }

         if let Some(dir) = dir {
            tracing::info!(
               "Saved {} pools from block {} to block {} for ChainId {}",
               pool_len,
               from_block,
               temp_to,
               chain.id()
            );
            self.save_to_dir(&dir)?;
         }

         from_block = temp_to + 1;
      }

      self.add_v4_pool_last_sync_time(chain.id(), dex);

      Ok(())
   }
}

/// Key: (chain_id, tokenA, tokenB)
type LastSync = HashMap<(u64, Address, Address), Instant>;

type V4PoolLastSync = HashMap<(u64, DexKind), Instant>;

/// Key: (chain_id, dex_kind, fee, tokenA, tokenB) -> Value: Pool
type Pools = HashMap<(u64, DexKind, u32, Currency, Currency), AnyUniswapPool>;

/// Key: (chain_id, dex) -> Value: Checkpoint
type CheckpointMap = HashMap<(u64, DexKind), Checkpoint>;

/// Ignore chains for V4 pool historic sync
type IgnoreChains = HashSet<u64>;

fn default_batch_size_for_updating_pool_state() -> usize {
   20
}

fn default_batch_size_for_syncing_pools() -> usize {
   30
}

fn default_concurrency() -> usize {
   4
}

fn default_sync_v4_pools() -> bool {
   false
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

   /// Last time we requested to sync pools for a token pair
   #[serde(skip)]
   pub last_sync: LastSync,

   /// V4 Pools are synced by using the `eth_get_logs` method so they get a different map
   #[serde(skip)]
   pub v4_pool_last_sync: V4PoolLastSync,

   #[serde(with = "serde_hashmap")]
   pub checkpoints: CheckpointMap,

   /// Set to 1 for no concurrency
   #[serde(default = "default_concurrency")]
   pub concurrency: usize,

   /// Batch size when syncing the pools state
   #[serde(default = "default_batch_size_for_updating_pool_state")]
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
         last_sync: HashMap::new(),
         v4_pool_last_sync: HashMap::new(),
         checkpoints: manager.checkpoints,
         concurrency: default_concurrency(),
         batch_size_for_updating_pool_state: default_batch_size_for_updating_pool_state(),
         batch_size_for_syncing_pools: default_batch_size_for_syncing_pools(),
         sync_v4_pools: default_sync_v4_pools(),
         ignore_chains: default_ignore_chains(),
      }
   }
}

impl PoolManager {
   fn add_last_sync(&mut self, chain: u64, token_a: Address, token_b: Address) {
      let key = (chain, token_a, token_b);
      self.last_sync.insert(key, Instant::now());
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

   fn get_last_sync_time(&self, chain: u64, token_a: Address, token_b: Address) -> Option<Instant> {
      let time1 = self.last_sync.get(&(chain, token_a, token_b)).cloned();
      let time2 = self.last_sync.get(&(chain, token_b, token_a)).cloned();
      time1.or(time2)
   }

   fn get_v4_pool_last_sync_time(&self, chain: u64, dex: DexKind) -> Option<Instant> {
      self.v4_pool_last_sync.get(&(chain, dex)).cloned()
   }

   /// Removes pool that exceed the [MAX_FEE]
   pub fn remove_v4_pools_with_high_fee(&mut self) {
      let mut keys = Vec::new();
      for (key, pool) in self.pools.iter() {
         if !pool.dex_kind().is_v4() {
            continue;
         }

         if pool.fee().fee_percent() > MAX_FEE {
            keys.push(key.clone());
         }
      }

      tracing::info!("Removed {} V4 pools with high fee", keys.len());

      for key in keys {
         self.pools.remove(&key);
      }

      self.pools.shrink_to_fit();
   }

   pub fn remove_v4_pools_with_no_base_token(&mut self) {
      let mut keys = Vec::new();
      for (key, pool) in self.pools.iter() {
         if !pool.dex_kind().is_v4() {
            continue;
         }

         if !pool.currency0().is_base() && !pool.currency1().is_base() {
            keys.push(key.clone());
         }
      }

      tracing::info!(
         "Removed {} V4 pools with no base tokens",
         keys.len()
      );

      for key in keys {
         self.pools.remove(&key);
      }

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
      self.pools.remove(&(chain_id, dex, fee, currency0, currency1));
   }

   /// Get any pools that includes the given currency
   pub fn get_pools_that_have_currency(&self, currency: &Currency) -> Vec<AnyUniswapPool> {
      let mut pools = Vec::new();
      for pool in self.pools.values() {
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
      for pool in self.pools.values() {
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
      self.pools.values().filter(|p| p.chain_id() == chain_id).cloned().collect()
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
      if let Some(pool) =
         self.pools.iter().find(|(_, p)| p.id() == pool_id && p.chain_id() == chain_id)
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
}

async fn batch_update_state(
   ctx: ZeusCtx,
   chain: u64,
   concurrency: usize,
   batch_size: usize,
   pools: Vec<AnyUniswapPool>,
) -> Result<Vec<AnyUniswapPool>, anyhow::Error> {
   if pools.is_empty() {
      return Ok(Vec::new());
   }

   let mut v2_pools = Vec::new();
   let mut v3_pools = Vec::new();
   let mut v4_pools = Vec::new();

   for pool in &pools {
      if pool.dex_kind().is_v2() {
         v2_pools.push(pool.clone());
      }

      if pool.dex_kind().is_v3() {
         v3_pools.push(pool.clone());
      }

      if pool.dex_kind().is_v4() {
         v4_pools.push(pool.clone());
      }
   }

   drop(pools);

   let all_v2_addresses: Vec<Address> = v2_pools
      .iter()
      .filter(|p| p.dex_kind().is_v2() && p.chain_id() == chain)
      .map(|p| p.address())
      .collect();

   let mut all_v3_pool_info = Vec::new();
   let mut all_v4_pool_info = Vec::new();

   for pool in &v3_pools {
      if pool.dex_kind().is_v3() && pool.chain_id() == chain {
         all_v3_pool_info.push(V3Pool {
            addr: pool.address(),
            tokenA: pool.currency0().address(),
            tokenB: pool.currency1().address(),
            fee: pool.fee().fee_u24(),
         });
      }
   }

   for pool in &v4_pools {
      if pool.dex_kind().is_v4() && pool.chain_id() == chain {
         all_v4_pool_info.push(V4Pool {
            pool: pool.id(),
            tickSpacing: pool.fee().tick_spacing(),
         });
      }
   }

   let zeus_client = ctx.get_zeus_client();
   let state_view = uniswap_v4_stateview(chain)?;

   #[cfg(feature = "dev")]
   let time = Instant::now();

   let batches = get_batches(
      batch_size,
      all_v2_addresses,
      all_v3_pool_info,
      all_v4_pool_info,
   );

   let mut tasks: Vec<JoinHandle<Result<PoolsState, anyhow::Error>>> = Vec::new();
   let semaphore = Arc::new(Semaphore::new(concurrency));

   for batch in &batches {
      let semaphore = semaphore.clone();
      let zeus_client = zeus_client.clone();
      let v2_chunk = batch.v2_pools.clone();
      let v3_chunk = batch.v3_pools.clone();
      let v4_chunk = batch.v4_pools.clone();

      let task = RT.spawn(async move {
         let _permit = semaphore.acquire().await?;
         let res = zeus_client
            .request(chain, move |client| {
               let v2_chunk = v2_chunk.clone();
               let v3_chunk = v3_chunk.clone();
               let v4_chunk = v4_chunk.clone();
               async move {
                  batch::get_pools_state(
                     client.clone(),
                     chain,
                     v2_chunk,
                     v3_chunk,
                     v4_chunk,
                     state_view,
                  )
                  .await
               }
            })
            .await?;

         Ok(res)
      });
      tasks.push(task);
   }

   let mut results = Vec::new();

   for task in tasks {
      let res = match task.await {
         Ok(res) => res,
         Err(e) => {
            tracing::error!("Error updating pool state: {:?}", e);
            continue;
         }
      };

      match res {
         Ok(res) => results.push(res),
         Err(e) => {
            tracing::error!("Error updating pool state: {:?}", e);
            continue;
         }
      }
   }

   let mut pool_state = PoolsState::default();

   for state in results {
      pool_state.v2Reserves.extend(state.v2Reserves);
      pool_state.v3PoolsData.extend(state.v3PoolsData);
      pool_state.v4PoolsData.extend(state.v4PoolsData);
   }

   let v2_reserves = &pool_state.v2Reserves;
   let v3_pool_state = &pool_state.v3PoolsData;
   let v4_pool_state = &pool_state.v4PoolsData;

   for pool in v2_pools.iter_mut() {
      for data in v2_reserves {
         if data.pool == pool.address() {
            pool.set_state(State::v2(data.clone().into()));
         }
      }
   }

   for pool in v3_pools.iter_mut() {
      for data in v3_pool_state {
         if data.pool == pool.address() {
            let state = V3PoolState::new(data.clone(), pool.fee().tick_spacing(), None)?;
            pool.set_state(State::v3(state));
            pool.v3_mut(|pool| {
               pool.liquidity_amount0 = data.tokenABalance;
               pool.liquidity_amount1 = data.tokenBBalance;
            });
         }
      }
   }

   for pool in v4_pools.iter_mut() {
      for data in v4_pool_state {
         if data.pool == pool.id() {
            let state = V3PoolState::for_v4(pool, data.clone())?;
            pool.set_state(State::v3(state));
            match pool.compute_virtual_reserves() {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!(
                     "Error computing virtual reserves for pool {} / {} ID: {} {:?}",
                     pool.currency0().symbol(),
                     pool.currency1().symbol(),
                     pool.id(),
                     e
                  );
               }
            }
         }
      }
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Updated pool state for {} V2 Pools {} V3 Pools {} V4 Pools in {} ms. Chain {}",
      v2_pools.len(),
      v3_pools.len(),
      v4_pools.len(),
      time.elapsed().as_millis(),
      chain
   );

   let mut pools = Vec::new();
   pools.extend(v2_pools);
   pools.extend(v3_pools);
   pools.extend(v4_pools);

   Ok(pools)
}

struct Batch {
   v2_pools: Vec<Address>,
   v3_pools: Vec<V3Pool>,
   v4_pools: Vec<V4Pool>,
}

fn get_batches(
   batch_size: usize,
   all_v2_addresses: Vec<Address>,
   all_v3_pool_info: Vec<V3Pool>,
   all_v4_pool_info: Vec<V4Pool>,
) -> Vec<Batch> {
   let total_len = all_v2_addresses.len() + all_v3_pool_info.len() + all_v4_pool_info.len();

   if total_len <= batch_size {
      return vec![Batch {
         v2_pools: all_v2_addresses,
         v3_pools: all_v3_pool_info,
         v4_pools: all_v4_pool_info,
      }];
   }

   // Process in concurrent batches, chunking each pool type proportionally
   // so each batch includes slices from all 3 types of pools.
   let total_pools = all_v2_addresses.len() + all_v3_pool_info.len() + all_v4_pool_info.len();
   let num_batches = (total_pools + batch_size - 1) / batch_size;

   let chunk_size_v2 = (all_v2_addresses.len() + num_batches - 1) / num_batches;
   let chunk_size_v3 = (all_v3_pool_info.len() + num_batches - 1) / num_batches;
   let chunk_size_v4 = (all_v4_pool_info.len() + num_batches - 1) / num_batches;

   let mut batches = Vec::new();

   for i in 0..num_batches {
      let len_v2 = all_v2_addresses.len();
      let start_v2 = i * chunk_size_v2;
      let v2_chunk = if start_v2 >= len_v2 {
         Vec::new()
      } else {
         let end_v2 = std::cmp::min(start_v2 + chunk_size_v2, len_v2);
         all_v2_addresses[start_v2..end_v2].to_vec()
      };

      let len_v3 = all_v3_pool_info.len();
      let start_v3 = i * chunk_size_v3;
      let v3_chunk = if start_v3 >= len_v3 {
         Vec::new()
      } else {
         let end_v3 = std::cmp::min(start_v3 + chunk_size_v3, len_v3);
         all_v3_pool_info[start_v3..end_v3].to_vec()
      };

      let len_v4 = all_v4_pool_info.len();
      let start_v4 = i * chunk_size_v4;
      let v4_chunk = if start_v4 >= len_v4 {
         Vec::new()
      } else {
         let end_v4 = std::cmp::min(start_v4 + chunk_size_v4, len_v4);
         all_v4_pool_info[start_v4..end_v4].to_vec()
      };

      let batch = Batch {
         v2_pools: v2_chunk,
         v3_pools: v3_chunk,
         v4_pools: v4_chunk,
      };

      batches.push(batch);
   }

   batches
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::amm::uniswap::UniswapV4Pool;

   #[test]
   fn test_default_init() {
      let _manager = PoolManagerHandle::default();
   }

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
