use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::{sync::Semaphore, task::JoinHandle};
use tracing::info;
use zeus_eth::abi::zeus::ZeusStateViewV3::PoolsState;
use zeus_eth::alloy_primitives::FixedBytes;
use zeus_eth::amm::uniswap::UniswapV4Pool;

use crate::core::{WalletStateKey, ZeusCtx, context::pool_data_dir, serde_hashmap};
use crate::embedded::POOL_DATA;
use crate::utils::{RT, TimeStamp, write_private_atomic};
use zeus_eth::{
   abi::zeus::ZeusStateViewV3::{V3Pool, V4Pool},
   alloy_primitives::{Address, B256},
   amm::uniswap::{
      AnyUniswapPool, DexKind, FEE_TIERS, FeeAmount, UniswapPool, UniswapV2Pool, UniswapV3Pool,
      state::{State, V3PoolState},
      sync::Checkpoint,
   },
   currency::{Currency, ERC20Token},
   types::*,
   utils::{
      address_book::{uniswap_v2_factory, uniswap_v3_factory, uniswap_v4_stateview},
      batch,
   },
};

use anyhow::anyhow;

#[cfg(feature = "dev")]
use tracing::debug;

/// Bound ciphertext to this logical slot (AAD).
const POOL_DATA_AAD: &[u8] = b"zeus-pool-data-v1";

// Timeout for pool discovery in seconds (10 minutes)
const POOL_DISCOVERY_TIMEOUT: u64 = 600;

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

   /// Serialize the [PoolManager] to a JSON string
   pub fn to_string(&self) -> Result<String, serde_json::Error> {
      self.read(|manager| serde_json::to_string(manager))
   }

   pub fn load_from_file(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let dir = pool_data_dir()?;
      let sealed = std::fs::read(dir)?;
      let manager: PoolManager = key.open_json(&sealed, POOL_DATA_AAD)?;
      self.write(|m| *m = manager);
      Ok(())
   }

   pub fn save_to_file(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let (manager, key) = self.read(|manager| (manager.clone(), key.clone()));
      let sealed = key.seal_json(&manager, POOL_DATA_AAD)?;
      let dir = pool_data_dir()?;
      write_private_atomic(&dir, &sealed)?;
      Ok(())
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(pool_data_dir()?.exists())
   }

   pub fn reset_default_settings(&self) {
      self.write(|manager| {
         manager.concurrency = default_concurrency();
         manager.batch_size_for_updating_pool_state = default_batch_size_for_updating_pool_state();
         manager.batch_size_for_discovering_pools = default_batch_size_for_discovering_pools();
         manager.discover_v4_pools = default_discover_v4_pools();
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

   pub fn set_concurrency(&self, concurrency: usize) {
      self.write(|manager| manager.concurrency = concurrency);
   }

   pub fn set_batch_size_for_updating_pools_state(&self, batch_size: usize) {
      self.write(|manager| manager.batch_size_for_updating_pool_state = batch_size);
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

   pub fn get_v2_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_v2_pools_for_chain(chain_id))
   }

   pub fn get_v3_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_v3_pools_for_chain(chain_id))
   }

   pub fn get_v4_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_v4_pools_for_chain(chain_id))
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
         manager.pools.shrink_to_fit();
      });
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

   /// Update the state for the given pools without updating the PriceManager
   pub(crate) async fn _update_state_for_pools(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      pools: Vec<AnyUniswapPool>,
   ) -> Result<Vec<AnyUniswapPool>, anyhow::Error> {
      let concurrency = self.concurrency();
      let batch_size = self.batch_size_for_updating_pools_state();

      let updated_pools =
         batch_update_state(ctx.clone(), chain, concurrency, batch_size, pools).await?;

      self.add_pools(updated_pools.clone());

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

   pub fn add_last_discover_time(&self, chain: u64, token_a: Address, token_b: Address) {
      self.write(|manager| manager.add_last_discover(chain, token_a, token_b))
   }

   fn get_last_discover(&self, chain: u64, token_a: Address, token_b: Address) -> Option<u64> {
      self.read(|manager| manager.get_last_discover_time(chain, token_a, token_b))
   }

   fn should_discover_pools(&self, chain: u64, token_a: Address, token_b: Address) -> bool {
      let last_discover = self.get_last_discover(chain, token_a, token_b);
      if last_discover.is_none() {
         return true;
      }

      let last_discover = last_discover.unwrap();
      let now = TimeStamp::now_as_secs().unwrap_or_default();
      let passed = now.timestamp().saturating_sub(last_discover);

      passed > POOL_DISCOVERY_TIMEOUT
   }

   /// Discover all the possible V2/V3/V4 pools for the given tokens
   pub async fn discover_pools_for_tokens(
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

      let v2_factory = uniswap_v2_factory(chain)?;
      let v3_factory = uniswap_v3_factory(chain)?;
      let state_view = uniswap_v4_stateview(chain)?;
      let base_tokens = ERC20Token::base_tokens(chain);

      #[cfg(feature = "dev")]
      {
         let symbols = tokens.iter().map(|t| t.symbol.clone()).collect::<Vec<_>>();
         let addresses = tokens.iter().map(|t| t.address.to_string()).collect::<Vec<_>>();
         debug!(
            "Discovering pools for {} {} Chain {}",
            symbols.join(", "),
            addresses.join(", "),
            chain
         );
      }

      for token in tokens {
         let semaphore = semaphore.clone();
         let ctx = ctx.clone();
         let manager = self.clone();
         let base_tokens = base_tokens.clone();

         let task = RT.spawn(async move {
            let _permit = semaphore.acquire().await?;

            let mut v4_pools_map = HashMap::new();
            let mut v4_pool_ids = Vec::new();
            let mut bases_to_discover = Vec::new();

            for base_token in &base_tokens {
               if base_token.address == token.address {
                  continue;
               }

               let should_discover =
                  manager.should_discover_pools(chain, base_token.address, token.address);

               #[cfg(feature = "dev")]
               debug!(
                  "Should discover {} for {} {}-{}",
                  should_discover, chain, base_token.symbol, token.symbol
               );

               if !should_discover {
                  continue;
               }

               bases_to_discover.push(base_token.clone());

               for fee in FEE_TIERS.iter() {
                  let fee_amount = FeeAmount::new(*fee);
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

            if bases_to_discover.is_empty() {
               return Ok(token);
            }

            let mut tokens_map = HashMap::new();

            for base_token in &bases_to_discover {
               tokens_map.insert(base_token.address, base_token.clone());
            }

            tokens_map.insert(token.address, token.clone());

            let base_tokens_addr = bases_to_discover.iter().map(|t| t.address).collect::<Vec<_>>();
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

               if manager.get_v2_pool_from_address(chain, v2_pool.addr).is_some() {
                  continue;
               }

               let Some(token_a) = tokens_map.get(&v2_pool.tokenA) else {
                  tracing::error!("V2Pool Token not found: {}", v2_pool.tokenA);
                  continue;
               };

               let Some(token_b) = tokens_map.get(&v2_pool.tokenB) else {
                  tracing::error!("V2Pool Token not found: {}", v2_pool.tokenB);
                  continue;
               };

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

               if manager.get_v3_pool_from_address(chain, v3_pool.addr).is_some() {
                  continue;
               }

               let Some(token_a) = tokens_map.get(&v3_pool.tokenA) else {
                  tracing::error!("V3Pool Token not found: {}", v3_pool.tokenA);
                  continue;
               };

               let Some(token_b) = tokens_map.get(&v3_pool.tokenB) else {
                  tracing::error!("V3Pool Token not found: {}", v3_pool.tokenB);
                  continue;
               };

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

               if manager.get_v4_pool_from_id(chain, *v4_pool).is_some() {
                  continue;
               }

               let Some(pool_full) = v4_pools_map.get(v4_pool) else {
                  tracing::error!("V4Pool not found: {}", v4_pool);
                  continue;
               };

               manager.add_pool(pool_full.clone());
            }

            for base_token in &bases_to_discover {
               manager.add_last_discover_time(chain, base_token.address, token.address);
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
               tracing::error!("Error discovering pools: {:?}", e);
               continue;
            }
         };

         match res {
            Ok(token) => {
               info!(
                  "Discovered Pools for {} in {} ms Chain {}",
                  token.symbol,
                  time.elapsed().as_millis(),
                  chain
               );
            }
            Err(e) => {
               tracing::error!("Error discovering pools: {:?}", e);
            }
         }
      }

      self.write(|manager| {
         manager.pools.shrink_to_fit();
      });

      Ok(())
   }
}

/// Key: (chain_id, tokenA, tokenB) -> Value: UNIX timestamp in secs
type LastDiscovery = HashMap<(u64, Address, Address), u64>;

/// Key: (chain_id, dex_kind) -> Value: UNIX timestamp in secs
type V4PoolLastDiscovery = HashMap<(u64, DexKind), u64>;

/// Key: (chain_id, dex_kind, fee, tokenA, tokenB) -> Value: Pool
type Pools = HashMap<(u64, DexKind, u32, Currency, Currency), AnyUniswapPool>;

/// Key: (chain_id, dex) -> Value: Checkpoint
type CheckpointMap = HashMap<(u64, DexKind), Checkpoint>;

/// Ignore chains for V4 pool historic sync
type IgnoreChains = HashSet<u64>;

fn default_batch_size_for_updating_pool_state() -> usize {
   20
}

fn default_batch_size_for_discovering_pools() -> usize {
   30
}

fn default_concurrency() -> usize {
   1
}

fn default_discover_v4_pools() -> bool {
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

   /// Last time we requested to discover new pools for a token pair
   #[serde(default, with = "serde_hashmap")]
   pub last_discover: LastDiscovery,

   /// V4 Pools are discovered by using the `eth_get_logs` method so they get a different map
   #[serde(default, with = "serde_hashmap")]
   pub v4_pool_last_discover: V4PoolLastDiscovery,

   #[serde(with = "serde_hashmap")]
   pub checkpoints: CheckpointMap,

   /// Concurrent requests when syncing and discovering pools
   ///
   /// Set to 1 for no concurrency
   #[serde(default = "default_concurrency")]
   pub concurrency: usize,

   /// Batch size when syncing the pools state used in [batch_update_state]
   #[serde(default = "default_batch_size_for_updating_pool_state")]
   pub batch_size_for_updating_pool_state: usize,

   /// Batch size when discovering pools from logs
   #[serde(default = "default_batch_size_for_discovering_pools")]
   pub batch_size_for_discovering_pools: usize,

   #[serde(default = "default_discover_v4_pools")]
   pub discover_v4_pools: bool,

   #[serde(default = "default_ignore_chains")]
   pub ignore_chains: IgnoreChains,
}

impl Default for PoolManager {
   fn default() -> Self {
      let manager: PoolManager = match serde_json::from_str(POOL_DATA) {
         Ok(manager) => manager,
         Err(e) => {
            tracing::error!("Failed to parse pool data: {e}");
            PoolManager {
               pools: HashMap::new(),
               last_discover: HashMap::new(),
               v4_pool_last_discover: HashMap::new(),
               checkpoints: HashMap::new(),
               concurrency: default_concurrency(),
               batch_size_for_updating_pool_state: default_batch_size_for_updating_pool_state(),
               batch_size_for_discovering_pools: default_batch_size_for_discovering_pools(),
               discover_v4_pools: default_discover_v4_pools(),
               ignore_chains: default_ignore_chains(),
            }
         }
      };

      Self {
         pools: manager.pools,
         last_discover: HashMap::new(),
         v4_pool_last_discover: HashMap::new(),
         checkpoints: manager.checkpoints,
         concurrency: default_concurrency(),
         batch_size_for_updating_pool_state: default_batch_size_for_updating_pool_state(),
         batch_size_for_discovering_pools: default_batch_size_for_discovering_pools(),
         discover_v4_pools: default_discover_v4_pools(),
         ignore_chains: default_ignore_chains(),
      }
   }
}

impl PoolManager {
   fn add_last_discover(&mut self, chain: u64, token_a: Address, token_b: Address) {
      let key = (chain, token_a, token_b);
      let now = TimeStamp::now_as_secs().unwrap_or_default();
      self.last_discover.insert(key, now.timestamp());
   }

   fn add_checkpoint(&mut self, chain: u64, dex: DexKind, checkpoint: Checkpoint) {
      let key = (chain, dex);
      self.checkpoints.insert(key, checkpoint);
   }

   fn get_last_discover_time(&self, chain: u64, token_a: Address, token_b: Address) -> Option<u64> {
      let time1 = self.last_discover.get(&(chain, token_a, token_b)).cloned();
      let time2 = self.last_discover.get(&(chain, token_b, token_a)).cloned();
      time1.or(time2)
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

   pub fn get_v2_pool_from_address(
      &self,
      chain_id: u64,
      address: Address,
   ) -> Option<&AnyUniswapPool> {
      self
         .pools
         .values()
         .find(|p| p.address() == address && p.chain_id() == chain_id && p.dex_kind().is_v2())
   }

   pub fn get_v3_pool_from_address(
      &self,
      chain_id: u64,
      address: Address,
   ) -> Option<&AnyUniswapPool> {
      self
         .pools
         .values()
         .find(|p| p.address() == address && p.chain_id() == chain_id && p.dex_kind().is_v3())
   }

   pub fn get_v4_pool_from_id(&self, chain_id: u64, pool_id: B256) -> Option<&AnyUniswapPool> {
      self.pools.values().find(|p| p.id() == pool_id && p.chain_id() == chain_id)
   }

   pub fn get_pool_from_address(&self, chain_id: u64, address: Address) -> Option<&AnyUniswapPool> {
      self.pools.values().find(|p| p.address() == address && p.chain_id() == chain_id)
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

   #[cfg(feature = "dev")]
   tracing::info!(
      "Updating pool state for {} pools - batch size {}",
      pools.len(),
      batch_size
   );

   let mut v2_pools = Vec::new();
   let mut v3_pools = Vec::new();
   let mut v4_pools = Vec::new();

   for pool in pools {
      if pool.dex_kind().is_v2() {
         v2_pools.push(pool);
      } else if pool.dex_kind().is_v3() {
         v3_pools.push(pool);
      } else if pool.dex_kind().is_v4() {
         v4_pools.push(pool);
      }
   }

   let all_v2_addresses: Vec<Address> = v2_pools.iter().map(|p| p.address()).collect();

   let mut all_v3_pool_info = Vec::with_capacity(v3_pools.len());
   let mut all_v4_pool_info = Vec::with_capacity(v4_pools.len());

   for pool in &v3_pools {
      all_v3_pool_info.push(V3Pool {
         addr: pool.address(),
         tokenA: pool.currency0().address(),
         tokenB: pool.currency1().address(),
         fee: pool.fee().fee_u24(),
      });
   }

   for pool in &v4_pools {
      let Some(tick_spacing) = FeeAmount::i24_tick_spacing(pool.tick_spacing_i32()) else {
         tracing::warn!(
            "Skipping V4 pool {} with invalid tick spacing 0 on chain {}",
            pool.id(),
            chain
         );
         continue;
      };
      all_v4_pool_info.push(V4Pool {
         pool: pool.id(),
         tickSpacing: tick_spacing,
      });
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Fetching pool state for {} V2 pools {} V3 pools {} V4 pools",
      v2_pools.len(),
      v3_pools.len(),
      v4_pools.len()
   );

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
   let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));

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

   let mut v2_by_addr = HashMap::new();
   let mut v3_by_addr = HashMap::new();
   let mut v4_by_id = HashMap::new();

   for state in results {
      for data in state.v2Reserves {
         v2_by_addr.insert(data.pool, data);
      }
      for data in state.v3PoolsData {
         if data.pool.is_zero() {
            continue;
         }
         v3_by_addr.insert(data.pool, data);
      }
      for data in state.v4PoolsData {
         if data.pool.is_zero() {
            continue;
         }
         v4_by_id.insert(data.pool, data);
      }
   }

   #[cfg(feature = "dev")]
   let (v2_state_len, v3_state_len, v4_state_len) =
      (v2_by_addr.len(), v3_by_addr.len(), v4_by_id.len());

   for pool in v2_pools.iter_mut() {
      if let Some(data) = v2_by_addr.remove(&pool.address()) {
         pool.set_state(State::v2(data.into()));
      }
   }

   for pool in v3_pools.iter_mut() {
      let Some(data) = v3_by_addr.remove(&pool.address()) else {
         continue;
      };
      let token_a_balance = data.tokenABalance;
      let token_b_balance = data.tokenBBalance;
      let state = V3PoolState::new(data, pool.tick_spacing(), None)?;
      pool.set_state(State::v3(state));
      pool.v3_mut(|pool| {
         pool.liquidity_amount0 = token_a_balance;
         pool.liquidity_amount1 = token_b_balance;
      });
   }

   for pool in v4_pools.iter_mut() {
      let Some(data) = v4_by_id.remove(&pool.id()) else {
         continue;
      };
      let state = V3PoolState::for_v4(pool, data)?;
      pool.set_state(State::v3(state));
      #[cfg(feature = "dev")]
      if let Err(e) = pool.compute_virtual_reserves() {
         tracing::error!(
            "Error computing virtual reserves for pool {} / {} ID: {} {:?}",
            pool.currency0().symbol(),
            pool.currency1().symbol(),
            pool.id(),
            e
         );
      }
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Updated pool state for {} V2 Pools {} V3 Pools {} V4 Pools in {} ms. Chain {}",
      v2_state_len,
      v3_state_len,
      v4_state_len,
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
   let batch_size = batch_size.max(1);
   let total_pools = all_v2_addresses.len() + all_v3_pool_info.len() + all_v4_pool_info.len();

   if total_pools <= batch_size {
      return vec![Batch {
         v2_pools: all_v2_addresses,
         v3_pools: all_v3_pool_info,
         v4_pools: all_v4_pool_info,
      }];
   }

   // Process in concurrent batches, chunking each pool type proportionally
   // so each batch includes slices from all 3 types of pools.
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

      let _desered_manager: PoolManager = serde_json::from_str(&json).unwrap();
   }

   #[test]
   fn test_seal_open_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let pool = UniswapV2Pool::weth_uni();
      let handle = PoolManagerHandle::new(PoolManager::default());
      handle.add_pool(pool);

      let sealed = handle.read(|manager| key.seal_json(manager, POOL_DATA_AAD)).unwrap();
      let loaded: PoolManager = key.open_json(&sealed, POOL_DATA_AAD).unwrap();
      assert!(!loaded.pools.is_empty());
      assert!(key.open_json::<PoolManager>(&sealed, b"wrong-aad").is_err());
   }
}
