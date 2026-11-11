use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::{
   sync::{Semaphore, mpsc},
   task::JoinHandle,
};
use tracing::info;
use zeus_eth::abi::zeus::ZeusStateViewV3::{PoolsState, V4PoolData};
use zeus_eth::alloy_primitives::FixedBytes;
use zeus_eth::amm::uniswap::UniswapV4Pool;

use crate::assets;
use crate::context::{
   CurrencyDB, DEFAULT_POOL_MINIMUM_LIQUIDITY, RwLockHandle,
   pool_store::{POOL_DB_FORMAT, PoolDbMeta, PoolStore, V4PoolRecord},
};
use crate::persisted::ensure_data_dir;
use crate::platform::TimeStamp;
use crate::{WalletStateKey, ZeusCtx, context::pool_data_dir, serde_hashmap};
use zeus_eth::{
   abi::zeus::ZeusStateViewV3::{V3Pool, V4Pool},
   alloy_primitives::{Address, B256, U256},
   alloy_provider::Provider,
   amm::uniswap::{
      AnyUniswapPool, DexKind, FeeAmount, UniswapPool, UniswapV2Pool, UniswapV3Pool,
      V4_FEE_CANDIDATES,
      state::{State, V3PoolState},
      sync::{Checkpoint, v4_pools_from_logs},
      v4::{DYNAMIC_FEE_FLAG, v4_virtual_reserves},
      v4_fee_spacing_candidates,
   },
   currency::{Currency, ERC20Token, NativeCurrency},
   types::*,
   utils::{
      GET_LOGS_MAX_RETRIES, NumericValue,
      address_book::{uniswap_v2_factory, uniswap_v3_factory, uniswap_v4_stateview},
      batch, is_base_token,
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

/// Narrowest configurable `eth_getLogs` span. Anything smaller turns one step into enough
/// requests to trip a rate limit for no gain.
pub const MIN_V4_SYNC_BLOCK_RANGE: u64 = 1_000;

/// Widest configurable `eth_getLogs` span. Public endpoints routinely reject more than this.
pub const MAX_V4_SYNC_BLOCK_RANGE: u64 = 50_000;

fn default_v4_sync_block_range() -> u64 {
   V4_SYNC_BLOCK_RANGE
}

/// Clamp the configurable span, applied on load and again on set so a hand-edited catalog or a
/// stale slider value can never reach `get_logs_for`.
fn clamp_v4_sync_block_range(block_range: u64) -> u64 {
   block_range.clamp(MIN_V4_SYNC_BLOCK_RANGE, MAX_V4_SYNC_BLOCK_RANGE)
}

/// Fee ceiling of the V4 cleanup, as a percentage of the swap amount. A pool above it is deleted.
pub const DEFAULT_V4_MAX_FEE_PERCENT: f32 = 2.0;

/// The same ceiling for a pool that pairs a base asset. The deepest V4 pools sit between 0.01% and
/// 0.3%, so a higher fee on an ETH or USDC pair is someone hoping to be mistaken for one.
pub const DEFAULT_V4_MAX_BASE_FEE_PERCENT: f32 = 1.0;

/// Lowest configurable ceiling; 0.01% is the smallest V4 fee tier.
pub const MIN_V4_MAX_FEE_PERCENT: f32 = 0.01;

/// Highest configurable ceiling: above it a cleanup would delete every V4 pool there is.
pub const MAX_V4_MAX_FEE_PERCENT: f32 = 10.0;

fn default_v4_max_fee_percent() -> f32 {
   DEFAULT_V4_MAX_FEE_PERCENT
}

fn default_v4_max_base_fee_percent() -> f32 {
   DEFAULT_V4_MAX_BASE_FEE_PERCENT
}

/// Clamp a cleanup ceiling, on load and again on set, so a hand-edited catalog or a stale slider can
/// neither wipe the catalog nor sit outside the slider's own range. A non-positive or non-finite
/// value falls back to `default` instead of becoming a ceiling of zero.
fn clamp_fee_percent(percent: f32, default: f32) -> f32 {
   if !percent.is_finite() || percent <= 0.0 {
      return default;
   }

   percent.clamp(MIN_V4_MAX_FEE_PERCENT, MAX_V4_MAX_FEE_PERCENT)
}

/// Records deleted per catalog transaction during a cleanup: durable progress without a commit per
/// row.
const V4_CLEANUP_DELETE_BATCH: usize = 2_048;

/// Pools per `getV4PoolState` call. The view reads ~8 storage slots per pool, so a hundred of them
/// stays well inside an RPC's eth_call gas cap.
const V4_CLEANUP_STATE_BATCH: usize = 100;

/// Pools between progress lines while the cleanup walks the catalog.
const V4_CLEANUP_PROGRESS_STEP: usize = 20_000;

/// Token addresses per `getERC20InfoBatch` call when loading the catalog into memory.
const V4_CATALOG_TOKEN_BATCH: usize = 100;

/// What one [`PoolManagerHandle::load_catalog_pools`] run brought into memory.
#[derive(Debug, Clone, Copy, Default)]
pub struct CatalogLoadReport {
   /// Distinct currencies the chain's cold index knows.
   pub indexed: usize,
   /// Tokens whose metadata was fetched and stored.
   pub tokens: usize,
   /// Addresses the batch could not describe: an NFT, or a contract that is not an ERC20.
   pub failed: usize,
   /// Cold records that became full pools, so the pools window can show them.
   pub loaded: usize,
}

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
#[derive(Debug, Clone, Default)]
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
   /// The cleanup the run ended with: a sync deletes what its own steps discovered.
   pub cleanup: Option<V4CleanupReport>,
   /// Why that cleanup failed. Every step above is durable, so this is reported rather than returned.
   pub cleanup_error: Option<String>,
}

/// The knobs of one [`PoolManagerHandle::cleanup_v4_pools`] run.
///
/// A `0` means "use the stored value": the ceilings the pools window's settings write into the
/// catalog.
#[derive(Debug, Clone, Copy, Default)]
pub struct V4CleanupConfig {
   /// Pools charging more than this percentage are deleted.
   pub max_fee_percent: f32,
   /// Pools pairing a base asset are held to this stricter ceiling.
   pub max_base_fee_percent: f32,
}

/// What one [`PoolManagerHandle::cleanup_v4_pools`] run deleted.
#[derive(Debug, Clone, Copy, Default)]
pub struct V4CleanupReport {
   /// V4 records the chain held when the run started.
   pub scanned: usize,
   /// Dynamic-fee pools, which no fee ceiling can judge, so they are always kept.
   pub dynamic_skipped: usize,
   /// Deleted for charging more than the general ceiling.
   pub removed_by_fee: usize,
   /// Deleted for charging more than the base-asset ceiling.
   pub removed_by_base_fee: usize,
   /// Deleted for having no base asset on either side: a pair of unknowns nobody routes through.
   pub removed_no_base: usize,
   /// Deleted for having no in-range liquidity at all.
   pub removed_dead: usize,
   /// Deleted for holding less than `crate::context::DEFAULT_POOL_MINIMUM_LIQUIDITY` of value.
   pub removed_illiquid: usize,
   /// Kept because their base asset had no price: a missing feed is not zero liquidity.
   pub unjudged_no_price: usize,
   /// Kept because their state could not be read (a failed request, or a spacing that cannot exist).
   pub unjudged_no_state: usize,
}

impl V4CleanupReport {
   pub fn removed(&self) -> usize {
      self.removed_by_fee
         + self.removed_by_base_fee
         + self.removed_no_base
         + self.removed_dead
         + self.removed_illiquid
   }

   /// Pools kept because nothing could be said about their liquidity.
   pub fn unjudged(&self) -> usize {
      self.unjudged_no_price + self.unjudged_no_state
   }
}

/// What the V4 side of one chain's catalog holds, for the pools window's own accounting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct V4CatalogStats {
   /// Rows the catalog holds for the chain: a hot pool and a cold record are both one row.
   pub total: usize,
   /// Cold records: the 68-byte row on disk, its id in the index.
   pub cold: usize,
   /// Hot pools: the row plus the whole pool in memory.
   pub hot: usize,
   /// Approximate bytes the cold index costs: its keys, the id vectors and their buckets.
   pub cold_bytes: usize,
   /// Approximate bytes the hot pools cost: the enum slot and the key it hangs under.
   pub hot_bytes: usize,
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
         manager.v4_sync_block_range = clamp_v4_sync_block_range(meta.v4_sync_block_range);
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

   /// The `eth_getLogs` span a V4 sync uses, clamped into [`MIN_V4_SYNC_BLOCK_RANGE`]`..=`
   /// [`MAX_V4_SYNC_BLOCK_RANGE`].
   pub fn v4_sync_block_range(&self) -> u64 {
      clamp_v4_sync_block_range(self.read(|manager| manager.v4_sync_block_range))
   }

   /// Set the span. Persisted with the rest of the catalog metadata on the next save.
   pub fn set_v4_sync_block_range(&self, block_range: u64) {
      let block_range = clamp_v4_sync_block_range(block_range);
      self.write(|manager| manager.v4_sync_block_range = block_range);
   }

   /// The fee ceiling [`Self::cleanup_v4_pools`] deletes at, clamped like the slider that writes it.
   pub fn v4_max_fee_percent(&self) -> f32 {
      clamp_fee_percent(
         self.read(|manager| manager.v4_max_fee_percent),
         DEFAULT_V4_MAX_FEE_PERCENT,
      )
   }

   pub fn set_v4_max_fee_percent(&self, percent: f32) {
      let percent = clamp_fee_percent(percent, DEFAULT_V4_MAX_FEE_PERCENT);
      self.write(|manager| manager.v4_max_fee_percent = percent);
   }

   /// The stricter ceiling applied to pools that pair a base asset.
   pub fn v4_max_base_fee_percent(&self) -> f32 {
      clamp_fee_percent(
         self.read(|manager| manager.v4_max_base_fee_percent),
         DEFAULT_V4_MAX_BASE_FEE_PERCENT,
      )
   }

   pub fn set_v4_max_base_fee_percent(&self, percent: f32) {
      let percent = clamp_fee_percent(percent, DEFAULT_V4_MAX_BASE_FEE_PERCENT);
      self.write(|manager| manager.v4_max_base_fee_percent = percent);
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

   /// Pull every cold V4 record on `chain` into memory, so the pools window can show it.
   ///
   /// A record is cold because one of its currencies is unknown, so this fetches the metadata of every
   /// unknown address in the chain's cold index (one `getERC20InfoBatch` call per
   /// [`V4_CATALOG_TOKEN_BATCH`] addresses) and promotes the records that now resolve. Nothing is
   /// deleted and the catalog keeps every row: the pools simply become hot, which is what makes them
   /// visible.
   ///
   /// The fetched tokens are **not** written to the currency DB. That DB is part of the wallet state
   /// and is persisted, and none of these tokens is tradable: a catalog of hundreds of thousands of
   /// scam pools would bloat the wallet permanently for an inspection that lasts one session. The
   /// promotion gets them through a local copy of the DB, so the pools hold their metadata in memory
   /// and go cold again on restart.
   pub async fn load_catalog_pools(
      &self,
      ctx: ZeusCtx,
      chain: u64,
   ) -> Result<CatalogLoadReport, anyhow::Error> {
      let mut report = CatalogLoadReport::default();

      let addresses = self.read(|manager| {
         manager
            .cold_v4
            .keys()
            .filter(|(pool_chain, _)| *pool_chain == chain)
            .map(|(_, address)| *address)
            .collect::<HashSet<Address>>()
      });
      report.indexed = addresses.len();

      let known = ctx.read(|ctx| ctx.currency_db.clone());
      let missing = addresses
         .into_iter()
         .filter(|address| {
            !address.is_zero() && resolve_currency(chain, *address, &known).is_none()
         })
         .collect::<Vec<_>>();

      if missing.is_empty() {
         return Ok(report);
      }

      let client = ctx.get_client(chain).await?;
      let concurrency = ctx.get_client_manager().concurrency().max(1);
      let semaphore = Arc::new(Semaphore::new(concurrency));
      let (tx, mut rx) =
         mpsc::channel::<(usize, Result<Vec<ERC20Token>, anyhow::Error>)>(concurrency);

      for chunk in missing.chunks(V4_CATALOG_TOKEN_BATCH) {
         let client = client.clone();
         let tx = tx.clone();
         let semaphore = semaphore.clone();
         let chunk = chunk.to_vec();

         ctx.rt().spawn(async move {
            let Ok(_permit) = semaphore.acquire_owned().await else {
               return;
            };

            let result = ERC20Token::from_batch(client, chain, chunk.clone()).await;
            let _ = tx.send((chunk.len(), result)).await;
         });
      }

      drop(tx);

      let mut fetched = Vec::new();
      while let Some((asked, result)) = rx.recv().await {
         let tokens = match result {
            Ok(tokens) => tokens,
            Err(e) => {
               tracing::warn!(
                  "V4 catalog load: {} tokens could not be read: {:?}",
                  asked,
                  e
               );
               report.failed += asked;
               continue;
            }
         };

         // An address the batch skipped is not an ERC20, or would not decode.
         report.failed += asked.saturating_sub(tokens.len());
         report.tokens += tokens.len();

         fetched.extend(tokens);
      }

      // Promotion resolves both sides through the DB, so the fetched tokens go on a local copy of it:
      // the wallet's own DB is never touched.
      let mut known = ctx.read(|ctx| ctx.currency_db.clone());
      for token in &fetched {
         known.insert_token(chain, token.clone());
      }
      report.loaded = self.promote_v4_pools_for_tokens(chain, &fetched, &known)?;

      Ok(report)
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

   /// The V4 side of this chain's catalog: how many rows the window shows, and roughly what they
   /// cost in memory.
   pub fn v4_catalog_stats(&self, chain: u64) -> V4CatalogStats {
      self.read(|manager| manager.v4_catalog_stats(chain))
   }

   /// A cheap revision of this chain's V4 catalog, for a window that caches what it built from it.
   ///
   /// Every add and every delete moves the pool count or the cold id count, and a table row depends
   /// on nothing else about a pool -- not its state, not its reserves. The checkpoint moves with the
   /// sync's steps, so a step that stored only cold records still invalidates the cache.
   pub fn v4_revision(&self, chain: u64) -> (usize, usize, Option<u64>) {
      self.read(|manager| {
         let cold = manager
            .cold_v4
            .iter()
            .filter(|((pool_chain, _), _)| *pool_chain == chain)
            .map(|(_, ids)| ids.len())
            .sum::<usize>();
         let checkpoint = manager
            .checkpoints
            .get(&(chain, DexKind::UniswapV4))
            .map(|checkpoint| checkpoint.block);

         (manager.pools.len(), cold, checkpoint)
      })
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
         (report.cleanup, report.cleanup_error) = self.cleanup_after_sync(ctx, chain).await;

         return Ok(report);
      }

      let step = match config.step_blocks {
         0 => V4_SYNC_STEP_BLOCKS,
         step => step,
      };
      let range = match config.block_range {
         0 => self.v4_sync_block_range(),
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
      // One read for the whole chain instead of a lookup per discovered pool.
      known.extend(store.v4_ids(chain)?);

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
            if known.contains(&id) {
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

         // The catalog write is I/O: one immediate-durability transaction per step, sealing every
         // record and fsyncing the pages. It runs outside the manager lock, so a frame that reads the
         // pool list or the counters never waits on the disk. Durable first, then memory: a load
         // rebuilds whatever the manager did not get to.
         store.commit_v4_step(chain, &records, end)?;

         self.write(|manager| {
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
         });

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

      (report.cleanup, report.cleanup_error) = self.cleanup_after_sync(ctx, chain).await;

      Ok(report)
   }

   /// Clean the catalog after a sync.
   ///
   /// A sync's own steps are the newest junk in the catalog, and every row that survives them is a
   /// row on disk at the next start. A failure comes back as a message rather than an `Err`: every
   /// step above already committed, so a node that cannot be read must not turn a durable sync into
   /// a failed one.
   async fn cleanup_after_sync(
      &self,
      ctx: ZeusCtx,
      chain: u64,
   ) -> (Option<V4CleanupReport>, Option<String>) {
      match self.cleanup_v4_pools(ctx, chain, V4CleanupConfig::default()).await {
         Ok(report) => (Some(report), None),
         Err(e) => {
            tracing::error!("V4 cleanup after sync on chain {chain} failed: {e:?}");
            (None, Some(format!("{e:?}")))
         }
      }
   }

   /// Sync `chain` from its V4 creation block again, forgetting the saved checkpoint first.
   ///
   /// The catalog is kept. A cleanup deletes rows, and only the full log scan can bring back a pool
   /// it judged too eagerly; the rows that survived are already where they belong. The run ends with
   /// the sync's own cleanup, which judges everything it re-discovered by the current rules.
   pub async fn resync_v4_pools(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      config: V4SyncConfig,
   ) -> Result<V4SyncReport, anyhow::Error> {
      // Before the checkpoint goes: a chain with no usable endpoint fails here instead of after it
      // has lost the block it would have resumed from.
      ctx.get_client(chain).await?;

      self.forget_v4_checkpoint(chain)?;
      self.sync_v4_pools(ctx, chain, config).await
   }

   /// Forget this chain's V4 checkpoint in memory and in the catalog, so the next sync starts over.
   fn forget_v4_checkpoint(&self, chain: u64) -> Result<(), anyhow::Error> {
      let store = self.write(|manager| {
         let key = (chain, DexKind::UniswapV4);
         manager.checkpoints.remove(&key);
         manager.dirty_checkpoints.remove(&key);

         manager.store.clone()
      });

      match store {
         Some(store) => store.remove_checkpoint(chain, DexKind::UniswapV4),
         None => Ok(()),
      }
   }

   /// Delete the V4 pools on `chain` that no one would ever swap through, for good.
   ///
   /// Three rules, in order:
   ///
   /// 1. a fee above `max_fee_percent` (default 2%) -- dynamic-fee pools are skipped, their key holds a
   ///    flag rather than a tier;
   /// 2. a fee above `max_base_fee_percent` (default 1%) when either side is a base asset, since a real
   ///    ETH, WETH or USDC pool never charges that much;
   /// 3. a pool whose state says nobody can swap through it: no in-range liquidity at all, or a
   ///    base-side balance worth less than `crate::context::DEFAULT_POOL_MINIMUM_LIQUIDITY`.
   ///
   /// The first two are answered by the 68-byte record alone, so the bulk of a scam-laden catalog goes
   /// without a single RPC call. The third reads `getV4PoolState` for every surviving record --
   /// including the cold ones this wallet cannot name, because that call takes a pool id and a tick
   /// spacing and nothing else. Deletions are durable chunk by chunk: a crash mid-run leaves the
   /// catalog valid and the next run picks up the remainder.
   pub async fn cleanup_v4_pools(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      config: V4CleanupConfig,
   ) -> Result<V4CleanupReport, anyhow::Error> {
      let key = ctx.read_vault(|vault| vault.wallet_state_key())?;
      let store = match self.read(|manager| manager.store.clone()) {
         Some(store) => store,
         None => {
            // No catalog open yet: create it from the manager in memory, so the deletions below have
            // rows to remove.
            self.save_to_file(&key)?;
            self
               .read(|manager| manager.store.clone())
               .ok_or_else(|| anyhow!("the pool catalog was not attached"))?
         }
      };

      // Fetched before the first deletion: a chain with no RPC must fail cleanly instead of leaving
      // the catalog half cleaned.
      let client = ctx.get_client(chain).await?;

      let max_fee_percent = match config.max_fee_percent {
         0.0 => self.v4_max_fee_percent(),
         percent => clamp_fee_percent(percent, DEFAULT_V4_MAX_FEE_PERCENT),
      };
      let max_base_fee_percent = match config.max_base_fee_percent {
         0.0 => self.v4_max_base_fee_percent(),
         percent => clamp_fee_percent(percent, DEFAULT_V4_MAX_BASE_FEE_PERCENT),
      };

      let records = store
         .load_v4_records(Some(chain))?
         .into_iter()
         .map(|(_, record)| record)
         .collect::<Vec<_>>();
      let plan = plan_v4_cleanup(
         chain,
         records,
         max_fee_percent,
         max_base_fee_percent,
      );

      let mut report = V4CleanupReport {
         scanned: plan.remove.len() + plan.keep.len(),
         dynamic_skipped: plan.dynamic,
         ..Default::default()
      };

      // Rules 1 and 2, straight from the records: no currency resolution, no RPC.
      let mut doomed = Vec::with_capacity(V4_CLEANUP_DELETE_BATCH);
      for (record, reason) in plan.remove {
         doomed.push((record, reason));

         if doomed.len() == V4_CLEANUP_DELETE_BATCH {
            self.drop_v4_records(&store, chain, &doomed, &mut report)?;
            doomed.clear();
         }
      }
      self.drop_v4_records(&store, chain, &doomed, &mut report)?;
      doomed.clear();

      // Rule 3 skips dynamic-fee pools too: their economics belong to the hook, and "skip dynamic for
      // now" has to mean every rule, not just the fee ones.
      let candidates = plan
         .keep
         .into_iter()
         .filter(|record| record.fee != DYNAMIC_FEE_FLAG)
         .collect::<Vec<_>>();

      let kept = candidates.len();
      if kept == 0 {
         tracing::info!(
            "V4 cleanup chain {}: {} of {} pools deleted ({} fee, {} base-asset fee), {} dynamic",
            chain,
            report.removed(),
            report.scanned,
            report.removed_by_fee,
            report.removed_by_base_fee,
            report.dynamic_skipped
         );

         return Ok(report);
      }

      // Rule 3 reads the state of every survivor, cold rows included: `getV4PoolState` takes a pool id
      // and a tick spacing, not a `Currency`, so a pool this wallet cannot name is still judged.
      let record_by_id = candidates
         .iter()
         .map(|record| (record.id(), *record))
         .collect::<HashMap<B256, V4PoolRecord>>();

      let concurrency = ctx.get_client_manager().concurrency().max(1);
      let semaphore = Arc::new(Semaphore::new(concurrency));
      let (tx, mut rx) = mpsc::channel::<Result<Vec<V4PoolData>, anyhow::Error>>(concurrency);

      for chunk in candidates.chunks(V4_CLEANUP_STATE_BATCH) {
         let client = client.clone();
         let tx = tx.clone();
         let semaphore = semaphore.clone();

         let mut pools = Vec::with_capacity(chunk.len());
         for record in chunk {
            // A pool whose tick spacing is 0 cannot exist on chain, so it cannot be read either.
            let Some(tick_spacing) = FeeAmount::i24_tick_spacing(record.tick_spacing) else {
               continue;
            };

            pools.push(V4Pool {
               pool: record.id(),
               tickSpacing: tick_spacing,
            });
         }

         ctx.rt().spawn(async move {
            let Ok(_permit) = semaphore.acquire_owned().await else {
               return;
            };

            let result = batch::get_v4_pool_state(client, chain, pools).await;
            let _ = tx.send(result).await;
         });
      }

      drop(tx);

      let mut judged = HashSet::new();
      let mut next_log = V4_CLEANUP_PROGRESS_STEP;

      while let Some(result) = rx.recv().await {
         let data = match result {
            Ok(data) => data,
            Err(e) => {
               // A request that failed is not a verdict: those pools stay and the run carries on.
               tracing::warn!("V4 cleanup: pool state request failed: {:?}", e);
               continue;
            }
         };

         for pool in data {
            let Some(record) = record_by_id.get(&pool.pool) else {
               continue;
            };
            judged.insert(pool.pool);

            let price_of = |token: &ERC20Token| ctx.get_token_price(token).f64();
            let sqrt_price = U256::from(pool.sqrtPriceX96);

            match judge_v4_pool(
               chain,
               record,
               pool.liquidity,
               sqrt_price,
               price_of,
            ) {
               V4LiquidityVerdict::Keep => {}
               V4LiquidityVerdict::Remove(reason) => doomed.push((*record, reason)),
               V4LiquidityVerdict::UnjudgedNoPrice => report.unjudged_no_price += 1,
               V4LiquidityVerdict::UnjudgedBadState => report.unjudged_no_state += 1,
            }
         }

         if judged.len() >= next_log {
            tracing::info!(
               "V4 cleanup chain {}: {} of {} pools judged",
               chain,
               judged.len(),
               kept
            );
            next_log += V4_CLEANUP_PROGRESS_STEP;
         }

         if doomed.len() >= V4_CLEANUP_DELETE_BATCH {
            self.drop_v4_records(&store, chain, &doomed, &mut report)?;
            doomed.clear();
         }
      }

      self.drop_v4_records(&store, chain, &doomed, &mut report)?;

      // Records we never got state for: a failed request, or a spacing that cannot exist.
      report.unjudged_no_state += kept.saturating_sub(judged.len());

      tracing::info!(
         "V4 cleanup chain {}: {} of {} pools deleted ({} fee, {} base-asset fee, {} no base asset, {} dead, {} illiquid), {} dynamic skipped, {} unjudged ({} no price, {} no state)",
         chain,
         report.removed(),
         report.scanned,
         report.removed_by_fee,
         report.removed_by_base_fee,
         report.removed_no_base,
         report.removed_dead,
         report.removed_illiquid,
         report.dynamic_skipped,
         report.unjudged(),
         report.unjudged_no_price,
         report.unjudged_no_state
      );

      Ok(report)
   }

   /// Delete a chunk of V4 records from the catalog and from memory, and count them by reason.
   fn drop_v4_records(
      &self,
      store: &PoolStore,
      chain: u64,
      records: &[(V4PoolRecord, V4RemovalReason)],
      report: &mut V4CleanupReport,
   ) -> Result<(), anyhow::Error> {
      if records.is_empty() {
         return Ok(());
      }

      let ids = records.iter().map(|(record, _)| record.id()).collect::<Vec<_>>();
      store.remove_v4_records(chain, &ids)?;

      self.write(|manager| {
         let forgotten = records.iter().map(|(record, _)| *record).collect::<Vec<_>>();
         manager.forget_v4_records(chain, &forgotten);
      });

      for (_, reason) in records {
         match reason {
            V4RemovalReason::Fee => report.removed_by_fee += 1,
            V4RemovalReason::BaseFee => report.removed_by_base_fee += 1,
            V4RemovalReason::NoBaseAsset => report.removed_no_base += 1,
            V4RemovalReason::Dead => report.removed_dead += 1,
            V4RemovalReason::Illiquid => report.removed_illiquid += 1,
         }
      }

      Ok(())
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

   /// Blocks per `eth_getLogs` during a V4 sync, adjustable from the pools window's settings.
   #[serde(default = "default_v4_sync_block_range")]
   pub v4_sync_block_range: u64,

   /// Fee ceiling of the V4 cleanup, as a percentage, adjustable from the pools window's settings.
   #[serde(default = "default_v4_max_fee_percent")]
   pub v4_max_fee_percent: f32,

   /// The stricter ceiling the cleanup applies to pools that pair a base asset.
   #[serde(default = "default_v4_max_base_fee_percent")]
   pub v4_max_base_fee_percent: f32,

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
   cold_v4: HashMap<(u64, Address), HashSet<B256>>,
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
         v4_sync_block_range: default_v4_sync_block_range(),
         v4_max_fee_percent: default_v4_max_fee_percent(),
         v4_max_base_fee_percent: default_v4_max_base_fee_percent(),
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
         v4_sync_block_range: default_v4_sync_block_range(),
         v4_max_fee_percent: default_v4_max_fee_percent(),
         v4_max_base_fee_percent: default_v4_max_base_fee_percent(),
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
         // A set, not a list: a step indexes thousands of records while the frame keeps reading the
         // manager under the same lock, and the insert must not scan the bucket it adds to.
         self.cold_v4.entry((chain, currency)).or_default().insert(id);
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

   /// Forget V4 records whose catalog rows are already gone: drop the pools they built and the cold
   /// index entries that still point at them.
   fn forget_v4_records(&mut self, chain: u64, records: &[V4PoolRecord]) {
      if records.is_empty() {
         return;
      }

      let forgotten = records.iter().map(|record| record.id()).collect::<HashSet<_>>();
      self
         .pools
         .retain(|key, pool| !(key.0 == chain && key.1.is_v4() && forgotten.contains(&pool.id())));

      for record in records {
         self.unindex_cold_v4(chain, record);
      }
   }

   /// Count this chain's V4 rows and approximate what they cost.
   ///
   /// The cold index keeps ids, not records -- the 68-byte row is read from the catalog on a hit --
   /// and every record is indexed under both of its currencies, so half the id count is the number
   /// of cold pools. A hot pool's token metadata is shared (`Arc<str>` behind `ERC20Token`), so the
   /// per-pool cost is the enum itself plus the key it hangs under, not its names.
   fn v4_catalog_stats(&self, chain: u64) -> V4CatalogStats {
      let mut stats = V4CatalogStats::default();

      for ((pool_chain, _), ids) in &self.cold_v4 {
         if *pool_chain != chain {
            continue;
         }

         stats.cold += ids.len();
         // A set slot holds the id plus its control byte, and the table keeps a few empty ones.
         stats.cold_bytes +=
            size_of::<((u64, Address), HashSet<B256>)>() + ids.capacity() * (size_of::<B256>() + 1);
      }
      stats.cold /= 2;

      for (key, pool) in &self.pools {
         if key.0 != chain || !matches!(pool, AnyUniswapPool::V4(_)) {
            continue;
         }

         stats.hot += 1;
         stats.hot_bytes += size_of::<AnyUniswapPool>() + size_of::<PoolKey>();
      }

      stats.total = stats.cold + stats.hot;
      stats
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
         v4_sync_block_range: self.v4_sync_block_range,
         v4_max_fee_percent: self.v4_max_fee_percent,
         v4_max_base_fee_percent: self.v4_max_base_fee_percent,
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

/// Why a V4 record is deleted. Kept per record so the log line can explain the total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum V4RemovalReason {
   Fee,
   BaseFee,
   /// Neither side is a base asset: no price feed, and no route worth keeping.
   NoBaseAsset,
   /// No in-range liquidity: nothing can be swapped through the pool.
   Dead,
   /// In-range liquidity, but the base side is worth less than the wallet's floor.
   Illiquid,
}

/// The split a cleanup applies, computed without touching the catalog or the network.
struct V4CleanupPlan {
   remove: Vec<(V4PoolRecord, V4RemovalReason)>,
   keep: Vec<V4PoolRecord>,
   dynamic: usize,
}

/// Split `records` into the pools to delete and the pools to keep, from the record alone.
///
/// A cold row is judged on its fee and its two addresses, so the bulk of a scam-laden catalog is
/// deleted without resolving a single currency. Liquidity is not part of this pass: it needs pool
/// state, which only a hot pool has.
fn plan_v4_cleanup(
   chain: u64,
   records: Vec<V4PoolRecord>,
   max_fee_percent: f32,
   max_base_fee_percent: f32,
) -> V4CleanupPlan {
   let mut plan = V4CleanupPlan {
      remove: Vec::new(),
      keep: Vec::with_capacity(records.len()),
      dynamic: 0,
   };

   for record in records {
      // A dynamic-fee pool has no fee of its own, so no ceiling can judge it.
      if record.fee == DYNAMIC_FEE_FLAG {
         plan.dynamic += 1;
         plan.keep.push(record);
         continue;
      }

      let percent = record.fee as f32 / 10_000.0;
      if percent > max_fee_percent {
         plan.remove.push((record, V4RemovalReason::Fee));
      } else if percent > max_base_fee_percent && record_has_base_currency(chain, &record) {
         plan.remove.push((record, V4RemovalReason::BaseFee));
      } else {
         plan.keep.push(record);
      }
   }

   plan
}

/// Whether either side is an asset every real deployment pairs against: native, or a base token
/// (WETH, WBNB, USDC, USDT, DAI, USDG).
fn record_has_base_currency(chain: u64, record: &V4PoolRecord) -> bool {
   [record.currency0, record.currency1]
      .into_iter()
      .any(|address| address.is_zero() || is_base_token(chain, address))
}

/// The base asset on a V4 record's side, as the ERC20 the wallet prices it by (native becomes its
/// wrapped token), plus whether that side is `currency0`.
fn base_side(chain: u64, record: &V4PoolRecord) -> Option<(ERC20Token, bool)> {
   for (address, is_currency0) in [(record.currency0, true), (record.currency1, false)] {
      if address.is_zero() {
         let native = NativeCurrency::from_chain_id(chain).ok()?;
         return Some((
            Currency::from(native).to_erc20().into_owned(),
            is_currency0,
         ));
      }

      if let Some(token) = ERC20Token::base_token(chain, address) {
         return Some((token, is_currency0));
      }
   }

   None
}

/// Why a pool's base side could not be weighed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum V4ValueGap {
   /// Neither side is a base asset, so the wallet knows a price for neither.
   NoBaseAsset,
   /// The base asset itself has no price in the wallet's price database.
   NoPrice,
   /// The state is unusable: liquidity without a sqrt price.
   BadState,
}

/// What rule 3 makes of one pool, from its state alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum V4LiquidityVerdict {
   Keep,
   Remove(V4RemovalReason),
   /// There is a base asset but no price for it: a missing feed is not zero liquidity.
   UnjudgedNoPrice,
   /// The state cannot be read as a price curve.
   UnjudgedBadState,
}

/// The verdict of rule 3 for one pool.
///
/// A pool with no in-range liquidity cannot fill a swap at any price, so no feed is needed to condemn
/// it; a pool with no base asset is a pair of unknowns, which is dead weight rather than a route.
/// Both are structural. Only the value test needs a price, and a pool whose price is missing is kept.
fn judge_v4_pool(
   chain: u64,
   record: &V4PoolRecord,
   liquidity: u128,
   sqrt_price: U256,
   price_of: impl Fn(&ERC20Token) -> f64,
) -> V4LiquidityVerdict {
   if liquidity == 0 {
      return V4LiquidityVerdict::Remove(V4RemovalReason::Dead);
   }

   match v4_base_value(chain, record, liquidity, sqrt_price, price_of) {
      Ok(value) if value < DEFAULT_POOL_MINIMUM_LIQUIDITY => {
         V4LiquidityVerdict::Remove(V4RemovalReason::Illiquid)
      }
      Ok(_) => V4LiquidityVerdict::Keep,
      Err(V4ValueGap::NoBaseAsset) => V4LiquidityVerdict::Remove(V4RemovalReason::NoBaseAsset),
      Err(V4ValueGap::NoPrice) => V4LiquidityVerdict::UnjudgedNoPrice,
      Err(V4ValueGap::BadState) => V4LiquidityVerdict::UnjudgedBadState,
   }
}

/// What the base side of a V4 pool is worth, from its state alone.
///
/// This is `ZeusCtx::pool_has_sufficient_liquidity` without the pool: the same base side, the same
/// decimals, the same price and the same floor, which is what lets a cold record be judged. The error
/// names what was missing rather than returning zero, because a missing feed is not zero liquidity
/// and a deletion is permanent.
fn v4_base_value(
   chain: u64,
   record: &V4PoolRecord,
   liquidity: u128,
   sqrt_price: U256,
   price_of: impl Fn(&ERC20Token) -> f64,
) -> Result<f64, V4ValueGap> {
   let (token, is_currency0) = base_side(chain, record).ok_or(V4ValueGap::NoBaseAsset)?;

   let price = price_of(&token);
   if price <= 0.0 {
      return Err(V4ValueGap::NoPrice);
   }

   let (amount0, amount1) =
      v4_virtual_reserves(liquidity, sqrt_price).map_err(|_| V4ValueGap::BadState)?;
   let amount = if is_currency0 { amount0 } else { amount1 };

   Ok(NumericValue::format_wei(amount, token.decimals).f64() * price)
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

   /// The sync span is user-editable, so it is clamped on the way in and on the way out: a
   /// hand-edited catalog or a stale slider must never reach `get_logs_for`.
   #[test]
   fn v4_sync_block_range_is_clamped_both_ways() {
      let handle = PoolManagerHandle::new(PoolManager::empty());

      assert_eq!(handle.v4_sync_block_range(), V4_SYNC_BLOCK_RANGE);

      handle.set_v4_sync_block_range(1);
      assert_eq!(
         handle.v4_sync_block_range(),
         MIN_V4_SYNC_BLOCK_RANGE
      );

      handle.set_v4_sync_block_range(u64::MAX);
      assert_eq!(
         handle.v4_sync_block_range(),
         MAX_V4_SYNC_BLOCK_RANGE
      );

      handle.set_v4_sync_block_range(12_345);
      assert_eq!(handle.v4_sync_block_range(), 12_345);

      // A stored value outside the range is clamped when the catalog is read back.
      handle.write(|manager| manager.v4_sync_block_range = 0);
      assert_eq!(
         handle.v4_sync_block_range(),
         MIN_V4_SYNC_BLOCK_RANGE
      );
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

   /// Both ceilings are user-editable, so they are clamped on the way in and on the way out: a
   /// hand-edited catalog must never turn into "delete the whole catalog".
   #[test]
   fn v4_cleanup_ceilings_are_clamped() {
      let handle = PoolManagerHandle::new(PoolManager::empty());

      assert_eq!(
         handle.v4_max_fee_percent(),
         DEFAULT_V4_MAX_FEE_PERCENT
      );
      assert_eq!(
         handle.v4_max_base_fee_percent(),
         DEFAULT_V4_MAX_BASE_FEE_PERCENT
      );

      handle.set_v4_max_fee_percent(0.0);
      assert_eq!(
         handle.v4_max_fee_percent(),
         DEFAULT_V4_MAX_FEE_PERCENT
      );

      handle.set_v4_max_fee_percent(99.0);
      assert_eq!(
         handle.v4_max_fee_percent(),
         MAX_V4_MAX_FEE_PERCENT
      );

      handle.set_v4_max_base_fee_percent(f32::NAN);
      assert_eq!(
         handle.v4_max_base_fee_percent(),
         DEFAULT_V4_MAX_BASE_FEE_PERCENT
      );

      handle.set_v4_max_base_fee_percent(0.5);
      assert_eq!(handle.v4_max_base_fee_percent(), 0.5);
   }

   /// `DYNAMIC_FEE_FLAG` is not a fee tier: read as one it is 838%, which would delete every
   /// dynamic-fee pool on the chain.
   #[test]
   fn cleanup_skips_dynamic_fee_pools() {
      let dynamic = V4PoolRecord {
         fee: DYNAMIC_FEE_FLAG,
         ..record_for(
            Address::repeat_byte(0x01),
            Address::repeat_byte(0x02),
         )
      };

      let plan = plan_v4_cleanup(1, vec![dynamic], 2.0, 1.0);

      assert_eq!(plan.dynamic, 1);
      assert!(plan.remove.is_empty());
      assert_eq!(plan.keep, vec![dynamic]);
   }

   #[test]
   fn cleanup_deletes_high_fee_and_base_asset_pools() {
      let weth = ERC20Token::weth().address;
      let far_a = Address::repeat_byte(0x11);
      let far_b = Address::repeat_byte(0x22);
      let with_fee = |currency0: Address, currency1: Address, fee: u32| V4PoolRecord {
         fee,
         ..record_for(currency0, currency1)
      };

      // 3% on an unknown pair: over the general ceiling.
      let high = with_fee(far_a, far_b, 30_000);
      // 1.5% is under 2% but over the base-asset ceiling, for a WETH pair and for native.
      let weth_pair = with_fee(weth, far_b, 15_000);
      let native_pair = with_fee(Address::ZERO, far_b, 15_000);
      // The same fee on a pair with no base asset is a legitimately obscure pool.
      let unknown_pair = with_fee(far_a, far_b, 15_000);
      // And a sane fee survives on a base pair.
      let ok = with_fee(weth, far_b, 3_000);
      // The ceiling is inclusive, like the snapshot builder's own 2% gate.
      let at_ceiling = with_fee(far_a, far_b, 20_000);

      let plan = plan_v4_cleanup(
         1,
         vec![high, weth_pair, native_pair, unknown_pair, ok, at_ceiling],
         2.0,
         1.0,
      );

      assert_eq!(
         plan.remove,
         vec![
            (high, V4RemovalReason::Fee),
            (weth_pair, V4RemovalReason::BaseFee),
            (native_pair, V4RemovalReason::BaseFee),
         ]
      );
      assert_eq!(plan.keep, vec![unknown_pair, ok, at_ceiling]);
   }

   /// A cold pool has no `Currency`, so the liquidity rule used to skip it entirely. The state is
   /// enough: the base side comes from the record, its decimals from the base token itself.
   #[test]
   fn cleanup_weighs_the_base_side_from_state_alone() {
      let weth = ERC20Token::weth();
      let other = Address::repeat_byte(0x44);
      let q96 = U256::from(1u128 << 96);
      let one_weth = 1_000_000_000_000_000_000u128;

      // One WETH at $3 000, from a pool whose other side this wallet cannot name.
      let record = record_for(weth.address, other);
      let value = v4_base_value(1, &record, one_weth, q96, |_| 3_000.0).expect("a price is known");
      assert!((value - 3_000.0).abs() < 1.0, "value was {value}");

      // Native is priced as its wrapped token, so it keeps the 18 decimals.
      let native = record_for(Address::ZERO, other);
      let value =
         v4_base_value(1, &native, one_weth, q96, |_| 3_000.0).expect("native is a base asset");
      assert!((value - 3_000.0).abs() < 1.0);

      // A pair with no base asset has nothing to weigh.
      let unknown = record_for(other, Address::repeat_byte(0x55));
      assert_eq!(
         v4_base_value(1, &unknown, one_weth, q96, |_| 3_000.0),
         Err(V4ValueGap::NoBaseAsset)
      );

      // A zero price is a missing feed, not zero liquidity.
      assert_eq!(
         v4_base_value(1, &record, one_weth, q96, |_| 0.0),
         Err(V4ValueGap::NoPrice)
      );
   }

   /// A pair of unknowns is not a route: with in-range liquidity but no base asset, the pool goes.
   #[test]
   fn cleanup_removes_pools_without_a_base_asset() {
      let q96 = U256::from(1u128 << 96);
      let unknown = record_for(
         Address::repeat_byte(0x11),
         Address::repeat_byte(0x22),
      );

      assert_eq!(
         judge_v4_pool(1, &unknown, 1_000_000, q96, |_| 3_000.0),
         V4LiquidityVerdict::Remove(V4RemovalReason::NoBaseAsset)
      );

      // Dead beats everything: a pool with no in-range liquidity cannot be swapped through at all.
      assert_eq!(
         judge_v4_pool(1, &unknown, 0, q96, |_| 3_000.0),
         V4LiquidityVerdict::Remove(V4RemovalReason::Dead)
      );
   }

   /// The value floor, on a pool whose base side the wallet can price.
   #[test]
   fn cleanup_judges_base_pairs_by_value() {
      let weth = ERC20Token::weth();
      let record = record_for(weth.address, Address::repeat_byte(0x44));
      let q96 = U256::from(1u128 << 96);

      // One WETH at $3 000 is under the $10 000 floor.
      assert_eq!(
         judge_v4_pool(1, &record, 1_000_000_000_000_000_000, q96, |_| {
            3_000.0
         }),
         V4LiquidityVerdict::Remove(V4RemovalReason::Illiquid)
      );

      // Ten WETH is over it.
      assert_eq!(
         judge_v4_pool(
            1,
            &record,
            10_000_000_000_000_000_000,
            q96,
            |_| 3_000.0
         ),
         V4LiquidityVerdict::Keep
      );

      // A base asset with no price is kept, never condemned.
      assert_eq!(
         judge_v4_pool(1, &record, 1_000_000_000_000_000_000, q96, |_| {
            0.0
         }),
         V4LiquidityVerdict::UnjudgedNoPrice
      );
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
         manager.v4_sync_block_range = 12_345;
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
      assert_eq!(loaded.v4_sync_block_range(), 12_345);
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

   #[test]
   fn forgetting_a_checkpoint_makes_the_next_sync_start_over() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = WalletStateKey::generate().unwrap();
      let store = PoolStore::open_at(&path, &key).unwrap();

      let handle = PoolManagerHandle::new(PoolManager::empty());
      handle.write(|manager| manager.store = Some(store.clone()));

      handle.add_checkpoint(
         1,
         DexKind::UniswapV4,
         Checkpoint::new(1, 5_000, DexKind::UniswapV4),
      );
      handle.add_checkpoint(
         1,
         DexKind::UniswapV2,
         Checkpoint::new(1, 4_000, DexKind::UniswapV2),
      );
      handle.save_to_store(&store).unwrap();
      assert_eq!(store.checkpoints().unwrap().len(), 2);

      handle.forget_v4_checkpoint(1).unwrap();

      assert!(
         handle.read(|manager| !manager.checkpoints.contains_key(&(1, DexKind::UniswapV4))),
         "the resume point is gone, so the next sync starts from the creation block"
      );
      assert!(
         store
            .checkpoints()
            .unwrap()
            .iter()
            .all(|(_, dex, _)| *dex != DexKind::UniswapV4),
         "and so is the catalog row, or a restart would resume from it"
      );
      assert_eq!(
         store.checkpoints().unwrap().len(),
         1,
         "another dex's checkpoint is untouched"
      );
   }

   #[test]
   fn catalog_stats_split_hot_from_cold() {
      let handle = PoolManagerHandle::new(PoolManager::empty());

      handle.add_pool(AnyUniswapPool::V4(
         UniswapV4Pool::new_with_spacing(
            1,
            FeeAmount::LOW,
            10,
            DexKind::UniswapV4,
            Currency::native(1),
            Currency::from(ERC20Token::weth()),
            State::none(),
            Address::ZERO,
         ),
      ));

      let cold = V4PoolRecord {
         currency0: ERC20Token::weth().address,
         currency1: Address::repeat_byte(0xab),
         fee: 3000,
         tick_spacing: 60,
         hooks: Address::ZERO,
      };
      handle.write(|manager| manager.index_cold_v4(1, &cold));

      let stats = handle.v4_catalog_stats(1);
      assert_eq!(stats.hot, 1);
      assert_eq!(
         stats.cold, 1,
         "a record sits under both of its currencies, so half the ids is the pool count"
      );
      assert_eq!(
         stats.total, 2,
         "a hot pool and a cold record are one row each"
      );
      assert!(stats.cold_bytes > 0 && stats.hot_bytes > 0);
      assert_eq!(
         handle.v4_catalog_stats(2).total,
         0,
         "another chain is not counted"
      );
   }

   #[test]
   fn the_catalog_revision_tracks_the_pool_set() {
      let handle = PoolManagerHandle::new(PoolManager::empty());
      let empty = handle.v4_revision(1);

      handle.add_pool(AnyUniswapPool::V4(
         UniswapV4Pool::new_with_spacing(
            1,
            FeeAmount::LOW,
            10,
            DexKind::UniswapV4,
            Currency::native(1),
            Currency::from(ERC20Token::weth()),
            State::none(),
            Address::ZERO,
         ),
      ));
      let hot = handle.v4_revision(1);
      assert_eq!(hot.0, empty.0 + 1, "a pool moves the count");

      let cold = V4PoolRecord {
         currency0: ERC20Token::weth().address,
         currency1: Address::repeat_byte(0xab),
         fee: 3000,
         tick_spacing: 60,
         hooks: Address::ZERO,
      };
      handle.write(|manager| manager.index_cold_v4(1, &cold));
      let indexed = handle.v4_revision(1);
      assert_eq!(
         indexed.0, hot.0,
         "the cold side does not move the pool count"
      );
      assert_eq!(
         indexed.1,
         empty.1 + 2,
         "a cold record is indexed under both of its currencies"
      );

      handle.add_checkpoint(
         1,
         DexKind::UniswapV4,
         Checkpoint::new(1, 7, DexKind::UniswapV4),
      );
      assert_eq!(
         handle.v4_revision(1).2,
         Some(7),
         "a sync step moves the checkpoint, so a cold-only step is not missed"
      );
      assert_eq!(
         handle.v4_revision(2).1,
         0,
         "another chain has no cold records here"
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
      assert_eq!(remaining, HashSet::from([record_b.id()]));
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
