use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::{error, info, trace};
use zeus_eth::amm::uniswap::UniswapV4Pool;

use crate::core::{ZeusCtx, context::pool_data_dir, serde_hashmap, utils::RT};
use zeus_eth::{
   abi::zeus::ZeusStateView,
   alloy_primitives::{Address, B256},
   alloy_provider::Provider,
   amm::uniswap::{
      AnyUniswapPool, DexKind, FEE_TIERS, FeeAmount, UniswapPool, UniswapV2Pool,
      UniswapV3Pool,
      state::{State, V3PoolState},
      sync::*,
      v4::pool::MAX_FEE,
   },
   currency::{Currency, ERC20Token, NativeCurrency},
   types::*,
   utils::batch,
};

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

   pub fn concurrency(&self) -> usize {
      let concurrency = self.read(|manager| manager.concurrency);
      if concurrency == 0 {
         1
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

   /// Sync pools for the given tokens based on:
   ///
   /// - The token's chain id
   /// - The [DexKind]
   /// - Base Tokens [ERC20Token::base_tokens()]
   pub async fn sync_pools_for_tokens(
      &self,
      ctx: ZeusCtx,
      tokens: Vec<ERC20Token>,
      dex_kinds: Vec<DexKind>,
   ) -> Result<(), anyhow::Error> {
      if tokens.is_empty() {
         return Ok(());
      }

      let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
      let semaphore = Arc::new(Semaphore::new(self.concurrency()));

      let token_len = tokens.len();
      let chain = tokens[0].chain_id;

      for token in tokens {
         let semaphore = semaphore.clone();
         let ctx = ctx.clone();
         let manager = self.clone();
         let dex_kinds = dex_kinds.clone();
         let token = token.clone();

         let task = RT.spawn(async move {
            let _permit = semaphore.acquire().await?;

            manager
               .sync_v2_pools_for_token(ctx.clone(), token.clone(), dex_kinds.clone())
               .await?;

            manager
               .sync_v3_pools_for_token(ctx.clone(), token.clone(), dex_kinds.clone())
               .await?;

            manager
               .sync_v4_pools_for_token(ctx.clone(), token.clone(), dex_kinds.clone())
               .await?;

            Ok(())
         });
         tasks.push(task);
      }

      let mut synced = 0;

      for task in tasks {
         match task.await {
            Ok(_) => {
               synced += 1;
               info!(
                  "Synced Token Pool Data {} out of {} tokens Chain {}",
                  synced, token_len, chain
               );
            }
            Err(e) => tracing::error!("Error syncing pools: {:?}", e),
         }
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
      let client = ctx.get_zeus_client();
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

            let pool_res = client.request(token.chain_id, |client| {
               let token_clone = token.clone();
               let base_token_clone = base_token.clone();
               async move {
                  UniswapV2Pool::from_components(
                     client,
                     chain,
                     token_clone,
                     base_token_clone,
                     *dex,
                  )
                  .await
               }
            }).await?;

            if let Some(pool) = pool_res {
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
      let client = ctx.get_zeus_client();
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
               let pools = client
                  .request(chain, |client| async move {
                     batch::get_v3_pools(
                        client,
                        chain,
                        factory,
                        token.address,
                        base_token.address,
                     )
                     .await
                  })
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

   /// Sync all the V4 pools for the given token that are paired with [ERC20Token::base_tokens()]
   ///
   /// This is a best-effort sync without relying on a archive node
   /// Most pools still use the V3 standard fee tier so it should work pretty well for most tokens
   pub async fn sync_v4_pools_for_token(
      &self,
      ctx: ZeusCtx,
      token: ERC20Token,
      dex_kinds: Vec<DexKind>,
   ) -> Result<(), anyhow::Error> {
      let client = ctx.get_zeus_client();
      let chain = token.chain_id;
      let base_tokens = ERC20Token::base_tokens(chain);

      let concurrency = self.concurrency();
      let semaphore = Arc::new(Semaphore::new(concurrency));
      let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

      for base_token in &base_tokens {
         if base_token.address == token.address {
            continue;
         }

         let base_currency = if base_token.is_weth() || base_token.is_wbnb() {
            Currency::from(NativeCurrency::from(chain))
         } else {
            Currency::from(base_token.clone())
         };

         let quote_currency = Currency::from(token.clone());

         let manager = self.clone();
         let semaphore = semaphore.clone();
         let dex_kinds = dex_kinds.clone();
         let client = client.clone();
         let task = RT.spawn(async move {
            let _permit = semaphore.acquire().await?;

            for dex in &dex_kinds {
               if !dex.is_v4() {
                  continue;
               }

               if !manager.should_sync_pools(
                  token.chain_id,
                  *dex,
                  quote_currency.address(),
                  base_currency.address(),
               ) {
                  continue;
               }

               let mut pools_exists = [false; FEE_TIERS.len()];
               for (i, fee) in FEE_TIERS.iter().enumerate() {
                  let pool = manager.get_pool(chain, *dex, *fee, &quote_currency, &base_currency);
                  if pool.is_some() {
                     pools_exists[i] = true;
                  }
               }

               if pools_exists.iter().all(|b| *b == true) {
                  continue;
               }

               let mut pools = Vec::new();
               let mut pool_ids = Vec::new();
               for fee in FEE_TIERS.iter() {
                  let pool = UniswapV4Pool::new(
                     chain,
                     FeeAmount::CUSTOM(*fee),
                     *dex,
                     quote_currency.clone(),
                     base_currency.clone(),
                     State::none(),
                     Address::ZERO,
                  );
                  pool_ids.push(pool.id());
                  pools.push(pool);
               }

               let valid_pool_ids = client.request(chain, |client| {
                  let pool_ids_clone = pool_ids.clone();
                  async move {
                     batch::validate_v4_pools(client, chain, pool_ids_clone).await
                  }
               }).await?;

               for valid_pool in &valid_pool_ids {
                  for pool in &pools {
                     if pool.id() == *valid_pool {
                        trace!(
                        target: "zeus_eth::amm::pool_manager", "Got {} pool {} for {}/{} - Fee: {}",
                        dex.as_str(),
                        pool.id(),
                        pool.currency0().symbol(),
                        pool.currency1().symbol(),
                        pool.fee.fee_percent()
                     );

                        manager.add_pool(pool.clone());
                     }
                  }
               }

               manager.add_token_last_sync_time(
                  token.chain_id,
                  *dex,
                  quote_currency.address(),
                  base_currency.address(),
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

/// Key: (chain_id, dex, tokenA, tokenB) -> Value: Time since last sync
type PoolLastSync = HashMap<(u64, DexKind, Address, Address), Instant>;

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
         pool_last_sync: HashMap::new(),
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
      let time1 = self.pool_last_sync.get(&(chain, dex, token_a, token_b)).cloned();
      let time2 = self.pool_last_sync.get(&(chain, dex, token_b, token_a)).cloned();
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

/// Update the state of all the pools for the given chain
///
/// Supports V2, V3 & V4 pools
///
/// Returns the pools with updated state
async fn batch_update_state(
   ctx: ZeusCtx,
   chain_id: u64,
   concurrency: usize,
   batch_size: usize,
   mut pools: Vec<AnyUniswapPool>,
) -> Result<Vec<AnyUniswapPool>, anyhow::Error> {
   let v2_addresses: Vec<Address> = pools
      .iter()
      .filter(|p| p.dex_kind().is_v2() && p.chain_id() == chain_id)
      .map(|p| p.address())
      .collect();

   info!(target: "zeus_eth::amm::uniswap::state", "Batch request for {} V2 pools ChainId {}", v2_addresses.len(), chain_id);
   let v2_reserves = Arc::new(Mutex::new(Vec::new()));
   let mut v2_tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let semaphore = Arc::new(Semaphore::new(concurrency));
   let client = ctx.get_zeus_client();

   for chunk in v2_addresses.chunks(batch_size) {
      let client = client.clone();
      let chunk_clone = chunk.to_vec();
      let semaphore = semaphore.clone();
      let v2_reserves = v2_reserves.clone();
      let client = client.clone();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire_owned().await?;
         let res = client
            .request(chain_id, |client| {
               let chunk_clone = chunk_clone.clone();
               async move { batch::get_v2_reserves(client.clone(), chain_id, chunk_clone).await }
            })
            .await;

         match res {
            Ok(data) => {
               v2_reserves.lock().await.extend(data);
            }
            Err(e) => {
               error!(target: "zeus_eth::amm::uniswap::state","Error fetching v2 pool reserves: (ChainId {}): {:?}", chain_id, e);
            }
         }
         Ok(())
      });
      v2_tasks.push(task);
   }

   let v3_data = Arc::new(Mutex::new(Vec::new()));
   let mut v3_tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let mut v3_pool_info = Vec::new();

   for pool in &pools {
      if pool.dex_kind().is_v3() && pool.chain_id() == chain_id {
         v3_pool_info.push(ZeusStateView::V3Pool {
            addr: pool.address(),
            tokenA: pool.currency0().address(),
            tokenB: pool.currency1().address(),
            fee: pool.fee().fee_u24(),
         });
      }
   }

   tracing::info!(target: "zeus_eth::amm::uniswap::state", "Batch request for {} V3 pools ChainId {}", v3_pool_info.len(), chain_id);
   for pool in v3_pool_info.chunks(batch_size) {
      let client = client.clone();
      let semaphore = semaphore.clone();
      let v3_data = v3_data.clone();
      let pool_chunk = pool.to_vec();
      let client = client.clone();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire_owned().await.unwrap();
         let res = client
            .request(chain_id, |client| {
               let pool_chunk = pool_chunk.clone();
               async move { batch::get_v3_state(client.clone(), chain_id, pool_chunk).await }
            })
            .await;

         match res {
            Ok(data) => {
               v3_data.lock().await.extend(data);
            }
            Err(e) => {
               tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v3 pool data (ChainId {}): {:?}", chain_id, e);
            }
         }
         Ok(())
      });
      v3_tasks.push(task);
   }

   let v4_data = Arc::new(Mutex::new(Vec::new()));
   let mut v4_tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let mut v4_pool_info = Vec::new();

   for pool in &pools {
      if pool.dex_kind().is_v4() && pool.chain_id() == chain_id {
         v4_pool_info.push(ZeusStateView::V4Pool {
            pool: pool.id(),
            tickSpacing: pool.fee().tick_spacing(),
         });
      }
   }

   tracing::info!(target: "zeus_eth::amm::uniswap::state", "Batch request for {} V4 pools ChainId {}", v4_pool_info.len(), chain_id);
   for pool in v4_pool_info.chunks(batch_size) {
      let client = client.clone();
      let semaphore = semaphore.clone();
      let v4_data = v4_data.clone();
      let pool_chunk = pool.to_vec();
      let client = client.clone();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire_owned().await.unwrap();
         let res = client
            .request(chain_id, |client| {
               let pool_chunk = pool_chunk.clone();
               async move { batch::get_v4_pool_state(client.clone(), chain_id, pool_chunk).await }
            })
            .await;

         match res {
            Ok(data) => {
               v4_data.lock().await.extend(data);
            }
            Err(e) => {
               tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v4 pool data (ChainId {}): {:?}", chain_id, e);
            }
         }

         Ok(())
      });
      v4_tasks.push(task);
   }

   for task in v2_tasks {
      if let Err(e) = task.await? {
         tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v2 pool reserves: {:?}", e);
      }
   }

   for task in v3_tasks {
      if let Err(e) = task.await? {
         tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v3 pool data: {:?}", e);
      }
   }

   for task in v4_tasks {
      if let Err(e) = task.await? {
         tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v4 pool data: {:?}", e);
      }
   }

   let v2_reserves = Arc::try_unwrap(v2_reserves).unwrap().into_inner();
   let v3_pool_data = Arc::try_unwrap(v3_data).unwrap().into_inner();
   let v4_pool_data = Arc::try_unwrap(v4_data).unwrap().into_inner();

   // update the state of the pools
   for pool in pools.iter_mut() {
      if pool.dex_kind().is_v2() && pool.chain_id() == chain_id {
         for data in &v2_reserves {
            if data.pool == pool.address() {
               pool.set_state(State::v2(data.clone().into()));
            }
         }
      }

      if pool.dex_kind().is_v3() && pool.chain_id() == chain_id {
         for data in &v3_pool_data {
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

      if pool.dex_kind().is_v4() && pool.chain_id() == chain_id {
         for data in &v4_pool_data {
            if data.pool == pool.id() {
               let state = V3PoolState::for_v4(pool, data.clone())?;
               pool.set_state(State::v3(state));
               match pool.compute_virtual_reserves() {
                  Ok(_) => {}
                  Err(e) => {
                     tracing::error!(target: "zeus_eth::amm::uniswap::state","Error computing virtual reserves for pool {} / {} ID: {} {:?}",
                      pool.currency0().symbol(),
                       pool.currency1().symbol(),
                        pool.id(), e);
                  }
               }
            }
         }
      }
   }

   Ok(pools)
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
