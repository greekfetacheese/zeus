use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::{sync::Semaphore, task::JoinHandle};
use tracing::info;
use zeus_eth::abi::zeus::ZeusStateViewV3::PoolsState;
use zeus_eth::alloy_primitives::FixedBytes;
use zeus_eth::amm::uniswap::UniswapV4Pool;

use crate::assets;
use crate::context::{
   CurrencyDB, RwLockHandle,
   pool_store::{POOL_DB_FORMAT, PoolDbMeta, PoolStore, V4PoolRecord},
};
use crate::persisted::ensure_data_dir;
use crate::platform::TimeStamp;
use crate::{WalletStateKey, ZeusCtx, context::pool_data_dir, serde_hashmap};
use zeus_eth::{
   abi::zeus::ZeusStateViewV3::{V3Pool, V4Pool},
   alloy_primitives::{Address, B256},
   alloy_provider::Provider,
   amm::uniswap::{
      AnyUniswapPool, DexKind, FeeAmount, UniswapPool, UniswapV2Pool, UniswapV3Pool,
      V4_FEE_CANDIDATES,
      state::{State, V3PoolState},
      sync::{Checkpoint, v4_pools_from_logs},
      v4_fee_spacing_candidates,
   },
   currency::{Currency, ERC20Token, NativeCurrency},
   types::*,
   utils::{
      GET_LOGS_MAX_RETRIES,
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

/// Largest span sent to a single `eth_getLogs`. 5k keeps every public RPC happy.
pub const V4_SYNC_BLOCK_RANGE: u64 = 5_000;

/// Blocks per checkpointed step: 1_000_000 blocks => 10 commits.
pub const V4_SYNC_STEP_BLOCKS: u64 = 100_000;

/// The knobs of one [`PoolManagerHandle::sync_v4_pools`] run.
///
/// A `0` means "use the default": [`V4_SYNC_STEP_BLOCKS`], [`V4_SYNC_BLOCK_RANGE`] and the client
/// manager's RPC concurrency — the sync is a log fan-out, so that setting governs it rather than the
/// pool-discovery knob.
#[derive(Debug, Clone, Copy)]
pub struct V4SyncConfig {
   pub step_blocks: u64,
   pub block_range: u64,
   pub concurrency: usize,
}

impl Default for V4SyncConfig {
   fn default() -> Self {
      Self {
         step_blocks: V4_SYNC_STEP_BLOCKS,
         block_range: V4_SYNC_BLOCK_RANGE,
         concurrency: 0,
      }
   }
}

/// What one [`PoolManagerHandle::sync_v4_pools`] run covered.
#[derive(Debug, Clone, Copy, Default)]
pub struct V4SyncReport {
   pub from_block: u64,
   pub to_block: u64,
   pub steps: usize,
   /// `Initialize` logs decoded, before de-duplication.
   pub fetched: usize,
   /// New pools whose currencies resolve locally, so they are held in memory.
   pub hot: usize,
   /// New pools that stay on disk until a token they hold is imported.
   pub cold: usize,
}

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
pub struct PoolManagerHandle(RwLockHandle<PoolManager>);

impl Default for PoolManagerHandle {
   fn default() -> Self {
      Self(RwLockHandle::new(PoolManager::default()))
   }
}

impl Deref for PoolManagerHandle {
   type Target = RwLockHandle<PoolManager>;

   fn deref(&self) -> &Self::Target {
      &self.0
   }
}

impl PoolManagerHandle {
   pub fn new(pool_manager: PoolManager) -> Self {
      Self(RwLockHandle::new(pool_manager))
   }

   /// Serialize the [PoolManager] to a JSON string
   pub fn to_string(&self) -> Result<String, serde_json::Error> {
      self.read(serde_json::to_string)
   }

   /// Load `data/pool_data.data` into this handle.
   ///
   /// The file may still be the pre-redb sealed blob, which is imported once
   /// ([`migrate_legacy_blob`]); otherwise it is the redb catalog, and only the V4 pools whose
   /// currencies resolve locally are materialised.
   pub fn load_from_file(
      &self,
      key: &WalletStateKey,
      tokens: &CurrencyDB,
   ) -> Result<(), anyhow::Error> {
      let path = pool_data_dir()?;

      if let Some(mut manager) = migrate_legacy_blob(&path, key, tokens)? {
         manager.store = Some(PoolStore::open_at(&path, key)?);
         self.write(|current| *current = manager);
         return Ok(());
      }

      self.load_from_store(PoolStore::open_at(&path, key)?, tokens)
   }

   /// Replace the live manager with the catalog in `store`.
   ///
   /// A catalog that does not read leaves the current manager untouched and hands the error back:
   /// half a catalog is worse than the pools already in memory.
   pub(crate) fn load_from_store(
      &self,
      store: PoolStore,
      tokens: &CurrencyDB,
   ) -> Result<(), anyhow::Error> {
      let meta = store.load_meta()?;
      let checkpoints = store.checkpoints()?;
      let discovered = store.discovered()?;
      let discovered_v4 = store.discovered_v4()?;
      let v2_v3 = store.load_v2_v3_pools()?;
      let v4 = store.load_v4_records(None)?;

      let mut manager = PoolManager::empty();
      if let Some(meta) = meta {
         manager.concurrency = meta.concurrency;
         manager.batch_size_for_updating_pool_state = meta.batch_size_for_updating_pool_state;
         manager.batch_size_for_discovering_pools = meta.batch_size_for_discovering_pools;
         manager.discover_v4_pools = meta.discover_v4_pools;
         manager.ignore_chains = meta.ignore_chains;
      }

      for (chain, dex, block) in checkpoints {
         manager.checkpoints.insert((chain, dex), Checkpoint::new(chain, block, dex));
      }

      manager.last_discover.extend(discovered);
      manager.v4_pool_last_discover.extend(discovered_v4);

      for pool in v2_v3 {
         manager.insert_loaded_pool(pool);
      }

      for (chain, record) in v4 {
         let Some(pool) = v4_pool_from_record(chain, &record, tokens) else {
            manager.index_cold_v4(chain, &record);
            continue;
         };
         manager.insert_loaded_pool(AnyUniswapPool::V4(pool));
      }

      manager.store = Some(store);
      self.write(|current| *current = manager);
      Ok(())
   }

   /// Persist the manager through its catalog, opening one on the first save.
   ///
   /// Without a catalog yet, the whole manager is written: that keeps the old behaviour of the file
   /// appearing on the first save, and seeds a fresh db from the embedded snapshot.
   pub fn save_to_file(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      if let Some(store) = self.read(|manager| manager.store.clone()) {
         return self.save_to_store(&store);
      }

      ensure_data_dir()?;
      let store = PoolStore::open(key)?;
      let catalog = self.read(|manager| manager.catalog());

      store.write_catalog(
         &catalog.meta,
         &catalog.pools,
         &catalog.v4,
         &catalog.checkpoints,
         &catalog.discovered,
         &catalog.discovered_v4,
      )?;

      self.write(|manager| {
         manager.store = Some(store);
         manager.clear_dirty();
      });

      Ok(())
   }

   /// Write the dirty rows, then the metadata, to `store`.
   pub(crate) fn save_to_store(&self, store: &PoolStore) -> Result<(), anyhow::Error> {
      let (meta, pools, checkpoints, discovery, discovery_v4) = self.read(|manager| {
         let pools = manager
            .dirty_pools
            .iter()
            .filter_map(|key| manager.pools.get(key).map(|pool| (key.clone(), pool.clone())))
            .collect::<Vec<_>>();
         let checkpoints = manager
            .dirty_checkpoints
            .iter()
            .filter_map(|key| {
               manager.checkpoints.get(key).map(|checkpoint| (*key, checkpoint.block))
            })
            .collect::<Vec<_>>();
         let discovery = manager
            .dirty_discovery
            .iter()
            .filter_map(|key| manager.last_discover.get(key).map(|ts| (*key, *ts)))
            .collect::<Vec<_>>();
         let discovery_v4 = manager
            .dirty_discovery_v4
            .iter()
            .filter_map(|key| manager.v4_pool_last_discover.get(key).map(|ts| (*key, *ts)))
            .collect::<Vec<_>>();

         (
            manager.meta(),
            pools,
            checkpoints,
            discovery,
            discovery_v4,
         )
      });

      store.save_meta(&meta)?;

      for (key, pool) in &pools {
         match pool {
            AnyUniswapPool::V4(pool) => {
               store.put_v4_record(key.0, &V4PoolRecord::from_pool(pool))?
            }
            pool => store.put_pool(pool)?,
         }
      }

      for ((chain, dex), block) in &checkpoints {
         store.put_checkpoint(*chain, *dex, *block)?;
      }

      for ((chain, a, b), ts) in &discovery {
         store.put_discovered(*chain, *a, *b, *ts)?;
      }

      for ((chain, dex), ts) in &discovery_v4 {
         store.put_discovered_v4(*chain, *dex, *ts)?;
      }

      // Only the rows just written lose their mark: a pool added while this flush was running
      // stays dirty for the next one.
      self.write(|manager| {
         for (key, _) in &pools {
            manager.dirty_pools.remove(key);
         }
         for (key, _) in &checkpoints {
            manager.dirty_checkpoints.remove(key);
         }
         for (key, _) in &discovery {
            manager.dirty_discovery.remove(key);
         }
         for (key, _) in &discovery_v4 {
            manager.dirty_discovery_v4.remove(key);
         }
      });

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

   /// Materialise the catalog's V4 pools that hold any of `tokens`.
   ///
   /// This is what lets a token added after a sync find a pool whose other side is not a base token:
   /// the on-chain discovery grid only probes base pairs, the catalog knows every key we have synced.
   /// Idempotent, and a pool whose other side is still unknown stays on disk.
   pub fn promote_v4_pools_for_tokens(
      &self,
      chain: u64,
      tokens: &[ERC20Token],
      db: &CurrencyDB,
   ) -> Result<usize, anyhow::Error> {
      let Some(store) = self.read(|manager| manager.store.clone()) else {
         return Ok(0);
      };

      let candidates = self.read(|manager| {
         let mut ids = HashSet::new();
         for token in tokens {
            if let Some(bucket) = manager.cold_v4.get(&(chain, token.address)) {
               ids.extend(bucket.iter().copied());
            }
         }
         ids
      });

      if candidates.is_empty() {
         return Ok(0);
      }

      let mut promoted = Vec::new();
      for id in candidates {
         if self.get_v4_pool_from_id(chain, id).is_some() {
            continue;
         }

         let Some(record) = store.v4_record(chain, id)? else {
            continue;
         };

         if let Some(pool) = v4_pool_from_record(chain, &record, db) {
            promoted.push((record, pool));
         }
      }

      let promoted = self.write(|manager| {
         let mut count = 0;
         for (record, pool) in promoted {
            manager.insert_loaded_pool(AnyUniswapPool::V4(pool));
            manager.unindex_cold_v4(chain, &record);
            count += 1;
         }
         count
      });

      if promoted > 0 {
         tracing::info!(
            "Promoted {} V4 pools from the catalog for chain {}",
            promoted,
            chain
         );
      }

      Ok(promoted)
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

      // The catalog knows keys the on-chain grid never probes (token X / token Y pairs), so a token
      // added after a sync still finds its pool here.
      let db = ctx.read(|c| c.currency_db.clone());
      if let Err(e) = self.promote_v4_pools_for_tokens(chain, &tokens, &db) {
         tracing::warn!(
            "Failed to read the V4 catalog for chain {}: {:?}",
            chain,
            e
         );
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

         let task = ctx.rt().spawn(async move {
            let _permit = semaphore.acquire().await?;

            let mut v4_pools_map = HashMap::new();
            let mut v4_pool_ids = Vec::new();
            let mut bases_to_discover = Vec::new();
            // The V4 key grid is pair-independent, so build it once per token.
            let v4_candidates = v4_fee_spacing_candidates(&V4_FEE_CANDIDATES);

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

               for (fee_amount, tick_spacing) in &v4_candidates {
                  let pool = UniswapV4Pool::new_with_spacing(
                     chain,
                     *fee_amount,
                     *tick_spacing,
                     DexKind::UniswapV4,
                     Currency::from(base_token.clone()),
                     Currency::from(token.clone()),
                     State::none(),
                     Address::ZERO,
                  );

                  // The grid can repeat an id; keep `v4_pool_ids` aligned and duplicate-free.
                  let id = pool.id();
                  if v4_pools_map.insert(id, pool).is_none() {
                     v4_pool_ids.push(id);
                  }
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
            let zeus_client = ctx.get_client_manager();

            let pools = zeus_client
               .request_with(
                  chain,
                  (v4_pool_ids.clone(), base_tokens_addr.clone()),
                  |client, (v4_pool_ids, base_tokens_addr)| async move {
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
                  },
               )
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
                  #[cfg(feature = "dev")]
                  tracing::error!("V2Pool Token not found: {}", v2_pool.tokenA);
                  continue;
               };

               let Some(token_b) = tokens_map.get(&v2_pool.tokenB) else {
                  #[cfg(feature = "dev")]
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
                  #[cfg(feature = "dev")]
                  tracing::error!("V3Pool Token not found: {}", v3_pool.tokenA);
                  continue;
               };

               let Some(token_b) = tokens_map.get(&v3_pool.tokenB) else {
                  #[cfg(feature = "dev")]
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
                  #[cfg(feature = "dev")]
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

      // Persist the discovered pools now, not at shutdown: a crash must not lose them.
      ctx.save_pool_manager();

      Ok(())
   }

   /// Discover V4 pools from `Initialize` logs, committing one checkpointed step at a time.
   ///
   /// The checkpoint is the resume point: a step's pools and its checkpoint land in one transaction,
   /// so a crash or a failed request resumes at the first range that was never committed. Every
   /// completed step is durable even when this returns an error.
   pub async fn sync_v4_pools(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      config: V4SyncConfig,
   ) -> Result<V4SyncReport, anyhow::Error> {
      let key = ctx.read_vault(|vault| vault.wallet_state_key())?;
      let client = ctx.get_client(chain).await?;
      let tokens = ctx.read(|c| c.currency_db.clone());

      let store = match self.read(|manager| manager.store.clone()) {
         Some(store) => store,
         None => {
            // No catalog open yet: create it from the manager in memory so this sync has
            // somewhere to commit its steps.
            self.save_to_file(&key)?;
            self
               .read(|manager| manager.store.clone())
               .ok_or_else(|| anyhow!("the pool catalog was not attached"))?
         }
      };

      let tip = client.get_block_number().await?;
      let from = match self.read(|manager| {
         manager
            .checkpoints
            .get(&(chain, DexKind::UniswapV4))
            .map(|checkpoint| checkpoint.block)
      }) {
         Some(block) => block + 1,
         None => DexKind::UniswapV4.creation_block(chain)?,
      };

      let mut report = V4SyncReport {
         from_block: from,
         to_block: tip,
         ..Default::default()
      };

      if from > tip {
         return Ok(report);
      }

      let step = match config.step_blocks {
         0 => V4_SYNC_STEP_BLOCKS,
         step => step,
      };
      let range = match config.block_range {
         0 => V4_SYNC_BLOCK_RANGE,
         range => range,
      };
      // This is a log fan-out, so the RPC concurrency setting governs it, not the pool-discovery
      // knob. `max(1)` because a zero would wedge `get_logs_for`'s semaphore.
      let concurrency = match config.concurrency {
         0 => ctx.get_client_manager().concurrency().max(1),
         concurrency => concurrency,
      };

      let mut known = self.read(|manager| {
         manager
            .pools
            .values()
            .filter(|pool| pool.chain_id() == chain && pool.dex_kind().is_v4())
            .map(|pool| pool.id())
            .collect::<HashSet<_>>()
      });

      for (start, end) in sync_steps(from, tip, step) {
         let inits = v4_pools_from_logs(
            client.clone(),
            chain,
            start,
            end,
            concurrency,
            range,
            GET_LOGS_MAX_RETRIES,
         )
         .await?;

         report.fetched += inits.len();

         let mut records = Vec::new();
         let mut hot = Vec::new();
         let mut cold_records = Vec::new();

         for init in inits {
            let id = init.id();

            // A resumed range and a reorg overlap both land here.
            if known.contains(&id) || store.has_v4(chain, id)? {
               continue;
            }
            known.insert(id);

            let record = V4PoolRecord {
               currency0: init.currency0,
               currency1: init.currency1,
               fee: init.fee,
               tick_spacing: init.tick_spacing,
               hooks: init.hooks,
            };

            match v4_pool_from_record(chain, &record, &tokens) {
               Some(pool) => hot.push(pool),
               None => cold_records.push(record.clone()),
            }
            records.push(record);
         }

         let new = records.len();
         let hot_count = hot.len();
         let cold = new - hot_count;

         self.write(|manager| -> Result<(), anyhow::Error> {
            // One transaction: a crash leaves either the whole step or none of it.
            store.commit_v4_step(chain, &records, end)?;

            for pool in hot {
               let pool = AnyUniswapPool::V4(pool);
               manager.pools.insert(pool_key_of(&pool), pool);
            }

            for record in &cold_records {
               manager.index_cold_v4(chain, record);
            }

            manager.checkpoints.insert(
               (chain, DexKind::UniswapV4),
               Checkpoint::new(chain, end, DexKind::UniswapV4),
            );

            Ok(())
         })?;

         report.steps += 1;
         report.hot += hot_count;
         report.cold += cold;

         tracing::info!(
            "V4 sync chain {} {}-{}: {} new, {} hot, {} cold",
            chain,
            start,
            end,
            new,
            hot_count,
            cold
         );
      }

      Ok(report)
   }

   /// Reclaim the catalog's free pages. Blocking; call on shutdown only.
   pub fn compact(&self) -> Result<bool, anyhow::Error> {
      let Some(store) = self.read(|manager| manager.store.clone()) else {
         return Ok(false);
      };

      store.compact()
   }
}

/// Key: (chain_id, tokenA, tokenB) -> Value: UNIX timestamp in secs
type LastDiscovery = HashMap<(u64, Address, Address), u64>;

/// Key: (chain_id, dex_kind) -> Value: UNIX timestamp in secs
type V4PoolLastDiscovery = HashMap<(u64, DexKind), u64>;

/// Key: (chain_id, dex_kind, fee, tokenA, tokenB)
type PoolKey = (u64, DexKind, u32, Currency, Currency);

/// Key: (chain_id, dex_kind, fee, tokenA, tokenB) -> Value: Pool
type Pools = HashMap<PoolKey, AnyUniswapPool>;

/// A whole manager in the shape [`PoolStore::write_catalog`] takes.
struct Catalog {
   meta: PoolDbMeta,
   pools: Vec<AnyUniswapPool>,
   v4: Vec<(u64, V4PoolRecord)>,
   checkpoints: Vec<(u64, DexKind, u64)>,
   discovered: Vec<((u64, Address, Address), u64)>,
   discovered_v4: Vec<((u64, DexKind), u64)>,
}

/// The key `pools` uses for `pool`.
fn pool_key_of(pool: &AnyUniswapPool) -> PoolKey {
   (
      pool.chain_id(),
      pool.dex_kind(),
      pool.fee().fee(),
      pool.currency0().clone(),
      pool.currency1().clone(),
   )
}

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

   /// The redb catalog this manager is persisted through, opened on load or on the first save.
   ///
   /// Never part of the JSON: `embedded/pool_data.json` and `zeus-pools` mirror the fields above.
   #[serde(skip)]
   pub(crate) store: Option<PoolStore>,

   /// Pools touched since the last flush, keyed exactly like `pools`.
   #[serde(skip)]
   dirty_pools: HashSet<PoolKey>,

   /// Checkpoints touched since the last flush, keyed exactly like `checkpoints`.
   #[serde(skip)]
   dirty_checkpoints: HashSet<(u64, DexKind)>,

   /// Discovery timestamps touched since the last flush, keyed like `last_discover`.
   #[serde(skip)]
   dirty_discovery: HashSet<(u64, Address, Address)>,

   /// V4 discovery timestamps touched since the last flush, keyed like `v4_pool_last_discover`.
   #[serde(skip)]
   dirty_discovery_v4: HashSet<(u64, DexKind)>,

   /// Cold V4 records by currency: `(chain, currency) -> pool ids`.
   ///
   /// Only ids live here — the 68-byte record is read from the catalog on a hit — so a 50k-pool
   /// catalog costs ~2 ids per pool (~3 MB) instead of holding every record in memory. Built when the
   /// catalog is loaded and extended by the sync; the distinct-currency count is what the map is keyed
   /// on, and that is small compared to the pool count.
   #[serde(skip)]
   cold_v4: HashMap<(u64, Address), Vec<B256>>,
}

impl Default for PoolManager {
   fn default() -> Self {
      let Some(data) = assets::installed().pool_data() else {
         return Self::empty();
      };
      let manager: PoolManager = match serde_json::from_str(data) {
         Ok(manager) => manager,
         Err(e) => {
            tracing::error!("Failed to parse pool data: {e}");
            return Self::empty();
         }
      };

      Self::from_snapshot(manager)
   }
}

impl PoolManager {
   /// The embedded snapshot's pools, without the discovering run's transient state.
   ///
   /// The snapshot is built by `zeus-pools` from a log sync of its own, so its checkpoints and
   /// discovery timestamps describe *that* run, not this wallet. Inheriting the checkpoint would
   /// make a fresh install claim a sync it never ran, and `sync_v4_pools` would resume from it and
   /// skip every block below it. Only the pools are taken.
   fn from_snapshot(snapshot: PoolManager) -> Self {
      Self {
         pools: snapshot.pools,
         last_discover: HashMap::new(),
         v4_pool_last_discover: HashMap::new(),
         checkpoints: HashMap::new(),
         concurrency: default_concurrency(),
         batch_size_for_updating_pool_state: default_batch_size_for_updating_pool_state(),
         batch_size_for_discovering_pools: default_batch_size_for_discovering_pools(),
         discover_v4_pools: default_discover_v4_pools(),
         ignore_chains: default_ignore_chains(),
         store: None,
         dirty_pools: HashSet::new(),
         dirty_checkpoints: HashSet::new(),
         dirty_discovery: HashSet::new(),
         dirty_discovery_v4: HashSet::new(),
         cold_v4: HashMap::new(),
      }
   }

   /// The manager with no pools and the default settings.
   ///
   /// This is both the value used when no pool snapshot was installed and the fallback for a
   /// snapshot that fails to parse.
   fn empty() -> Self {
      Self {
         pools: HashMap::new(),
         last_discover: HashMap::new(),
         v4_pool_last_discover: HashMap::new(),
         checkpoints: HashMap::new(),
         concurrency: default_concurrency(),
         batch_size_for_updating_pool_state: default_batch_size_for_updating_pool_state(),
         batch_size_for_discovering_pools: default_batch_size_for_discovering_pools(),
         discover_v4_pools: default_discover_v4_pools(),
         ignore_chains: default_ignore_chains(),
         store: None,
         dirty_pools: HashSet::new(),
         dirty_checkpoints: HashSet::new(),
         dirty_discovery: HashSet::new(),
         dirty_discovery_v4: HashSet::new(),
         cold_v4: HashMap::new(),
      }
   }

   /// Insert a pool that came out of the catalog: no dirty mark, it is already on disk.
   fn insert_loaded_pool(&mut self, pool: AnyUniswapPool) {
      self.pools.insert(pool_key_of(&pool), pool);
   }

   /// Index a cold V4 record under both of its currencies, so a token added later can find it
   /// without scanning the catalog.
   fn index_cold_v4(&mut self, chain: u64, record: &V4PoolRecord) {
      let id = record.id();
      for currency in [record.currency0, record.currency1] {
         let ids = self.cold_v4.entry((chain, currency)).or_default();
         if !ids.contains(&id) {
            ids.push(id);
         }
      }
   }

   /// Drop a record from both currency buckets once its pool is hot: the catalog is on disk, so the
   /// index must not keep offering it.
   fn unindex_cold_v4(&mut self, chain: u64, record: &V4PoolRecord) {
      let id = record.id();
      for currency in [record.currency0, record.currency1] {
         if let Some(ids) = self.cold_v4.get_mut(&(chain, currency)) {
            ids.retain(|known| *known != id);
         }
      }
   }

   /// Everything in this manager, in the shape [`PoolStore::write_catalog`] takes.
   fn catalog(&self) -> Catalog {
      let mut pools = Vec::with_capacity(self.pools.len());
      let mut v4 = Vec::new();

      for pool in self.pools.values() {
         match pool {
            AnyUniswapPool::V4(pool) => v4.push((pool.chain_id(), V4PoolRecord::from_pool(pool))),
            pool => pools.push(pool.clone()),
         }
      }

      Catalog {
         meta: self.meta(),
         pools,
         v4,
         checkpoints: self
            .checkpoints
            .iter()
            .map(|((chain, dex), checkpoint)| (*chain, *dex, checkpoint.block))
            .collect(),
         discovered: self.last_discover.iter().map(|(key, ts)| (*key, *ts)).collect(),
         discovered_v4: self.v4_pool_last_discover.iter().map(|(key, ts)| (*key, *ts)).collect(),
      }
   }

   /// The catalog metadata this manager persists.
   fn meta(&self) -> PoolDbMeta {
      PoolDbMeta {
         concurrency: self.concurrency,
         batch_size_for_updating_pool_state: self.batch_size_for_updating_pool_state,
         batch_size_for_discovering_pools: self.batch_size_for_discovering_pools,
         discover_v4_pools: self.discover_v4_pools,
         ignore_chains: self.ignore_chains.clone(),
         format: POOL_DB_FORMAT,
      }
   }

   /// Forget every dirty mark: the whole manager was just written.
   fn clear_dirty(&mut self) {
      self.dirty_pools.clear();
      self.dirty_checkpoints.clear();
      self.dirty_discovery.clear();
      self.dirty_discovery_v4.clear();
   }

   /// Drop the V4 pools no local currency resolves; the catalog keeps them as cold records.
   fn retain_hot_v4(&mut self, tokens: &CurrencyDB) {
      self.pools.retain(|_, pool| match pool {
         AnyUniswapPool::V4(pool) => v4_pool_from_record(
            pool.chain_id(),
            &V4PoolRecord::from_pool(pool),
            tokens,
         )
         .is_some(),
         _ => true,
      });
   }

   fn add_last_discover(&mut self, chain: u64, token_a: Address, token_b: Address) {
      let key = (chain, token_a, token_b);
      let now = TimeStamp::now_as_secs().unwrap_or_default();
      self.last_discover.insert(key, now.timestamp());
      self.dirty_discovery.insert(key);
   }

   fn add_checkpoint(&mut self, chain: u64, dex: DexKind, checkpoint: Checkpoint) {
      let key = (chain, dex);
      self.checkpoints.insert(key, checkpoint);
      self.dirty_checkpoints.insert(key);
   }

   fn get_last_discover_time(&self, chain: u64, token_a: Address, token_b: Address) -> Option<u64> {
      let time1 = self.last_discover.get(&(chain, token_a, token_b)).cloned();
      let time2 = self.last_discover.get(&(chain, token_b, token_a)).cloned();
      time1.or(time2)
   }

   pub fn add_pool(&mut self, pool: impl UniswapPool) {
      let any_pool = AnyUniswapPool::from_pool(pool);
      let key = pool_key_of(&any_pool);

      self.dirty_pools.insert(key.clone());
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
         if (pool.is_currency0(currency_a) && pool.is_currency1(currency_b))
            || (pool.is_currency0(currency_b) && pool.is_currency1(currency_a))
         {
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

/// Inclusive block ranges covering `from..=to` in `step` blocks. Empty when `from > to`.
fn sync_steps(from: u64, to: u64, step: u64) -> Vec<(u64, u64)> {
   if from > to || step == 0 {
      return Vec::new();
   }

   let mut steps = Vec::new();
   let mut start = from;

   while start <= to {
      let end = start.saturating_add(step - 1).min(to);
      steps.push((start, end));

      let Some(next) = end.checked_add(1) else {
         break;
      };
      start = next;
   }

   steps
}

/// Resolve a V4 currency without any RPC: native, a static base token, or a token in the wallet DB.
fn resolve_currency(chain: u64, address: Address, tokens: &CurrencyDB) -> Option<Currency> {
   // `NativeCurrency::from` and `ERC20Token::base_token` both panic on a chain id this build does not
   // know, and a catalog row can have been written by a build that knew one more chain.
   let native = NativeCurrency::from_chain_id(chain).ok()?;

   if address.is_zero() {
      return Some(Currency::from(native));
   }

   if let Some(base) = ERC20Token::base_token(chain, address) {
      return Some(Currency::from(base));
   }

   tokens.get_erc20_token(chain, address).map(Currency::from)
}

/// A V4 pool whose both currencies resolve locally. `None` = the pool stays on disk.
fn v4_pool_from_record(
   chain: u64,
   record: &V4PoolRecord,
   tokens: &CurrencyDB,
) -> Option<UniswapV4Pool> {
   let currency0 = resolve_currency(chain, record.currency0, tokens)?;
   let currency1 = resolve_currency(chain, record.currency1, tokens)?;

   Some(UniswapV4Pool::new_with_spacing(
      chain,
      FeeAmount::new(record.fee),
      record.tick_spacing,
      DexKind::UniswapV4,
      currency0,
      currency1,
      State::none(),
      record.hooks,
   ))
}

/// One-time import of the pre-redb `pool_data.data` (sealed with `POOL_DATA_AAD`).
///
/// Renames the blob aside, writes the catalog, then removes it; a failure leaves the blob
/// recoverable.
fn migrate_legacy_blob(
   path: &Path,
   key: &WalletStateKey,
   tokens: &CurrencyDB,
) -> Result<Option<PoolManager>, anyhow::Error> {
   if !path.exists() {
      return Ok(None);
   }

   let bytes = std::fs::read(path)?;

   // The parse runs before any redb open: opening a foreign file as a database rewrites it.
   let Ok(mut manager) = key.open_json::<PoolManager>(&bytes, POOL_DATA_AAD) else {
      return Ok(None);
   };

   let legacy = path.with_file_name("pool_data.data.legacy");
   std::fs::rename(path, &legacy)?;

   // The catalog's cold rows come from every pool in memory, so it is built before the cold V4
   // pools are dropped from it.
   let catalog = manager.catalog();
   manager.retain_hot_v4(tokens);

   // The rows that just went cold are on disk only, so index them for a later token import.
   for (chain, record) in &catalog.v4 {
      if manager.get_v4_pool_from_id(*chain, record.id()).is_none() {
         manager.index_cold_v4(*chain, record);
      }
   }

   let written = match PoolStore::open_at(path, key) {
      // The database is dropped at the end of this arm, so a failure can move the blob back.
      Ok(store) => store.write_catalog(
         &catalog.meta,
         &catalog.pools,
         &catalog.v4,
         &catalog.checkpoints,
         &catalog.discovered,
         &catalog.discovered_v4,
      ),
      Err(e) => Err(e),
   };

   if let Err(e) = written {
      let _ = std::fs::remove_file(path);

      if let Err(restore) = std::fs::rename(&legacy, path) {
         tracing::error!(
            "Failed to restore {} after a failed import: {restore}",
            path.display()
         );
      }

      return Err(e);
   }

   std::fs::remove_file(&legacy)?;
   manager.clear_dirty();

   Ok(Some(manager))
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

   let zeus_client = ctx.get_client_manager();
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

      let task = ctx.rt().spawn(async move {
         let _permit = semaphore.acquire().await?;
         let res = zeus_client
            .request_with(
               chain,
               (
                  v2_chunk.clone(),
                  v3_chunk.clone(),
                  v4_chunk.clone(),
               ),
               |client, (v2_chunk, v3_chunk, v4_chunk)| async move {
                  batch::get_pools_state(
                     client.clone(),
                     chain,
                     v2_chunk,
                     v3_chunk,
                     v4_chunk,
                     state_view,
                  )
                  .await
               },
            )
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
   let num_batches = total_pools.div_ceil(batch_size);

   let chunk_size_v2 = all_v2_addresses.len().div_ceil(num_batches);
   let chunk_size_v3 = all_v3_pool_info.len().div_ceil(num_batches);
   let chunk_size_v4 = all_v4_pool_info.len().div_ceil(num_batches);

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
   use tempfile::tempdir;

   use super::*;
   use zeus_eth::{alloy_primitives::U256, amm::uniswap::UniswapV4Pool};

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

   #[test]
   fn sync_steps_cover_the_range() {
      // The ranges are inclusive, so 1_000_000 blocks are ten steps of 100_000 and the last one
      // ends on the last block covered.
      let steps = sync_steps(0, 999_999, 100_000);
      assert_eq!(steps.len(), 10);
      assert_eq!(steps.first(), Some(&(0, 99_999)));
      assert_eq!(steps.last().map(|(_, end)| *end), Some(999_999));
      assert!(
         steps.windows(2).all(|pair| pair[1].0 == pair[0].1 + 1),
         "steps must be contiguous: {steps:?}"
      );

      // One block past a step boundary is its own step: no block is skipped or repeated.
      let steps = sync_steps(0, 1_000_000, 100_000);
      assert_eq!(steps.len(), 11);
      assert_eq!(steps.last(), Some(&(1_000_000, 1_000_000)));

      assert_eq!(sync_steps(5, 5, 100_000), vec![(5, 5)]);
      assert!(sync_steps(10, 9, 100_000).is_empty());
   }

   /// The snapshot is built by `zeus-pools` from its own log sync, so its checkpoint describes that
   /// run, not this wallet. Inheriting it made a fresh install report a "last synced block"
   /// (mainnet 25_909_538) and made `sync_v4_pools` resume from it, skipping every block below.
   #[test]
   fn snapshot_checkpoints_are_not_inherited() {
      let mut snapshot = PoolManager::empty();
      snapshot.add_pool(UniswapV4Pool::eth_uni());
      snapshot.checkpoints.insert(
         (1, DexKind::UniswapV4),
         Checkpoint::new(1, 25_909_538, DexKind::UniswapV4),
      );
      snapshot.last_discover.insert(
         (
            1,
            ERC20Token::weth().address,
            ERC20Token::usdc().address,
         ),
         1,
      );

      let manager = PoolManager::from_snapshot(snapshot);

      assert_eq!(
         manager.pools.len(),
         1,
         "the pools are the point of the snapshot"
      );
      assert!(
         manager.checkpoints.is_empty(),
         "a snapshot checkpoint is not this wallet's sync and must not be resumed from"
      );
      assert!(manager.last_discover.is_empty());
   }

   #[test]
   fn v4_pool_is_hot_only_when_both_currencies_resolve() {
      let tokens = CurrencyDB::default();
      let weth = ERC20Token::weth();
      let usdc = ERC20Token::usdc();
      let unknown = Address::repeat_byte(0xab);

      let record = |currency0, currency1| V4PoolRecord {
         currency0,
         currency1,
         fee: 500,
         tick_spacing: 10,
         hooks: Address::ZERO,
      };

      assert!(
         v4_pool_from_record(1, &record(weth.address, usdc.address), &tokens).is_some(),
         "two base tokens are hot"
      );
      assert!(
         v4_pool_from_record(1, &record(weth.address, unknown), &tokens).is_none(),
         "a token the wallet does not know keeps the pool on disk"
      );

      let native = v4_pool_from_record(1, &record(Address::ZERO, weth.address), &tokens)
         .expect("native + base is hot");
      assert!(native.currency0().is_native());
      assert_eq!(
         native.id(),
         record(Address::ZERO, weth.address).id()
      );
   }

   #[test]
   fn catalog_roundtrips_through_the_store() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = WalletStateKey::generate().unwrap();

      let v2 = AnyUniswapPool::V2(UniswapV2Pool::weth_uni());
      let hot = AnyUniswapPool::V4(UniswapV4Pool::new_with_spacing(
         1,
         FeeAmount::LOW,
         10,
         DexKind::UniswapV4,
         Currency::native(1),
         Currency::from(ERC20Token::weth()),
         State::none(),
         Address::ZERO,
      ));
      let cold = V4PoolRecord {
         currency0: ERC20Token::weth().address,
         currency1: Address::repeat_byte(0xab),
         fee: 3000,
         tick_spacing: 60,
         hooks: Address::ZERO,
      };

      let store = PoolStore::open_at(&path, &key).unwrap();
      store.put_v4_record(1, &cold).unwrap();

      let source = PoolManagerHandle::new(PoolManager::empty());
      source.write(|manager| {
         manager.concurrency = 7;
         manager.batch_size_for_updating_pool_state = 11;
         manager.batch_size_for_discovering_pools = 13;
         manager.discover_v4_pools = true;
         manager.ignore_chains = HashSet::from([1]);
         manager.store = Some(store.clone());
      });

      source.add_pool(v2.clone());
      source.add_pool(hot.clone());
      source.add_checkpoint(
         1,
         DexKind::UniswapV4,
         Checkpoint::new(1, 5_000, DexKind::UniswapV4),
      );
      source.add_last_discover_time(
         1,
         ERC20Token::weth().address,
         ERC20Token::usdc().address,
      );
      source.save_to_store(&store).unwrap();

      let loaded = PoolManagerHandle::new(PoolManager::empty());
      loaded.load_from_store(store, &CurrencyDB::default()).unwrap();

      let pools = loaded.read(|manager| manager.pools.clone());
      assert_eq!(
         pools.len(),
         2,
         "the v2 pool and the hot v4 pool; the cold record stays on disk"
      );
      assert!(loaded.get_pool_from_address(1, v2.address()).is_some());
      assert!(loaded.get_v4_pool_from_id(1, hot.id()).is_some());

      let checkpoint = loaded.read(|manager| {
         manager
            .checkpoints
            .get(&(1, DexKind::UniswapV4))
            .map(|checkpoint| checkpoint.block)
      });
      assert_eq!(checkpoint, Some(5_000));
      assert_eq!(loaded.concurrency(), 7);
      assert_eq!(loaded.batch_size_for_updating_pools_state(), 11);
      assert!(loaded.read(|manager| manager.discover_v4_pools));
      assert_eq!(
         loaded.read(|manager| manager.ignore_chains.clone()),
         HashSet::from([1])
      );
      assert!(loaded.read(|manager| manager.store.is_some()));
      assert!(
         source.read(|manager| manager.dirty_pools.is_empty()),
         "a flush clears its dirty marks"
      );
   }

   fn fake_token(address: Address) -> ERC20Token {
      ERC20Token {
         chain_id: 1,
         address,
         symbol: "X".into(),
         name: "X".into(),
         decimals: 18,
         total_supply: U256::ZERO,
      }
   }

   fn record_for(currency0: Address, currency1: Address) -> V4PoolRecord {
      V4PoolRecord {
         currency0,
         currency1,
         fee: 500,
         tick_spacing: 10,
         hooks: Address::ZERO,
      }
   }

   #[test]
   fn cold_v4_pools_are_promoted_for_a_new_token() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = WalletStateKey::generate().unwrap();
      let store = PoolStore::open_at(&path, &key).unwrap();

      let weth = ERC20Token::weth();
      let token_a = fake_token(Address::repeat_byte(0xaa));
      let record = record_for(weth.address, token_a.address);
      store.put_v4_record(1, &record).unwrap();

      let handle = PoolManagerHandle::new(PoolManager::empty());
      handle.load_from_store(store, &CurrencyDB::default()).unwrap();
      assert!(
         handle.get_v4_pool_from_id(1, record.id()).is_none(),
         "the pool is cold until its other side is imported"
      );

      let mut db = CurrencyDB::default();
      db.insert_token(1, token_a.clone());

      assert_eq!(
         handle.promote_v4_pools_for_tokens(1, &[token_a.clone()], &db).unwrap(),
         1
      );

      let pool = handle.get_v4_pool_from_id(1, record.id()).expect("the catalog pool is hot now");
      assert_eq!(pool.id(), record.id());
      assert!(
         handle.read(|manager| manager.dirty_pools.is_empty()),
         "a promoted pool is already on disk, so it is not dirty"
      );

      assert_eq!(
         handle.promote_v4_pools_for_tokens(1, &[token_a], &db).unwrap(),
         0,
         "promotion is idempotent"
      );
   }

   #[test]
   fn promote_keeps_a_pool_whose_other_side_is_unknown() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = WalletStateKey::generate().unwrap();
      let store = PoolStore::open_at(&path, &key).unwrap();

      let weth = ERC20Token::weth();
      let token_a = fake_token(Address::repeat_byte(0xab));
      let unknown = fake_token(Address::repeat_byte(0xcd));
      let record = record_for(weth.address, unknown.address);
      store.put_v4_record(1, &record).unwrap();

      let handle = PoolManagerHandle::new(PoolManager::empty());
      handle.load_from_store(store, &CurrencyDB::default()).unwrap();

      let mut db = CurrencyDB::default();
      db.insert_token(1, token_a.clone());

      assert_eq!(
         handle.promote_v4_pools_for_tokens(1, &[token_a], &db).unwrap(),
         0
      );
      assert!(
         handle.get_v4_pool_from_id(1, record.id()).is_none(),
         "the record stays on disk while its other side is unknown"
      );
   }

   #[test]
   fn load_indexes_every_cold_record_under_both_currencies() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = WalletStateKey::generate().unwrap();
      let store = PoolStore::open_at(&path, &key).unwrap();

      let weth = ERC20Token::weth();
      let token_a = fake_token(Address::repeat_byte(0xaa));
      let token_b = fake_token(Address::repeat_byte(0xbb));
      let record_a = record_for(weth.address, token_a.address);
      let record_b = record_for(weth.address, token_b.address);
      store.put_v4_record(1, &record_a).unwrap();
      store.put_v4_record(1, &record_b).unwrap();

      let handle = PoolManagerHandle::new(PoolManager::empty());
      handle.load_from_store(store, &CurrencyDB::default()).unwrap();

      let indexed = handle.read(|manager| manager.cold_v4.get(&(1, weth.address)).cloned());
      let indexed = indexed.expect("both records are indexed under weth");
      assert_eq!(indexed.len(), 2);
      assert!(indexed.contains(&record_a.id()));
      assert!(indexed.contains(&record_b.id()));

      let mut db = CurrencyDB::default();
      db.insert_token(1, token_a.clone());
      assert_eq!(
         handle.promote_v4_pools_for_tokens(1, &[token_a], &db).unwrap(),
         1
      );

      let remaining = handle
         .read(|manager| manager.cold_v4.get(&(1, weth.address)).cloned())
         .expect("the other cold record is still indexed");
      assert_eq!(remaining, vec![record_b.id()]);
      assert!(
         handle
            .read(|manager| manager.cold_v4.get(&(1, record_a.currency1)).cloned())
            .is_none_or(|ids| ids.is_empty()),
         "the promoted pool is gone from the new token's bucket"
      );
   }

   #[test]
   fn legacy_blob_is_migrated() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = WalletStateKey::generate().unwrap();
      let weth = ERC20Token::weth().address;
      let usdc = ERC20Token::usdc().address;

      let mut legacy = PoolManager::empty();
      legacy.add_pool(UniswapV2Pool::weth_uni());
      legacy.add_pool(UniswapV4Pool::eth_uni());
      legacy.add_checkpoint(
         1,
         DexKind::UniswapV4,
         Checkpoint::new(1, 777, DexKind::UniswapV4),
      );
      legacy.last_discover.insert((1, weth, usdc), 1_234);

      let sealed = key.seal_json(&legacy, POOL_DATA_AAD).unwrap();
      std::fs::write(&path, &sealed).unwrap();

      let migrated = migrate_legacy_blob(&path, &key, &CurrencyDB::default())
         .unwrap()
         .expect("a sealed blob must be imported");

      assert!(
         migrated
            .get_v2_pool_from_address(1, UniswapV2Pool::weth_uni().address())
            .is_some(),
         "the v2 pool comes back"
      );
      assert_eq!(
         migrated.pools.len(),
         1,
         "the v4 pool has an unknown currency, so it stays on disk"
      );
      assert!(
         !dir.path().join("pool_data.data.legacy").exists(),
         "the blob is removed once the catalog exists"
      );

      let store = PoolStore::open_at(&path, &key).unwrap();
      assert_eq!(store.load_v2_v3_pools().unwrap().len(), 1);
      assert_eq!(
         store.load_v4_records(Some(1)).unwrap().len(),
         1,
         "the v4 pool is a cold record now"
      );
      assert_eq!(
         store.checkpoints().unwrap(),
         vec![(1, DexKind::UniswapV4, 777)]
      );
      assert_eq!(
         store.discovered().unwrap(),
         vec![((1, weth, usdc), 1_234)]
      );
   }
}
