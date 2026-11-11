//! redb store for the Uniswap pool catalog (`data/pool_data.data`).
//!
//! The catalog used to be one sealed JSON blob, so every write rewrote everything and the whole
//! thing had to be in memory to be used. It is a redb database now: one row per pool, one row per
//! checkpoint, one row per discovery timestamp. Values keep the old sealing (XChaCha20-Poly1305 over
//! compressed JSON, via `WalletStateKey::seal_json`) with an AAD per record kind.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::{anyhow, bail};
use redb::{Database as RedbInner, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use zeus_eth::{
   alloy_primitives::{Address, B256},
   amm::uniswap::{AnyUniswapPool, DexKind, FeeAmount, UniswapPool, UniswapV4Pool, v4_pool_id},
};

use crate::context::pool_manager::{
   DEFAULT_V4_MAX_BASE_FEE_PERCENT, DEFAULT_V4_MAX_FEE_PERCENT, V4_SYNC_BLOCK_RANGE,
};
use crate::persisted::{PersistedFile, ensure_data_dir, file_path};
use crate::platform::fs::restrict_file_to_owner;
use crate::wallet_state_key::WalletStateKey;

const POOL_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("pool");

/// redb's default page cache is 1 GiB; the pool store reads one row at a time.
const PAGE_CACHE_BYTES: usize = 1024 * 1024;

/// On-disk format. Bump when a key or value envelope changes; a newer file is refused.
pub(crate) const POOL_DB_FORMAT: u32 = 1;

const AAD_META: &[u8] = b"zeus-pool-data-v2/meta";
const AAD_CHECKPOINT: &[u8] = b"zeus-pool-data-v2/checkpoint";
const AAD_POOL: &[u8] = b"zeus-pool-data-v2/pool";
const AAD_V4: &[u8] = b"zeus-pool-data-v2/v4";
const AAD_DISCOVER: &[u8] = b"zeus-pool-data-v2/discover";

fn default_v4_sync_block_range() -> u64 {
   V4_SYNC_BLOCK_RANGE
}

fn default_v4_max_fee_percent() -> f32 {
   DEFAULT_V4_MAX_FEE_PERCENT
}

fn default_v4_max_base_fee_percent() -> f32 {
   DEFAULT_V4_MAX_BASE_FEE_PERCENT
}

/// Everything in `PoolManager` that is not a pool. Small enough to rewrite wholesale.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct PoolDbMeta {
   pub concurrency: usize,
   pub batch_size_for_updating_pool_state: usize,
   pub batch_size_for_discovering_pools: usize,
   pub discover_v4_pools: bool,
   pub ignore_chains: HashSet<u64>,
   /// Blocks per `eth_getLogs` during a V4 sync; the pools window's settings slider writes this.
   #[serde(default = "default_v4_sync_block_range")]
   pub v4_sync_block_range: u64,
   /// Fee ceiling of the V4 cleanup, as a percentage; the settings slider writes this.
   #[serde(default = "default_v4_max_fee_percent")]
   pub v4_max_fee_percent: f32,
   /// The stricter ceiling the cleanup applies to pools that pair a base asset.
   #[serde(default = "default_v4_max_base_fee_percent")]
   pub v4_max_base_fee_percent: f32,
   /// Bumped when the layout changes; a file with a newer value is refused on open.
   #[serde(default)]
   pub format: u32,
}

/// The key parameters of a V4 pool: everything needed to rebuild it once both currencies are known.
/// Deliberately holds no `Currency`, so a dead pool costs ~68 bytes instead of a full token record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct V4PoolRecord {
   pub currency0: Address,
   pub currency1: Address,
   pub fee: u32,
   pub tick_spacing: i32,
   pub hooks: Address,
}

impl V4PoolRecord {
   pub fn from_pool(pool: &UniswapV4Pool) -> Self {
      Self {
         currency0: pool.currency0().address(),
         currency1: pool.currency1().address(),
         fee: pool.fee().fee(),
         tick_spacing: pool.tick_spacing_i32(),
         hooks: pool.hooks(),
      }
   }

   pub fn id(&self) -> B256 {
      v4_pool_id(
         self.currency0,
         self.currency1,
         FeeAmount::new(self.fee),
         self.tick_spacing,
         self.hooks,
      )
   }
}

/// The sealed pool catalog: one redb row per pool, checkpoint and discovery timestamp.
///
/// The rows are:
///
/// | key | value |
/// |---|---|
/// | `m` | a `PoolDbMeta` |
/// | `c` + chain(8) + dex(1) | `u64` last synced block |
/// | `p` + chain(8) + address(20) + dex(1) | an `AnyUniswapPool` (V2/V3, full) |
/// | `v` + chain(8) + pool_id(32) | a `V4PoolRecord` |
/// | `l` + chain(8) + token_a(20) + token_b(20) | `u64` discovery timestamp |
/// | `d` + chain(8) + dex(1) | `u64` V4 discovery timestamp |
///
/// Integers are big-endian so a chain's rows sort together: a chain scan is a range, and a sync step
/// commits a handful of rows instead of rewriting the catalog.
pub(crate) struct PoolStore {
   inner: Arc<RwLock<RedbInner>>,
   key: SharedKey,
}

/// The sealing key of a [`PoolStore`].
///
/// `WalletStateKey` is `Send` but not `Sync`, and this store is held inside `PoolManager`, which
/// sits behind an `RwLock` that needs `Sync`. The key is only read under this mutex, so the
/// call sites below are unchanged.
#[derive(Debug)]
struct SharedKey(Mutex<WalletStateKey>);

impl SharedKey {
   fn seal_json<T: Serialize>(&self, value: &T, aad: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
      self.lock().seal_json(value, aad)
   }

   fn open_json<T: DeserializeOwned>(&self, sealed: &[u8], aad: &[u8]) -> Result<T, anyhow::Error> {
      self.lock().open_json(sealed, aad)
   }

   fn lock(&self) -> MutexGuard<'_, WalletStateKey> {
      self.0.lock().expect("the pool store key lock was poisoned")
   }
}

impl Clone for SharedKey {
   fn clone(&self) -> Self {
      Self(Mutex::new(self.lock().clone()))
   }
}

impl Clone for PoolStore {
   fn clone(&self) -> Self {
      Self {
         inner: Arc::clone(&self.inner),
         key: self.key.clone(),
      }
   }
}

impl PoolStore {
   /// `data/pool_data.data`, created if missing.
   pub fn open(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      ensure_data_dir()?;
      Self::open_at(&Self::path()?, key)
   }

   pub fn open_at(path: &Path, key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      if let Some(parent) = path.parent() {
         std::fs::create_dir_all(parent)?;
      }

      let inner = RedbInner::builder()
         .set_cache_size(PAGE_CACHE_BYTES)
         .create(path)
         .map_err(|e| anyhow!("open pool store: {e}"))?;

      if let Err(e) = restrict_file_to_owner(path) {
         tracing::warn!("Failed to restrict {}: {e}", path.display());
      }

      let tx = inner.begin_write().map_err(|e| anyhow!("pool store write txn: {e}"))?;
      {
         let _ = tx.open_table(POOL_TABLE).map_err(|e| anyhow!("pool store open table: {e}"))?;
      }
      tx.commit().map_err(|e| anyhow!("pool store commit: {e}"))?;

      let store = Self {
         inner: Arc::new(RwLock::new(inner)),
         key: SharedKey(Mutex::new(key.clone())),
      };

      if let Some(meta) = store.load_meta()? {
         if meta.format > POOL_DB_FORMAT {
            bail!(
               "pool store format {} is newer than this build ({})",
               meta.format,
               POOL_DB_FORMAT
            );
         }
      }

      Ok(store)
   }

   /// Path of the catalog: `data/pool_data.data` (the same file `persisted::pool_data_dir` returns).
   pub fn path() -> Result<PathBuf, anyhow::Error> {
      file_path(PersistedFile::PoolData)
   }

   pub fn load_meta(&self) -> Result<Option<PoolDbMeta>, anyhow::Error> {
      let guard = self.read_guard()?;
      let tx = guard.begin_read().map_err(|e| anyhow!("pool store read txn: {e}"))?;
      let table = tx.open_table(POOL_TABLE).map_err(|e| anyhow!("pool store open table: {e}"))?;

      let Some(sealed) = table.get(meta_key()).map_err(|e| anyhow!("pool store get meta: {e}"))?
      else {
         return Ok(None);
      };

      let meta = self.key.open_json(sealed.value(), AAD_META)?;
      Ok(Some(meta))
   }

   /// Rewrite the meta row.
   ///
   /// A `format` of `0` is stamped with the current `POOL_DB_FORMAT`; an explicit value is written
   /// verbatim, so a test can put a newer file on disk and check that it is refused.
   pub fn save_meta(&self, meta: &PoolDbMeta) -> Result<(), anyhow::Error> {
      let mut meta = meta.clone();
      if meta.format == 0 {
         meta.format = POOL_DB_FORMAT;
      }

      let sealed = self.key.seal_json(&meta, AAD_META)?;
      self.insert_rows(&[(meta_key().to_vec(), sealed)])
   }

   pub fn checkpoints(&self) -> Result<Vec<(u64, DexKind, u64)>, anyhow::Error> {
      self.scan_map(None, b'c', |key, value| {
         let Some(chain) = key_chain(key) else {
            return Ok(None);
         };
         let Some(dex) = dex_at(key, 9) else {
            return Ok(None);
         };
         let block: u64 = self.key.open_json(value, AAD_CHECKPOINT)?;
         Ok(Some((chain, dex, block)))
      })
   }

   pub fn put_checkpoint(&self, chain: u64, dex: DexKind, block: u64) -> Result<(), anyhow::Error> {
      let sealed = self.key.seal_json(&block, AAD_CHECKPOINT)?;
      self.insert_rows(&[(checkpoint_key(chain, dex), sealed)])
   }

   pub fn load_v2_v3_pools(&self) -> Result<Vec<AnyUniswapPool>, anyhow::Error> {
      self.scan_map(None, b'p', |key, value| {
         if key.len() != 30 || dex_at(key, 29).is_none() {
            return Ok(None);
         }
         let pool: AnyUniswapPool = self.key.open_json(value, AAD_POOL)?;
         Ok(Some(pool))
      })
   }

   pub fn put_pool(&self, pool: &AnyUniswapPool) -> Result<(), anyhow::Error> {
      if matches!(pool, AnyUniswapPool::V4(_)) {
         bail!("pool store: a v4 pool has no address; store its V4PoolRecord instead");
      }

      let sealed = self.key.seal_json(pool, AAD_POOL)?;
      let key = pool_key(pool.chain_id(), pool.address(), pool.dex_kind());
      self.insert_rows(&[(key, sealed)])
   }

   /// Write one V4 record on its own, sealed and with the same durability as every other row.
   pub fn put_v4_record(&self, chain: u64, record: &V4PoolRecord) -> Result<(), anyhow::Error> {
      let sealed = self.key.seal_json(record, AAD_V4)?;
      self.insert_rows(&[(v4_key(chain, record.id()), sealed)])
   }

   /// Delete V4 records in one transaction.
   ///
   /// A cleanup deletes a chunk at a time so its progress is durable without a commit per row.
   pub fn remove_v4_records(&self, chain: u64, ids: &[B256]) -> Result<(), anyhow::Error> {
      let keys = ids.iter().map(|id| v4_key(chain, *id)).collect::<Vec<_>>();
      self.remove_rows(&keys)
   }

   /// Test surface only: nothing deletes a pool yet (a token-prune path would be the first caller).
   #[cfg(test)]
   pub fn remove_pool(
      &self,
      chain: u64,
      address: Address,
      dex: DexKind,
   ) -> Result<(), anyhow::Error> {
      self.remove_row(&pool_key(chain, address, dex))
   }

   pub fn load_v4_records(
      &self,
      chain: Option<u64>,
   ) -> Result<Vec<(u64, V4PoolRecord)>, anyhow::Error> {
      self.scan_map(chain, b'v', |key, value| {
         if key.len() != 41 {
            return Ok(None);
         }
         let Some(chain) = key_chain(key) else {
            return Ok(None);
         };
         let record: V4PoolRecord = self.key.open_json(value, AAD_V4)?;
         Ok(Some((chain, record)))
      })
   }

   /// Read one cold V4 record without decoding the rest of the catalog.
   pub fn v4_record(&self, chain: u64, id: B256) -> Result<Option<V4PoolRecord>, anyhow::Error> {
      let guard = self.read_guard()?;
      let tx = guard.begin_read().map_err(|e| anyhow!("pool store read txn: {e}"))?;
      let table = tx.open_table(POOL_TABLE).map_err(|e| anyhow!("pool store open table: {e}"))?;

      let Some(sealed) = table
         .get(v4_key(chain, id).as_slice())
         .map_err(|e| anyhow!("pool store get v4 record: {e}"))?
      else {
         return Ok(None);
      };

      let record = self.key.open_json(sealed.value(), AAD_V4)?;
      Ok(Some(record))
   }

   pub fn has_v4(&self, chain: u64, id: B256) -> Result<bool, anyhow::Error> {
      let guard = self.read_guard()?;
      let tx = guard.begin_read().map_err(|e| anyhow!("pool store read txn: {e}"))?;
      let table = tx.open_table(POOL_TABLE).map_err(|e| anyhow!("pool store open table: {e}"))?;

      let row = table
         .get(v4_key(chain, id).as_slice())
         .map_err(|e| anyhow!("pool store get v4 record: {e}"))?;
      Ok(row.is_some())
   }

   /// Write the new V4 records AND the new checkpoint in ONE write transaction, so a crash can never
   /// advance the checkpoint past stored pools.
   pub fn commit_v4_step(
      &self,
      chain: u64,
      records: &[V4PoolRecord],
      checkpoint_block: u64,
   ) -> Result<(), anyhow::Error> {
      let mut rows = Vec::with_capacity(records.len() + 1);
      for record in records {
         rows.push((
            v4_key(chain, record.id()),
            self.key.seal_json(record, AAD_V4)?,
         ));
      }
      rows.push((
         checkpoint_key(chain, DexKind::UniswapV4),
         self.key.seal_json(&checkpoint_block, AAD_CHECKPOINT)?,
      ));

      self.insert_rows(&rows)
   }

   pub fn discovered(&self) -> Result<Vec<((u64, Address, Address), u64)>, anyhow::Error> {
      self.scan_map(None, b'l', |key, value| {
         if key.len() != 49 {
            return Ok(None);
         }
         let (Some(chain), Some(a), Some(b)) = (
            key_chain(key),
            key_address(key, 9),
            key_address(key, 29),
         ) else {
            return Ok(None);
         };
         let ts: u64 = self.key.open_json(value, AAD_DISCOVER)?;
         Ok(Some(((chain, a, b), ts)))
      })
   }

   pub fn put_discovered(
      &self,
      chain: u64,
      a: Address,
      b: Address,
      ts: u64,
   ) -> Result<(), anyhow::Error> {
      let sealed = self.key.seal_json(&ts, AAD_DISCOVER)?;
      self.insert_rows(&[(discovery_key(chain, a, b), sealed)])
   }

   pub fn discovered_v4(&self) -> Result<Vec<((u64, DexKind), u64)>, anyhow::Error> {
      self.scan_map(None, b'd', |key, value| {
         if key.len() != 10 {
            return Ok(None);
         }
         let (Some(chain), Some(dex)) = (key_chain(key), dex_at(key, 9)) else {
            return Ok(None);
         };
         let ts: u64 = self.key.open_json(value, AAD_DISCOVER)?;
         Ok(Some(((chain, dex), ts)))
      })
   }

   pub fn put_discovered_v4(&self, chain: u64, dex: DexKind, ts: u64) -> Result<(), anyhow::Error> {
      let sealed = self.key.seal_json(&ts, AAD_DISCOVER)?;
      self.insert_rows(&[(v4_discovery_key(chain, dex), sealed)])
   }

   /// One transaction that writes a whole catalog: used once, to import a legacy sealed blob or to
   /// seed a fresh db from the embedded snapshot.
   ///
   /// Rows are written by key, so an existing row with the same key is replaced and the table is not
   /// cleared. `v4` carries the chain of each record: a cold record has no chain of its own.
   pub fn write_catalog(
      &self,
      meta: &PoolDbMeta,
      pools: &[AnyUniswapPool],
      v4: &[(u64, V4PoolRecord)],
      checkpoints: &[(u64, DexKind, u64)],
      discovered: &[((u64, Address, Address), u64)],
      discovered_v4: &[((u64, DexKind), u64)],
   ) -> Result<(), anyhow::Error> {
      let mut rows = Vec::with_capacity(
         1 + pools.len() + v4.len() + checkpoints.len() + discovered.len() + discovered_v4.len(),
      );

      let mut meta = meta.clone();
      if meta.format == 0 {
         meta.format = POOL_DB_FORMAT;
      }
      rows.push((
         meta_key().to_vec(),
         self.key.seal_json(&meta, AAD_META)?,
      ));

      for pool in pools {
         if matches!(pool, AnyUniswapPool::V4(_)) {
            bail!("pool store: a v4 pool has no address; pass its V4PoolRecord instead");
         }
         rows.push((
            pool_key(pool.chain_id(), pool.address(), pool.dex_kind()),
            self.key.seal_json(pool, AAD_POOL)?,
         ));
      }

      for (chain, record) in v4 {
         rows.push((
            v4_key(*chain, record.id()),
            self.key.seal_json(record, AAD_V4)?,
         ));
      }

      for (chain, dex, block) in checkpoints {
         rows.push((
            checkpoint_key(*chain, *dex),
            self.key.seal_json(block, AAD_CHECKPOINT)?,
         ));
      }

      for ((chain, a, b), ts) in discovered {
         rows.push((
            discovery_key(*chain, *a, *b),
            self.key.seal_json(ts, AAD_DISCOVER)?,
         ));
      }

      for ((chain, dex), ts) in discovered_v4 {
         rows.push((
            v4_discovery_key(*chain, *dex),
            self.key.seal_json(ts, AAD_DISCOVER)?,
         ));
      }

      self.insert_rows(&rows)
   }

   /// `true` when the file actually shrank. Blocking; call on shutdown only.
   pub fn compact(&self) -> Result<bool, anyhow::Error> {
      let mut guard = self.write_guard()?;
      guard.compact().map_err(|e| anyhow!("pool store compact: {e}"))
   }

   fn read_guard(&self) -> Result<RwLockReadGuard<'_, RedbInner>, anyhow::Error> {
      self.inner.read().map_err(|e| anyhow!("pool store lock: {e}"))
   }

   fn write_guard(&self) -> Result<RwLockWriteGuard<'_, RedbInner>, anyhow::Error> {
      self.inner.write().map_err(|e| anyhow!("pool store lock: {e}"))
   }

   /// Write every `(key, value)` row in one immediate-durability transaction, so a crash leaves either
   /// all of them or none.
   fn insert_rows(&self, rows: &[(Vec<u8>, Vec<u8>)]) -> Result<(), anyhow::Error> {
      if rows.is_empty() {
         return Ok(());
      }

      let guard = self.write_guard()?;
      let mut tx = guard.begin_write().map_err(|e| anyhow!("pool store write txn: {e}"))?;
      tx.set_durability(Durability::Immediate)
         .map_err(|e| anyhow!("pool store durability: {e}"))?;
      {
         let mut table =
            tx.open_table(POOL_TABLE).map_err(|e| anyhow!("pool store open table: {e}"))?;
         for (key, value) in rows {
            table
               .insert(key.as_slice(), value.as_slice())
               .map_err(|e| anyhow!("pool store insert: {e}"))?;
         }
      }
      tx.commit().map_err(|e| anyhow!("pool store commit: {e}"))
   }

   #[cfg(test)]
   fn remove_row(&self, key: &[u8]) -> Result<(), anyhow::Error> {
      self.remove_rows(&[key.to_vec()])
   }

   /// Delete every key in one immediate-durability transaction, so a crash leaves all of them gone or
   /// none.
   fn remove_rows(&self, keys: &[Vec<u8>]) -> Result<(), anyhow::Error> {
      if keys.is_empty() {
         return Ok(());
      }

      let guard = self.write_guard()?;
      let mut tx = guard.begin_write().map_err(|e| anyhow!("pool store write txn: {e}"))?;
      tx.set_durability(Durability::Immediate)
         .map_err(|e| anyhow!("pool store durability: {e}"))?;
      {
         let mut table =
            tx.open_table(POOL_TABLE).map_err(|e| anyhow!("pool store open table: {e}"))?;
         for key in keys {
            table.remove(key.as_slice()).map_err(|e| anyhow!("pool store remove: {e}"))?;
         }
      }
      tx.commit().map_err(|e| anyhow!("pool store commit: {e}"))
   }

   /// Every row under `prefix` for one chain, or for every chain when `chain` is `None`.
   ///
   /// A chain scan is a range over `prefix` + chain .. `prefix` + chain + 1. The all-chains scan
   /// filters `iter()` on the prefix byte, because the byte after `c` already starts another kind's
   /// keys. Values are decoded inside the read transaction, so only decoded records stay in memory.
   fn scan_map<T>(
      &self,
      chain: Option<u64>,
      prefix: u8,
      decode: impl Fn(&[u8], &[u8]) -> Result<Option<T>, anyhow::Error>,
   ) -> Result<Vec<T>, anyhow::Error> {
      let guard = self.read_guard()?;
      let tx = guard.begin_read().map_err(|e| anyhow!("pool store read txn: {e}"))?;
      let table = tx.open_table(POOL_TABLE).map_err(|e| anyhow!("pool store open table: {e}"))?;

      let mut out = Vec::new();
      match chain {
         Some(chain) => {
            let (start, end) = chain_range(prefix, chain);
            let rows = table
               .range(start.as_slice()..end.as_slice())
               .map_err(|e| anyhow!("pool store range: {e}"))?;
            for row in rows {
               let (key, value) = row.map_err(|e| anyhow!("pool store row: {e}"))?;
               if let Some(decoded) = decode(key.value(), value.value())? {
                  out.push(decoded);
               }
            }
         }
         None => {
            let rows = table.iter().map_err(|e| anyhow!("pool store iter: {e}"))?;
            for row in rows {
               let (key, value) = row.map_err(|e| anyhow!("pool store row: {e}"))?;
               if key.value().first() != Some(&prefix) {
                  continue;
               }
               if let Some(decoded) = decode(key.value(), value.value())? {
                  out.push(decoded);
               }
            }
         }
      }

      Ok(out)
   }
}

/// `m`: the `PoolDbMeta` row.
fn meta_key() -> &'static [u8] {
   b"m"
}

/// `c` + chain(8) + dex(1): the last synced block.
fn checkpoint_key(chain: u64, dex: DexKind) -> Vec<u8> {
   let mut key = Vec::with_capacity(10);
   key.push(b'c');
   key.extend_from_slice(&chain.to_be_bytes());
   key.push(dex_byte(dex));
   key
}

/// `p` + chain(8) + address(20) + dex(1): one V2/V3 pool.
fn pool_key(chain: u64, address: Address, dex: DexKind) -> Vec<u8> {
   let mut key = Vec::with_capacity(30);
   key.push(b'p');
   key.extend_from_slice(&chain.to_be_bytes());
   key.extend_from_slice(address.as_slice());
   key.push(dex_byte(dex));
   key
}

/// `v` + chain(8) + pool id(32): one cold V4 record.
fn v4_key(chain: u64, id: B256) -> Vec<u8> {
   let mut key = Vec::with_capacity(41);
   key.push(b'v');
   key.extend_from_slice(&chain.to_be_bytes());
   key.extend_from_slice(id.as_slice());
   key
}

/// `l` + chain(8) + token a(20) + token b(20): the pair's last discovery timestamp.
fn discovery_key(chain: u64, a: Address, b: Address) -> Vec<u8> {
   let mut key = Vec::with_capacity(49);
   key.push(b'l');
   key.extend_from_slice(&chain.to_be_bytes());
   key.extend_from_slice(a.as_slice());
   key.extend_from_slice(b.as_slice());
   key
}

/// `d` + chain(8) + dex(1): the V4 discovery timestamp.
fn v4_discovery_key(chain: u64, dex: DexKind) -> Vec<u8> {
   let mut key = Vec::with_capacity(10);
   key.push(b'd');
   key.extend_from_slice(&chain.to_be_bytes());
   key.push(dex_byte(dex));
   key
}

/// Bounds of every row of `chain`: the chain's big-endian bytes up to the next chain's.
fn chain_range(prefix: u8, chain: u64) -> (Vec<u8>, Vec<u8>) {
   let mut start = Vec::with_capacity(9);
   start.push(prefix);
   start.extend_from_slice(&chain.to_be_bytes());

   let mut end = Vec::with_capacity(9);
   end.push(prefix);
   end.extend_from_slice(&chain.saturating_add(1).to_be_bytes());

   (start, end)
}

/// `DexKind` has no stable discriminant of its own, so the on-disk byte lives here; both directions
/// are in this file and nowhere else.
fn dex_byte(dex: DexKind) -> u8 {
   match dex {
      DexKind::UniswapV2 => 0,
      DexKind::UniswapV3 => 1,
      DexKind::UniswapV4 => 2,
      DexKind::PancakeSwapV2 => 3,
      DexKind::PancakeSwapV3 => 4,
   }
}

/// The inverse of `dex_byte`. An unknown byte is `None`, so a row written by a newer build is
/// skipped instead of panicking.
fn dex_from_byte(byte: u8) -> Option<DexKind> {
   match byte {
      0 => Some(DexKind::UniswapV2),
      1 => Some(DexKind::UniswapV3),
      2 => Some(DexKind::UniswapV4),
      3 => Some(DexKind::PancakeSwapV2),
      4 => Some(DexKind::PancakeSwapV3),
      _ => None,
   }
}

fn dex_at(key: &[u8], offset: usize) -> Option<DexKind> {
   key.get(offset).copied().and_then(dex_from_byte)
}

fn key_chain(key: &[u8]) -> Option<u64> {
   let bytes: [u8; 8] = key.get(1..9)?.try_into().ok()?;
   Some(u64::from_be_bytes(bytes))
}

fn key_address(key: &[u8], offset: usize) -> Option<Address> {
   let bytes = key.get(offset..offset + 20)?;
   Some(Address::from_slice(bytes))
}

#[cfg(test)]
mod tests {
   use std::time::Instant;

   use tempfile::tempdir;
   use zeus_eth::{
      alloy_primitives::address,
      amm::uniswap::{State, UniswapV2Pool, UniswapV3Pool},
      currency::{Currency, ERC20Token},
   };

   use super::*;

   fn new_key() -> WalletStateKey {
      WalletStateKey::generate().unwrap()
   }

   fn record(fee: u32, tick_spacing: i32) -> V4PoolRecord {
      V4PoolRecord {
         currency0: Address::ZERO,
         currency1: ERC20Token::weth().address,
         fee,
         tick_spacing,
         hooks: Address::ZERO,
      }
   }

   #[test]
   fn v4_record_roundtrip_and_id_is_stable() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = new_key();

      let pool = UniswapV4Pool::new_with_spacing(
         1,
         FeeAmount::LOW,
         10,
         DexKind::UniswapV4,
         Currency::native(1),
         Currency::from(ERC20Token::weth()),
         State::none(),
         Address::ZERO,
      );
      let written = V4PoolRecord::from_pool(&pool);

      {
         let store = PoolStore::open_at(&path, &key).unwrap();
         store.commit_v4_step(1, &[written], 100).unwrap();
      }

      let store = PoolStore::open_at(&path, &key).unwrap();
      let stored = store.v4_record(1, written.id()).unwrap().expect("record survives reopen");

      assert_eq!(stored, written);
      assert_eq!(
         stored.id(),
         pool.id(),
         "a cold record must rebuild the same pool id"
      );
      assert!(store.has_v4(1, written.id()).unwrap());
      assert!(!store.has_v4(2, written.id()).unwrap());
      assert_eq!(
         store.load_v4_records(Some(1)).unwrap(),
         vec![(1, written)]
      );
      assert!(store.load_v4_records(Some(2)).unwrap().is_empty());
      assert_eq!(
         store.load_v4_records(None).unwrap(),
         vec![(1, written)]
      );
   }

   #[test]
   fn checkpoint_and_cold_rows_commit_together() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = new_key();
      let records = [record(500, 10), record(3000, 60)];

      {
         let store = PoolStore::open_at(&path, &key).unwrap();
         store.save_meta(&PoolDbMeta::default()).unwrap();
         store.commit_v4_step(8453, &records, 1_234).unwrap();
      }

      let store = PoolStore::open_at(&path, &key).unwrap();
      let stored = store.load_v4_records(Some(8453)).unwrap();
      assert_eq!(stored.len(), 2);
      assert!(stored.iter().all(|(chain, _)| *chain == 8453));
      assert_eq!(
         store.checkpoints().unwrap(),
         vec![(8453, DexKind::UniswapV4, 1_234)],
         "the checkpoint commits with the cold rows"
      );
      assert_eq!(
         store.load_meta().unwrap().expect("meta row").format,
         POOL_DB_FORMAT
      );
   }

   #[test]
   fn v4_records_can_be_deleted_for_good() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = new_key();
      let records = [record(500, 10), record(3000, 60)];

      let store = PoolStore::open_at(&path, &key).unwrap();
      store.commit_v4_step(1, &records, 100).unwrap();

      store.remove_v4_records(1, &[records[0].id()]).unwrap();

      assert!(
         !store.has_v4(1, records[0].id()).unwrap(),
         "a deleted record is gone, not tombstoned"
      );
      assert_eq!(
         store.load_v4_records(Some(1)).unwrap(),
         vec![(1, records[1])],
         "the rest of the chain is untouched"
      );

      // An empty chunk is a no-op, which is what a cleanup with nothing to delete passes.
      store.remove_v4_records(1, &[]).unwrap();
      assert_eq!(store.load_v4_records(Some(1)).unwrap().len(), 1);
   }

   #[test]
   fn v2_v3_pools_roundtrip() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = new_key();

      let v2 = AnyUniswapPool::V2(UniswapV2Pool::weth_uni());
      let v3 = AnyUniswapPool::V3(UniswapV3Pool::new(
         1,
         address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640"),
         3000,
         ERC20Token::weth(),
         ERC20Token::usdc(),
         DexKind::UniswapV3,
      ));

      {
         let store = PoolStore::open_at(&path, &key).unwrap();
         store.put_pool(&v2).unwrap();
         store.put_pool(&v3).unwrap();
         store.put_checkpoint(1, DexKind::UniswapV2, 42).unwrap();
      }

      let store = PoolStore::open_at(&path, &key).unwrap();
      let pools = store.load_v2_v3_pools().unwrap();
      let found: Vec<(u64, Address, DexKind)> = pools
         .iter()
         .map(|pool| (pool.chain_id(), pool.address(), pool.dex_kind()))
         .collect();

      assert_eq!(pools.len(), 2);
      assert!(found.contains(&(1, v2.address(), DexKind::UniswapV2)));
      assert!(found.contains(&(1, v3.address(), DexKind::UniswapV3)));

      store.remove_pool(1, v2.address(), DexKind::UniswapV2).unwrap();
      let left = store.load_v2_v3_pools().unwrap();
      assert_eq!(left.len(), 1);
      assert_eq!(left[0].address(), v3.address());
   }

   #[test]
   fn newer_format_is_refused() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = new_key();

      {
         let store = PoolStore::open_at(&path, &key).unwrap();
         let newer = PoolDbMeta {
            format: POOL_DB_FORMAT + 1,
            ..Default::default()
         };
         store.save_meta(&newer).unwrap();
      }

      let err = match PoolStore::open_at(&path, &key) {
         Ok(_) => panic!("a file with a newer format must be refused"),
         Err(e) => e,
      };
      assert!(
         err.to_string().contains("is newer than this build"),
         "unexpected error: {err}"
      );
   }

   #[test]
   fn compact_preserves_records() {
      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = new_key();

      let records = [(1, record(100, 1)), (1, record(500, 10))];
      let a = ERC20Token::weth().address;
      let b = ERC20Token::usdc().address;

      let store = PoolStore::open_at(&path, &key).unwrap();
      store
         .write_catalog(
            &PoolDbMeta {
               concurrency: 3,
               ..Default::default()
            },
            &[AnyUniswapPool::V2(UniswapV2Pool::weth_uni())],
            &records,
            &[(1, DexKind::UniswapV2, 7)],
            &[((1, a, b), 11)],
            &[((1, DexKind::UniswapV4), 12)],
         )
         .unwrap();
      store.compact().unwrap();
      drop(store);

      let store = PoolStore::open_at(&path, &key).unwrap();
      let mut stored = store.load_v4_records(Some(1)).unwrap();
      stored.sort_by_key(|(_, record)| record.id());
      let mut expected = vec![records[0], records[1]];
      expected.sort_by_key(|(_, record)| record.id());

      assert_eq!(stored, expected);
      assert_eq!(
         store.checkpoints().unwrap(),
         vec![(1, DexKind::UniswapV2, 7)]
      );
      assert_eq!(store.discovered().unwrap(), vec![((1, a, b), 11)]);
      assert_eq!(
         store.discovered_v4().unwrap(),
         vec![((1, DexKind::UniswapV4), 12)]
      );
      assert_eq!(store.load_v2_v3_pools().unwrap().len(), 1);
      assert_eq!(store.load_meta().unwrap().unwrap().concurrency, 3);
   }

   #[test]
   fn cold_store_size_for_50k_v4_records() {
      // 50 000 records measured 336 B/pool in a debug build (45 s), so the committed run uses
      // 10 000. redb grows its file in doubling regions, so the raw file of a 10 000-row catalog
      // still carries a half-empty region; the size is measured after `compact()`, which reclaims it.
      const RECORDS: u64 = 10_000;

      let dir = tempdir().unwrap();
      let path = dir.path().join("pool_data.data");
      let key = new_key();
      let counter = ERC20Token::usdc().address;

      let mut v4 = Vec::with_capacity(RECORDS as usize);
      for i in 0..RECORDS {
         let mut bytes = [0u8; 20];
         bytes[..8].copy_from_slice(&i.to_be_bytes());
         v4.push((
            1,
            V4PoolRecord {
               currency0: Address::from(bytes),
               currency1: counter,
               fee: 3000,
               tick_spacing: 60,
               hooks: Address::ZERO,
            },
         ));
      }

      let store = PoolStore::open_at(&path, &key).unwrap();
      let started = Instant::now();
      store.write_catalog(&PoolDbMeta::default(), &[], &v4, &[], &[], &[]).unwrap();
      println!("10k write took {:?}", started.elapsed());
      println!(
         "raw file is {} B",
         std::fs::metadata(&path).unwrap().len()
      );
      store.compact().unwrap();
      drop(store);

      let file_len = std::fs::metadata(&path).unwrap().len();
      let bytes_per_pool = file_len / RECORDS;
      println!("file is {file_len} B for {RECORDS} records");
      println!("bytes/pool = {bytes_per_pool}");
      assert!(
         bytes_per_pool <= 420,
         "a cold V4 record costs {bytes_per_pool} B on disk, over the 420 B budget"
      );
   }
}
