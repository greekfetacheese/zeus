use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::anyhow;
use bincode_next::serde::{decode_from_slice, encode_to_vec};
use redb::backends::InMemoryBackend;
use redb::{Database as RedbInner, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::task;
use tracing::{info, warn};

use super::types::{SyncEvent, SyncerError};

const SNAP_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("snap");

/// On-disk `SnapshotDbMeta` version. Bump if the meta / chunk envelope changes.
pub const SNAP_FORMAT: u32 = 1;

/// Sealed chunk size (event count). Last chunk may be smaller.
const CHUNK_EVENTS: usize = 2048;

/// redb's default page cache is 1 GiB. Snapshot tip appends touch one chunk.
const PAGE_CACHE_BYTES: usize = 1024 * 1024;

/// Coverage + chunk catalog stored at key `meta` in `events-snapshot:{chain}.db`.
///
/// Coverage is the closed interval `[coverage_start, block_number]` when
/// `coverage_start > 0`. Legacy (`coverage_start == 0`) is treated as complete
/// history up to `block_number` (no known gap below the first stored event).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotDbMeta {
   pub version: u32,
   /// Inclusive start of the covered block range.
   ///
   /// `0` = legacy / unknown: do not assume a hole before the first stored event;
   /// treat the snapshot as usable for any `from_block <= block_number`.
   pub coverage_start: u64,
   /// Inclusive end of the covered block range (highest block known complete).
   pub block_number: u64,
   pub chunks: Vec<ChunkMeta>,
}

impl SnapshotDbMeta {
   pub fn event_count(&self) -> u64 {
      self.chunks.iter().map(|c| c.event_count as u64).sum()
   }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkMeta {
   pub index: u32,
   pub first_block: u64,
   pub last_block: u64,
   pub event_count: u32,
}

#[derive(Clone)]
struct EventsSnapshotDb {
   inner: Arc<RwLock<RedbInner>>,
}

/// Loader for the per-chain events snapshot (`events-snapshot:{chain}.db`).
pub struct SnapshotLoader {
   cache_dir: PathBuf,
   dbs: Arc<Mutex<HashMap<u64, EventsSnapshotDb>>>,
   in_memory: bool,
}

impl Clone for SnapshotLoader {
   fn clone(&self) -> Self {
      Self {
         cache_dir: self.cache_dir.clone(),
         dbs: self.dbs.clone(),
         in_memory: self.in_memory,
      }
   }
}

impl SnapshotLoader {
   pub fn new(cache_dir: PathBuf) -> Self {
      Self::remove_legacy_blobs(&cache_dir);
      Self {
         cache_dir,
         dbs: Arc::new(Mutex::new(HashMap::new())),
         in_memory: false,
      }
   }

   /// Drop leftover `events-snapshot:{chain}.data` / `.meta` (no import).
   fn remove_legacy_blobs(cache_dir: &Path) {
      let Ok(entries) = std::fs::read_dir(cache_dir) else {
         return;
      };
      for entry in entries.flatten() {
         let name = entry.file_name();
         let Some(name) = name.to_str() else {
            continue;
         };
         let Some(rest) = name.strip_prefix("events-snapshot:") else {
            continue;
         };
         if !(rest.ends_with(".data") || rest.ends_with(".meta")) {
            continue;
         }
         let path = entry.path();
         match std::fs::remove_file(&path) {
            Ok(()) => info!(
               "Removed leftover Railgun events snapshot blob {}",
               path.display()
            ),
            Err(e) => warn!(
               "Failed to remove leftover snapshot blob {}: {}",
               path.display(),
               e
            ),
         }
      }
   }

   /// In-memory redb (tests). Same API as a file-backed loader.
   pub fn in_memory() -> Self {
      Self {
         cache_dir: PathBuf::new(),
         dbs: Arc::new(Mutex::new(HashMap::new())),
         in_memory: true,
      }
   }

   pub fn db_filename(&self, chain_id: u64) -> String {
      format!("events-snapshot:{}.db", chain_id)
   }

   /// Open (and cache) the per-chain snapshot redb, acquiring its file lock now.
   ///
   /// Call this at provider-creation time so lock contention with a stale
   /// provider is surfaced inside the caller's retry window instead of lazily on
   /// the first sync. A missing db is not created (`Ok(())`); a locked or
   /// corrupt db returns the underlying (typed) error.
   pub fn acquire(&self, chain_id: u64) -> Result<(), anyhow::Error> {
      self.db_handle(chain_id, false).map(|_| ())
   }

   /// Tip path: caller only needs blocks **after** the snapshot's covered tip.
   ///
   /// - `from_block > snapshot_block` → pure RPC delta (trees already on disk).
   /// - Otherwise → historical path (slice blob + optional RPC tail/prefix).
   ///
   /// `snapshot_block == 0` means no snapshot yet → historical/cold fetch.
   ///
   /// Mid-chain resume **inside** snapshot coverage stays on the historical path
   /// so the blob is used (that is the whole point of the snapshot). A complete
   /// blob is required; never paper over gaps by skipping the snapshot.
   pub fn is_tip_sync(snapshot_block: u64, from_block: u64) -> bool {
      snapshot_block > 0 && from_block > snapshot_block
   }

   /// Sort events by chain block (stable). Leaf indices in payloads already
   /// identify tree positions; ordering keeps decryption/account scans deterministic.
   pub fn sort_events(events: &mut [SyncEvent]) {
      events.sort_by_key(|ev| ev.block_number());
   }

   /// Returns the highest block the snapshot is known to cover.
   ///
   /// Reads `events-snapshot:{chain}.db` only. Does **not** create a redb file
   /// on a miss and does not consult leftover `.data` / `.meta` blobs.
   pub async fn load_meta(&self, chain_id: u64) -> Result<u64, SyncerError> {
      if self.in_memory {
         return Ok(self.read_db_meta(chain_id).await?.map(|m| m.block_number).unwrap_or(0));
      }

      let db_path = self.cache_dir.join(self.db_filename(chain_id));
      if db_path.exists() {
         match std::fs::metadata(&db_path) {
            Ok(m) if m.len() == 0 => return Ok(0),
            Ok(_) => {
               return Ok(self.read_db_meta(chain_id).await?.map(|m| m.block_number).unwrap_or(0));
            }
            Err(e) => return Err(SyncerError::new(e)),
         }
      }

      Ok(0)
   }

   /// Full redb meta record. Missing db / missing key → default (block 0).
   pub async fn coverage(&self, chain_id: u64) -> Result<SnapshotDbMeta, SyncerError> {
      Ok(self.read_db_meta(chain_id).await?.unwrap_or_default())
   }

   /// Write `meta` into the redb file (creates it). Tests / later append path.
   pub async fn put_meta(&self, chain_id: u64, meta: &SnapshotDbMeta) -> Result<(), anyhow::Error> {
      let db = self
         .db_handle(chain_id, true)?
         .ok_or_else(|| anyhow!("failed to open snapshot db for chain {chain_id}"))?;
      let bytes = encode_to_vec(meta, bincode_next::config::standard())
         .map_err(|e| anyhow!("bincode encode error: {e}"))?;

      task::spawn_blocking(move || -> Result<(), anyhow::Error> {
         let guard = db.inner.write().map_err(|e| anyhow!("snapshot db lock: {e}"))?;
         let mut tx = guard.begin_write()?;
         tx.set_durability(Durability::Immediate)?;
         {
            let mut table = tx.open_table(SNAP_TABLE)?;
            table.insert(meta_key(), bytes.as_slice())?;
         }
         tx.commit()?;
         Ok(())
      })
      .await
      .map_err(|e| anyhow!("snapshot db join: {e}"))?
   }

   /// Append `delta` as sealed/open chunks. Empty delta on an empty DB is a no-op
   /// (never tip-bootstrap gappy coverage). Empty delta on an existing DB only
   /// advances `block_number` when `to_block` is higher.
   ///
   /// `coverage_start` is applied only when seeding a new snapshot.
   pub async fn append(
      &self,
      chain_id: u64,
      delta: &[SyncEvent],
      to_block: u64,
      coverage_start: Option<u64>,
   ) -> Result<(), anyhow::Error> {
      if delta.is_empty() {
         return self.advance_meta(chain_id, to_block).await;
      }

      let mut events = delta.to_vec();
      Self::sort_events(&mut events);

      let db = self
         .db_handle(chain_id, true)?
         .ok_or_else(|| anyhow!("failed to open snapshot db for chain {chain_id}"))?;

      task::spawn_blocking(move || -> Result<(), anyhow::Error> {
         let guard = db.inner.write().map_err(|e| anyhow!("snapshot db lock: {e}"))?;
         let mut meta = read_meta_locked(&guard)?.unwrap_or_default();
         let seeding = meta.chunks.is_empty() && meta.block_number == 0;

         let mut open = Vec::new();
         if seeding {
            meta.version = SNAP_FORMAT;
            meta.coverage_start = coverage_start
               .unwrap_or_else(|| events.first().map(|e| e.block_number()).unwrap_or(0));
         } else if let Some(last) = meta.chunks.last().copied() {
            if (last.event_count as usize) < CHUNK_EVENTS {
               open = decode_chunk_locked(&guard, last.index)?;
               meta.chunks.pop();
            }
         }

         open.extend(events);
         debug_assert!(
            open.windows(2).all(|w| w[0].block_number() <= w[1].block_number()),
            "events snapshot append must stay block-monotonic"
         );

         let mut next_index = meta.chunks.last().map(|c| c.index + 1).unwrap_or(0);
         let mut writes: Vec<(u32, Vec<u8>, ChunkMeta)> = Vec::new();
         while open.len() >= CHUNK_EVENTS {
            let chunk: Vec<SyncEvent> = open.drain(..CHUNK_EVENTS).collect();
            let cm = chunk_meta(next_index, &chunk);
            let bytes = encode_to_vec(&chunk, bincode_next::config::standard())
               .map_err(|e| anyhow!("bincode encode error: {e}"))?;
            writes.push((next_index, bytes, cm));
            next_index += 1;
         }
         if !open.is_empty() {
            let cm = chunk_meta(next_index, &open);
            let bytes = encode_to_vec(&open, bincode_next::config::standard())
               .map_err(|e| anyhow!("bincode encode error: {e}"))?;
            writes.push((next_index, bytes, cm));
         }

         if to_block > meta.block_number {
            meta.block_number = to_block;
         }

         let mut tx = guard.begin_write()?;
         tx.set_durability(Durability::Immediate)?;
         {
            let mut table = tx.open_table(SNAP_TABLE)?;
            for (index, bytes, cm) in writes {
               let key = chunk_key(index);
               table.insert(key.as_slice(), bytes.as_slice())?;
               meta.chunks.push(cm);
            }
            let meta_bytes = encode_to_vec(&meta, bincode_next::config::standard())
               .map_err(|e| anyhow!("bincode encode error: {e}"))?;
            table.insert(meta_key(), meta_bytes.as_slice())?;
         }
         tx.commit()?;
         Ok(())
      })
      .await
      .map_err(|e| anyhow!("snapshot db join: {e}"))?
   }

   /// Replace the snapshot so it covers `[coverage_start, to_block]`.
   ///
   /// Used when a historical sync fetches blocks *below* the current
   /// `coverage_start` (a full-history catch-up): that fetched prefix would
   /// otherwise be discarded, because the append-only paths never lower
   /// `coverage_start`. Since the fetched range is contiguous and reaches the old
   /// tip, the whole snapshot can safely be rewritten downward once — after which
   /// normal tip appends take over again.
   ///
   /// `events` must be block-sorted. Every existing chunk key is dropped and the
   /// events are re-chunked in a single transaction. `events` is borrowed, so a
   /// failed write leaves the caller's copy intact.
   pub async fn rewrite(
      &self,
      chain_id: u64,
      events: &[SyncEvent],
      to_block: u64,
      coverage_start: u64,
   ) -> Result<(), anyhow::Error> {
      // Encode up front: `events` cannot be borrowed into `spawn_blocking`, and
      // re-encoding is far cheaper than deep-cloning a multi-million event Vec.
      let chunk_count = events.len() / CHUNK_EVENTS + 1;
      let mut chunk_bytes: Vec<Vec<u8>> = Vec::with_capacity(chunk_count);
      let mut chunks: Vec<ChunkMeta> = Vec::with_capacity(chunk_count);
      for (index, chunk) in events.chunks(CHUNK_EVENTS).enumerate() {
         let index = index as u32;
         chunks.push(chunk_meta(index, chunk));
         chunk_bytes.push(
            encode_to_vec(chunk, bincode_next::config::standard())
               .map_err(|e| anyhow!("bincode encode error: {e}"))?,
         );
      }

      let meta = SnapshotDbMeta {
         version: SNAP_FORMAT,
         coverage_start,
         block_number: to_block,
         chunks,
      };
      let meta_bytes = encode_to_vec(&meta, bincode_next::config::standard())
         .map_err(|e| anyhow!("bincode encode error: {e}"))?;

      let db = self
         .db_handle(chain_id, true)?
         .ok_or_else(|| anyhow!("failed to open snapshot db for chain {chain_id}"))?;

      task::spawn_blocking(move || -> Result<(), anyhow::Error> {
         let guard = db.inner.write().map_err(|e| anyhow!("snapshot db lock: {e}"))?;

         // Drop every existing chunk key so a shorter rewrite cannot leave stale
         // chunks behind (which `load_range` would still see).
         let stale: Vec<Vec<u8>> = {
            let tx = guard.begin_read()?;
            let table: redb::ReadOnlyTable<&[u8], &[u8]> = tx.open_table(SNAP_TABLE)?;
            let mut keys = Vec::new();
            for kv in table.iter()? {
               let (k, _) = kv?;
               if k.value().starts_with(b"c:") {
                  keys.push(k.value().to_vec());
               }
            }
            keys
         };

         let mut tx = guard.begin_write()?;
         tx.set_durability(Durability::Immediate)?;
         {
            let mut table = tx.open_table(SNAP_TABLE)?;
            for key in &stale {
               table.remove(key.as_slice())?;
            }
            for (index, bytes) in chunk_bytes.iter().enumerate() {
               let key = chunk_key(index as u32);
               table.insert(key.as_slice(), bytes.as_slice())?;
            }
            table.insert(meta_key(), meta_bytes.as_slice())?;
         }
         tx.commit()?;
         Ok(())
      })
      .await
      .map_err(|e| anyhow!("snapshot db join: {e}"))?
   }

   /// Load events whose `block_number` is in `[from_block, to_block]`.
   pub async fn load_range(
      &self,
      chain_id: u64,
      from_block: u64,
      to_block: u64,
   ) -> Result<Vec<SyncEvent>, SyncerError> {
      if from_block > to_block {
         return Ok(Vec::new());
      }

      let Some(db) = self
         .db_handle(chain_id, false)
         .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?
      else {
         return Ok(Vec::new());
      };

      task::spawn_blocking(move || -> Result<Vec<SyncEvent>, SyncerError> {
         let guard = db
            .inner
            .read()
            .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?;
         let Some(meta) = read_meta_locked(&guard)
            .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?
         else {
            return Ok(Vec::new());
         };

         let mut out = Vec::new();
         for chunk in &meta.chunks {
            if chunk.last_block < from_block || chunk.first_block > to_block {
               continue;
            }
            let events = decode_chunk_locked(&guard, chunk.index)
               .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?;
            out.extend(events.into_iter().filter(|ev| {
               let b = ev.block_number();
               b >= from_block && b <= to_block
            }));
         }
         Ok(out)
      })
      .await
      .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?
   }

   /// Bump `block_number` without rewriting chunks. No-op on an empty snapshot
   /// so a tip-only empty fetch cannot claim full history.
   pub async fn advance_meta(&self, chain_id: u64, to_block: u64) -> Result<(), anyhow::Error> {
      let Some(db) = self.db_handle(chain_id, false)? else {
         return Ok(());
      };

      task::spawn_blocking(move || -> Result<(), anyhow::Error> {
         let guard = db.inner.write().map_err(|e| anyhow!("snapshot db lock: {e}"))?;
         let Some(mut meta) = read_meta_locked(&guard)? else {
            return Ok(());
         };
         if meta.chunks.is_empty() || meta.block_number == 0 {
            return Ok(());
         }
         if to_block <= meta.block_number {
            return Ok(());
         }
         meta.block_number = to_block;
         let bytes = encode_to_vec(&meta, bincode_next::config::standard())
            .map_err(|e| anyhow!("bincode encode error: {e}"))?;

         let mut tx = guard.begin_write()?;
         tx.set_durability(Durability::Immediate)?;
         {
            let mut table = tx.open_table(SNAP_TABLE)?;
            table.insert(meta_key(), bytes.as_slice())?;
         }
         tx.commit()?;
         Ok(())
      })
      .await
      .map_err(|e| anyhow!("snapshot db join: {e}"))?
   }

   /// Compact the snapshot redb file. Missing db is `Ok(false)` and does not create one.
   pub async fn compact(&self, chain_id: u64) -> Result<bool, anyhow::Error> {
      let Some(db) = self.db_handle(chain_id, false)? else {
         return Ok(false);
      };

      task::spawn_blocking(move || -> Result<bool, anyhow::Error> {
         let mut guard = db.inner.write().map_err(|e| anyhow!("snapshot db lock: {e}"))?;
         let did_compact = guard.compact()?;
         Ok(did_compact)
      })
      .await
      .map_err(|e| anyhow!("snapshot db join: {e}"))?
   }

   #[cfg(test)]
   async fn raw_chunk(&self, chain_id: u64, index: u32) -> Result<Option<Vec<u8>>, anyhow::Error> {
      let Some(db) = self.db_handle(chain_id, false)? else {
         return Ok(None);
      };
      task::spawn_blocking(
         move || -> Result<Option<Vec<u8>>, anyhow::Error> {
            let guard = db.inner.read().map_err(|e| anyhow!("snapshot db lock: {e}"))?;
            let tx = guard.begin_read()?;
            let table: redb::ReadOnlyTable<&[u8], &[u8]> = tx.open_table(SNAP_TABLE)?;
            let key = chunk_key(index);
            match table.get(key.as_slice())? {
               Some(v) => Ok(Some(v.value().to_vec())),
               None => Ok(None),
            }
         },
      )
      .await
      .map_err(|e| anyhow!("snapshot db join: {e}"))?
   }

   async fn read_db_meta(&self, chain_id: u64) -> Result<Option<SnapshotDbMeta>, SyncerError> {
      let Some(db) = self
         .db_handle(chain_id, false)
         .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?
      else {
         return Ok(None);
      };

      task::spawn_blocking(
         move || -> Result<Option<SnapshotDbMeta>, SyncerError> {
            let guard = db
               .inner
               .read()
               .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?;
            let tx = guard
               .begin_read()
               .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?;
            let table: redb::ReadOnlyTable<&[u8], &[u8]> = tx
               .open_table(SNAP_TABLE)
               .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?;
            match table.get(meta_key()) {
               Ok(Some(v)) => {
                  let meta = decode_meta(v.value())
                     .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?;
                  Ok(Some(meta))
               }
               Ok(None) => Ok(None),
               Err(e) => Err(SyncerError::new(std::io::Error::other(
                  e.to_string(),
               ))),
            }
         },
      )
      .await
      .map_err(|e| SyncerError::new(std::io::Error::other(e.to_string())))?
   }

   fn db_handle(
      &self,
      chain_id: u64,
      create: bool,
   ) -> Result<Option<EventsSnapshotDb>, anyhow::Error> {
      let mut map = self.dbs.lock().map_err(|e| anyhow!("snapshot db map lock: {e}"))?;
      if let Some(db) = map.get(&chain_id) {
         return Ok(Some(db.clone()));
      }

      if self.in_memory {
         if !create {
            return Ok(None);
         }
         let inner = RedbInner::builder()
            .set_cache_size(PAGE_CACHE_BYTES)
            .create_with_backend(InMemoryBackend::new())?;
         init_snap_table(&inner)?;
         let db = EventsSnapshotDb {
            inner: Arc::new(RwLock::new(inner)),
         };
         map.insert(chain_id, db.clone());
         return Ok(Some(db));
      }

      let path = self.cache_dir.join(self.db_filename(chain_id));
      if !path.exists() {
         if !create {
            return Ok(None);
         }
         std::fs::create_dir_all(&self.cache_dir)?;
      } else if std::fs::metadata(&path)?.len() == 0 && !create {
         return Ok(None);
      }

      let inner = RedbInner::builder().set_cache_size(PAGE_CACHE_BYTES).create(&path)?;
      init_snap_table(&inner)?;
      let db = EventsSnapshotDb {
         inner: Arc::new(RwLock::new(inner)),
      };
      map.insert(chain_id, db.clone());
      Ok(Some(db))
   }
}

fn meta_key() -> &'static [u8] {
   b"meta"
}

fn chunk_key(index: u32) -> Vec<u8> {
   format!("c:{index}").into_bytes()
}

fn chunk_meta(index: u32, events: &[SyncEvent]) -> ChunkMeta {
   ChunkMeta {
      index,
      first_block: events.first().map(|e| e.block_number()).unwrap_or(0),
      last_block: events.last().map(|e| e.block_number()).unwrap_or(0),
      event_count: events.len() as u32,
   }
}

fn decode_meta(bytes: &[u8]) -> Result<SnapshotDbMeta, anyhow::Error> {
   let (meta, _) = decode_from_slice::<SnapshotDbMeta, _>(bytes, bincode_next::config::standard())
      .map_err(|e| anyhow!("bincode decode error: {e}"))?;
   if meta.version != SNAP_FORMAT {
      return Err(anyhow!(
         "unsupported events snapshot format version {} (expected {})",
         meta.version,
         SNAP_FORMAT
      ));
   }
   Ok(meta)
}

fn read_meta_locked(inner: &RedbInner) -> Result<Option<SnapshotDbMeta>, anyhow::Error> {
   let tx = inner.begin_read()?;
   let table: redb::ReadOnlyTable<&[u8], &[u8]> = tx.open_table(SNAP_TABLE)?;
   match table.get(meta_key())? {
      Some(v) => Ok(Some(decode_meta(v.value())?)),
      None => Ok(None),
   }
}

fn decode_chunk_locked(inner: &RedbInner, index: u32) -> Result<Vec<SyncEvent>, anyhow::Error> {
   let tx = inner.begin_read()?;
   let table: redb::ReadOnlyTable<&[u8], &[u8]> = tx.open_table(SNAP_TABLE)?;
   let key = chunk_key(index);
   let Some(v) = table.get(key.as_slice())? else {
      return Err(anyhow!("missing snapshot chunk {index}"));
   };
   let (events, _) =
      decode_from_slice::<Vec<SyncEvent>, _>(v.value(), bincode_next::config::standard())
         .map_err(|e| anyhow!("bincode decode error: {e}"))?;
   Ok(events)
}

fn init_snap_table(inner: &RedbInner) -> Result<(), redb::Error> {
   let tx = inner.begin_write()?;
   {
      let _ = tx.open_table(SNAP_TABLE)?;
   }
   tx.commit()?;
   Ok(())
}

#[cfg(test)]
mod tip_sync_tests {
   use super::*;

   #[test]
   fn tip_when_past_snapshot() {
      assert!(SnapshotLoader::is_tip_sync(
         25_000_000, 25_000_001
      ));
      assert!(!SnapshotLoader::is_tip_sync(0, 1_000));
   }

   #[test]
   fn resume_inside_snapshot_uses_historical() {
      // Mid-chain resume must use the blob — that is why the snapshot exists.
      assert!(!SnapshotLoader::is_tip_sync(
         25_629_896, 24_653_562
      ));
      assert!(!SnapshotLoader::is_tip_sync(
         25_629_896, 25_629_896
      ));
   }

   #[test]
   fn cold_bootstrap_uses_historical() {
      assert!(!SnapshotLoader::is_tip_sync(
         25_629_896, 14_693_013
      ));
   }
}

#[cfg(test)]
mod redb_meta_tests {
   use super::*;
   use std::time::{SystemTime, UNIX_EPOCH};

   fn unique_dir() -> PathBuf {
      let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
      let dir = std::env::temp_dir().join(format!(
         "zeus-events-snap-{}-{}",
         std::process::id(),
         nanos
      ));
      std::fs::create_dir_all(&dir).unwrap();
      dir
   }

   #[tokio::test]
   async fn meta_roundtrip_in_memory() {
      let loader = SnapshotLoader::in_memory();
      let written = SnapshotDbMeta {
         version: SNAP_FORMAT,
         coverage_start: 100,
         block_number: 200,
         chunks: vec![ChunkMeta {
            index: 0,
            first_block: 100,
            last_block: 200,
            event_count: 3,
         }],
      };
      loader.put_meta(1, &written).await.unwrap();

      assert_eq!(loader.load_meta(1).await.unwrap(), 200);
      let loaded = loader.coverage(1).await.unwrap();
      assert_eq!(loaded.version, SNAP_FORMAT);
      assert_eq!(loaded.coverage_start, 100);
      assert_eq!(loaded.block_number, 200);
      assert_eq!(loaded.chunks.len(), 1);
      assert_eq!(loaded.chunks[0].event_count, 3);
   }

   #[tokio::test]
   async fn unknown_version_is_rejected() {
      let loader = SnapshotLoader::in_memory();
      let bad = SnapshotDbMeta {
         version: SNAP_FORMAT + 1,
         coverage_start: 1,
         block_number: 2,
         chunks: Vec::new(),
      };
      loader.put_meta(1, &bad).await.unwrap();

      assert!(loader.load_meta(1).await.is_err());
      assert!(loader.coverage(1).await.is_err());
   }

   #[tokio::test]
   async fn second_loader_open_reports_already_open() {
      let dir = unique_dir();
      let a = SnapshotLoader::new(dir.clone());
      let meta = SnapshotDbMeta {
         version: SNAP_FORMAT,
         coverage_start: 1,
         block_number: 1,
         chunks: Vec::new(),
      };
      a.put_meta(1, &meta).await.unwrap();

      // `a` still holds the redb file; a fresh loader must fail to open it and
      // the failure must be detectable without matching the message text.
      let b = SnapshotLoader::new(dir.clone());
      let err = b.acquire(1).unwrap_err();
      assert!(
         crate::database::is_database_already_open(&err),
         "expected typed already-open detection, got: {err}"
      );

      drop(a);
      let _ = std::fs::remove_dir_all(&dir);
   }

   #[tokio::test]
   async fn missing_db_load_meta_is_zero() {
      let dir = unique_dir();
      let loader = SnapshotLoader::new(dir.clone());
      let db_path = dir.join(loader.db_filename(1));

      assert_eq!(loader.load_meta(1).await.unwrap(), 0);
      assert!(
         !db_path.exists(),
         "load_meta must not create the redb file"
      );

      let _ = std::fs::remove_dir_all(&dir);
   }

   #[tokio::test]
   async fn empty_file_is_zero() {
      let dir = unique_dir();
      let loader = SnapshotLoader::new(dir.clone());
      let db_path = dir.join(loader.db_filename(7));
      std::fs::write(&db_path, b"").unwrap();

      assert_eq!(loader.load_meta(7).await.unwrap(), 0);

      let _ = std::fs::remove_dir_all(&dir);
   }

   #[test]
   fn leftover_blobs_deleted_on_new() {
      let dir = unique_dir();
      let data = dir.join("events-snapshot:1.data");
      let meta = dir.join("events-snapshot:1.meta");
      let keep = dir.join("events-snapshot:1.db");
      std::fs::write(&data, b"old").unwrap();
      std::fs::write(&meta, b"old").unwrap();
      std::fs::write(&keep, b"").unwrap();

      let _loader = SnapshotLoader::new(dir.clone());
      assert!(!data.exists());
      assert!(!meta.exists());
      assert!(keep.exists());

      let _ = std::fs::remove_dir_all(&dir);
   }
}

#[cfg(test)]
mod redb_chunk_tests {
   use super::*;
   use crate::indexer::syncer::types::Nullified;

   fn null_at(block: u64) -> SyncEvent {
      SyncEvent::Nullified(
         Nullified {
            tree_number: 0,
            nullifier: Default::default(),
            timestamp: 0,
            tx_hash: Default::default(),
         },
         block,
      )
   }

   #[tokio::test]
   async fn append_then_load_range_preserves_order() {
      let loader = SnapshotLoader::in_memory();
      let events = vec![null_at(30), null_at(10), null_at(20)];
      loader.append(1, &events, 30, Some(10)).await.unwrap();

      let loaded = loader.load_range(1, 0, u64::MAX).await.unwrap();
      assert_eq!(
         loaded.iter().map(|e| e.block_number()).collect::<Vec<_>>(),
         vec![10, 20, 30]
      );
      assert_eq!(loader.load_meta(1).await.unwrap(), 30);
   }

   #[tokio::test]
   #[cfg(debug_assertions)]
   async fn non_monotonic_append_is_caught() {
      // A partial open chunk is reopened on the next append; feeding a lower block
      // than what is already stored must not silently corrupt chunk ordering.
      let loader = SnapshotLoader::in_memory();
      loader.append(1, &[null_at(10), null_at(20)], 20, Some(10)).await.unwrap();

      let res = loader.append(1, &[null_at(5)], 5, None).await;
      assert!(
         res.is_err(),
         "out-of-order append must be rejected"
      );
   }

   #[tokio::test]
   async fn rewrite_extends_coverage_downward() {
      let loader = SnapshotLoader::in_memory();
      // Seed a near-tip slice (what an already-synced indexer produces).
      loader.append(1, &[null_at(100), null_at(200)], 200, Some(100)).await.unwrap();
      assert_eq!(
         loader.coverage(1).await.unwrap().coverage_start,
         100
      );

      // A later full-history sync fetches below coverage_start; extending
      // downward must lower coverage_start and keep every event.
      loader
         .rewrite(
            1,
            &[null_at(10), null_at(100), null_at(200)],
            250,
            10,
         )
         .await
         .unwrap();

      let meta = loader.coverage(1).await.unwrap();
      assert_eq!(meta.coverage_start, 10);
      assert_eq!(meta.block_number, 250);
      assert_eq!(meta.chunks.len(), 1);
      let loaded = loader.load_range(1, 0, u64::MAX).await.unwrap();
      assert_eq!(
         loaded.iter().map(|e| e.block_number()).collect::<Vec<_>>(),
         vec![10, 100, 200]
      );
   }

   #[tokio::test]
   async fn rewrite_file_backed_lowers_coverage() {
      let nanos = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap()
         .as_nanos();
      let dir = std::env::temp_dir().join(format!(
         "zeus-events-snap-rewrite-{}-{}",
         std::process::id(),
         nanos
      ));
      std::fs::create_dir_all(&dir).unwrap();
      let loader = SnapshotLoader::new(dir.clone());

      loader.append(1, &[null_at(500), null_at(600)], 600, Some(500)).await.unwrap();
      loader
         .rewrite(
            1,
            &[null_at(3), null_at(500), null_at(600)],
            700,
            3,
         )
         .await
         .unwrap();
      drop(loader);

      // Reopen from disk (fresh handle) to prove the rewrite persisted.
      let reopened = SnapshotLoader::new(dir.clone());
      let meta = reopened.coverage(1).await.unwrap();
      assert_eq!(meta.coverage_start, 3);
      assert_eq!(meta.block_number, 700);
      let loaded = reopened.load_range(1, 0, u64::MAX).await.unwrap();
      assert_eq!(
         loaded.iter().map(|e| e.block_number()).collect::<Vec<_>>(),
         vec![3, 500, 600]
      );
      drop(reopened);

      let _ = std::fs::remove_dir_all(&dir);
   }

   #[tokio::test]
   async fn rewrite_drops_stale_chunks() {
      let loader = SnapshotLoader::in_memory();
      let total = CHUNK_EVENTS as u64 * 2 + 5;
      let big: Vec<_> = (1..=total).map(null_at).collect();
      loader.append(1, &big, total, Some(1)).await.unwrap();
      assert_eq!(loader.coverage(1).await.unwrap().chunks.len(), 3);

      // Shrinking rewrite must not leave orphaned chunk keys behind.
      loader.rewrite(1, &[null_at(5), null_at(6)], 6, 5).await.unwrap();

      assert_eq!(loader.coverage(1).await.unwrap().chunks.len(), 1);
      assert!(loader.raw_chunk(1, 1).await.unwrap().is_none());
      assert!(loader.raw_chunk(1, 2).await.unwrap().is_none());
      let loaded = loader.load_range(1, 0, u64::MAX).await.unwrap();
      assert_eq!(loaded.len(), 2);
   }

   #[tokio::test]
   async fn second_append_does_not_rewrite_sealed_chunk() {
      let loader = SnapshotLoader::in_memory();
      let first: Vec<_> = (1..=CHUNK_EVENTS as u64).map(null_at).collect();
      loader.append(1, &first, CHUNK_EVENTS as u64, Some(1)).await.unwrap();

      let sealed = loader.raw_chunk(1, 0).await.unwrap().expect("chunk 0");
      loader
         .append(
            1,
            &[null_at(CHUNK_EVENTS as u64 + 1)],
            CHUNK_EVENTS as u64 + 1,
            None,
         )
         .await
         .unwrap();

      let sealed_after = loader.raw_chunk(1, 0).await.unwrap().expect("chunk 0");
      assert_eq!(sealed, sealed_after);
      let meta = loader.coverage(1).await.unwrap();
      assert_eq!(meta.chunks.len(), 2);
      assert_eq!(meta.chunks[0].event_count, CHUNK_EVENTS as u32);
      assert_eq!(meta.chunks[1].event_count, 1);
   }

   #[tokio::test]
   async fn empty_delta_advances_block_number_only() {
      let loader = SnapshotLoader::in_memory();
      loader.append(1, &[null_at(10), null_at(20)], 20, Some(10)).await.unwrap();

      loader.append(1, &[], 50, None).await.unwrap();

      assert_eq!(loader.load_meta(1).await.unwrap(), 50);
      let loaded = loader.load_range(1, 0, u64::MAX).await.unwrap();
      assert_eq!(loaded.len(), 2);
      assert_eq!(loaded[0].block_number(), 10);
      assert_eq!(loaded[1].block_number(), 20);
   }

   #[tokio::test]
   async fn refuse_tip_bootstrap_on_empty_db() {
      let loader = SnapshotLoader::in_memory();
      loader.append(1, &[], 99, None).await.unwrap();
      assert_eq!(loader.load_meta(1).await.unwrap(), 0);
      let loaded = loader.load_range(1, 0, u64::MAX).await.unwrap();
      assert!(loaded.is_empty());
   }

   #[tokio::test]
   async fn seed_sets_coverage_start() {
      let loader = SnapshotLoader::in_memory();
      loader.append(1, &[null_at(5)], 5, Some(1000)).await.unwrap();
      let meta = loader.coverage(1).await.unwrap();
      assert_eq!(meta.coverage_start, 1000);
      assert_eq!(meta.version, SNAP_FORMAT);
      assert_eq!(meta.block_number, 5);
   }

   #[tokio::test]
   async fn load_range_filters_blocks() {
      let loader = SnapshotLoader::in_memory();
      loader
         .append(
            1,
            &[null_at(10), null_at(20), null_at(30)],
            30,
            Some(10),
         )
         .await
         .unwrap();

      let mid = loader.load_range(1, 15, 25).await.unwrap();
      assert_eq!(mid.len(), 1);
      assert_eq!(mid[0].block_number(), 20);

      let none = loader.load_range(1, 40, 50).await.unwrap();
      assert!(none.is_empty());
   }

   #[tokio::test]
   async fn compact_in_memory_is_ok() {
      let loader = SnapshotLoader::in_memory();
      loader.append(1, &[null_at(1)], 1, Some(1)).await.unwrap();
      loader.compact(1).await.unwrap();
      let loaded = loader.load_range(1, 0, u64::MAX).await.unwrap();
      assert_eq!(loaded.len(), 1);
   }

   #[tokio::test]
   async fn compact_missing_db_is_false() {
      let loader = SnapshotLoader::in_memory();
      assert!(!loader.compact(1).await.unwrap());
   }

   #[tokio::test]
   async fn compact_file_backed_preserves_events() {
      let nanos = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap()
         .as_nanos();
      let dir = std::env::temp_dir().join(format!(
         "zeus-events-snap-compact-{}-{}",
         std::process::id(),
         nanos
      ));
      std::fs::create_dir_all(&dir).unwrap();
      let loader = SnapshotLoader::new(dir.clone());

      let first: Vec<_> = (1..=CHUNK_EVENTS as u64).map(null_at).collect();
      loader.append(1, &first, CHUNK_EVENTS as u64, Some(1)).await.unwrap();
      loader
         .append(
            1,
            &[null_at(CHUNK_EVENTS as u64 + 1)],
            CHUNK_EVENTS as u64 + 1,
            None,
         )
         .await
         .unwrap();

      let before = loader.load_range(1, 0, u64::MAX).await.unwrap();
      assert_eq!(before.len(), CHUNK_EVENTS + 1);

      loader.compact(1).await.unwrap();

      let after = loader.load_range(1, 0, u64::MAX).await.unwrap();
      assert_eq!(
         after.iter().map(|e| e.block_number()).collect::<Vec<_>>(),
         before.iter().map(|e| e.block_number()).collect::<Vec<_>>()
      );
      assert!(dir.join(loader.db_filename(1)).exists());

      let _ = std::fs::remove_dir_all(&dir);
   }
}
