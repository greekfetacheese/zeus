use std::path::{Path, PathBuf};

use anyhow::anyhow;
use bincode_next::serde::{decode_from_slice, encode_to_vec};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, warn};

use super::types::{SyncEvent, SyncerError};

// ? If we support more chains in the future this has to be adjusted.
/// How far the tip may get ahead of the on-disk events snapshot before we
/// pay for a full load+rewrite refresh.
///
/// ~30_000 blocks ≈ 4 days on Ethereum mainnet (12s blocks). Keeps the bootstrap
/// cache useful for new signers without rewriting tens of MB every tip sync.
pub const EVENTS_SNAPSHOT_REFRESH_BLOCK_INTERVAL: u64 = 30_000;

/// A snapshot of the synced events with coverage watermarks.
///
/// Used to speed up full re-syncs (e.g. when registering a new Railgun signer)
/// by replaying historical events from disk instead of hitting RPC/Subsquid again.
///
/// Coverage is the closed interval `[coverage_start, block_number]` when
/// `coverage_start > 0`. Legacy blobs (`coverage_start == 0`) are treated as
/// complete history up to `block_number` (no known gap below the first event).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EventsSnapshot {
   pub events: Vec<SyncEvent>,
   /// Inclusive end of covered block range (highest block known complete).
   pub block_number: u64,
   /// Inclusive start of covered block range.
   ///
   /// `0` = legacy / unknown: do not assume a hole before the first stored event;
   /// treat the blob as usable for any `from_block <= block_number` (old behaviour).
   #[serde(default)]
   pub coverage_start: u64,
}

/// Lightweight coverage watermark for the events snapshot.
///
/// Tip syncs only need this (not the full `events` Vec). Kept in a separate file so
/// interval syncs do not deserialize tens of MB of history on every tick.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct EventsSnapshotMeta {
   pub block_number: u64,
   #[serde(default)]
   pub coverage_start: u64,
}

/// Loader that persists/loads EventsSnapshot using bincode (compact binary).
#[derive(Debug, Clone)]
pub struct SnapshotLoader {
   cache_dir: PathBuf,
}

impl SnapshotLoader {
   pub fn new(cache_dir: PathBuf) -> Self {
      Self { cache_dir }
   }

   pub fn filename(&self, chain_id: u64) -> String {
      format!("events-snapshot:{}.data", chain_id)
   }

   pub fn meta_filename(&self, chain_id: u64) -> String {
      format!("events-snapshot:{}.meta", chain_id)
   }

   /// True when the snapshot lags `to_block` by at least
   /// [`EVENTS_SNAPSHOT_REFRESH_BLOCK_INTERVAL`] blocks.
   ///
   /// Requires a real existing snapshot (`snapshot_block > 0`). An empty / missing
   /// snapshot is filled by the historical catch-up path, not by tip refresh.
   pub fn should_refresh(snapshot_block: u64, to_block: u64) -> bool {
      snapshot_block > 0
         && to_block.saturating_sub(snapshot_block) >= EVENTS_SNAPSHOT_REFRESH_BLOCK_INTERVAL
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
   /// Prefers the tiny `.meta` file. On first run after upgrade (meta missing),
   /// falls back to loading the full snapshot once and writes the meta file.
   ///
   /// If meta claims coverage but the data blob is empty/missing, meta is treated
   /// as stale and cleared so we do not take a false tip path.
   pub async fn load_meta(&self, chain_id: u64) -> Result<u64, SyncerError> {
      let meta_path = self.cache_dir.join(self.meta_filename(chain_id));
      let data_path = self.cache_dir.join(self.filename(chain_id));

      if meta_path.exists() {
         match tokio::fs::read(&meta_path).await {
            Ok(data) if !data.is_empty() => {
               match decode_from_slice::<EventsSnapshotMeta, _>(
                  &data,
                  bincode_next::config::standard(),
               ) {
                  Ok((meta, _)) => {
                     // Meta without a real data blob is poison for tip detection.
                     if meta.block_number > 0 && !data_path.exists() {
                        warn!(
                           "Event snapshot meta exists (block={}) but data blob missing; clearing meta",
                           meta.block_number
                        );
                        let _ = tokio::fs::remove_file(&meta_path).await;
                        return Ok(0);
                     }
                     return Ok(meta.block_number);
                  }
                  Err(e) => {
                     error!(
                        "Event snapshot meta decode failed ({}). Falling back to full snapshot.",
                        e
                     );
                     let _ = tokio::fs::remove_file(&meta_path).await;
                  }
               }
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(SyncerError::new(e)),
         }
      }

      let snapshot = self.load(chain_id).await?;
      let block_number = snapshot.block_number;
      if block_number > 0 {
         if let Err(e) = self.save_meta(chain_id, block_number, snapshot.coverage_start).await {
            debug!(
               "Failed to write snapshot meta after fallback load: {}",
               e
            );
         }
      }
      Ok(block_number)
   }

   pub async fn save_meta(
      &self,
      chain_id: u64,
      block_number: u64,
      coverage_start: u64,
   ) -> Result<(), anyhow::Error> {
      let dir = &self.cache_dir;
      tokio::fs::create_dir_all(dir).await?;

      let path = dir.join(self.meta_filename(chain_id));
      let meta = EventsSnapshotMeta {
         block_number,
         coverage_start,
      };
      let bytes = encode_to_vec(&meta, bincode_next::config::standard())
         .map_err(|e| anyhow!("bincode encode error: {}", e))?;
      atomic_write(&path, &bytes).await?;
      Ok(())
   }

   pub async fn load(&self, chain_id: u64) -> Result<EventsSnapshot, SyncerError> {
      let path = self.cache_dir.join(self.filename(chain_id));

      if !path.exists() {
         return Ok(EventsSnapshot::default());
      }

      let data = match tokio::fs::read(&path).await {
         Ok(d) => d,
         Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(EventsSnapshot::default());
         }
         Err(e) => return Err(SyncerError::new(e)),
      };

      if data.is_empty() {
         return Ok(EventsSnapshot::default());
      }

      let mut snapshot = match decode_from_slice::<EventsSnapshot, _>(
         &data,
         bincode_next::config::standard(),
      ) {
         Ok((snapshot, _len)) => snapshot,
         Err(e) => {
            // Corrupt or incompatible snapshot (e.g. old format after code change).
            // Delete it so we don't keep failing, and start fresh.
            error!(
               "Event snapshot decode failed ({}). Deleting corrupt snapshot and starting fresh.",
               e
            );
            let _ = tokio::fs::remove_file(&path).await;
            let meta_path = self.cache_dir.join(self.meta_filename(chain_id));
            let _ = tokio::fs::remove_file(&meta_path).await;
            return Ok(EventsSnapshot::default());
         }
      };

      // Inconsistent empty blob with non-zero watermark → treat as empty.
      if snapshot.events.is_empty() && snapshot.block_number > 0 {
         warn!(
            "Event snapshot claims block {} but has 0 events; ignoring blob",
            snapshot.block_number
         );
         return Ok(EventsSnapshot::default());
      }

      Self::sort_events(&mut snapshot.events);

      // block_number is whatever was last written with the events blob.
      // Do not raise it from a tip-only meta file — that would skip real event gaps
      // on historical catch-up.
      Ok(snapshot)
   }

   pub async fn save(
      &self,
      chain_id: u64,
      mut snapshot: EventsSnapshot,
   ) -> Result<(), anyhow::Error> {
      let dir = &self.cache_dir;
      tokio::fs::create_dir_all(dir).await?;

      Self::sort_events(&mut snapshot.events);

      let path = dir.join(self.filename(chain_id));
      let block_number = snapshot.block_number;
      let coverage_start = snapshot.coverage_start;

      let bytes = encode_to_vec(&snapshot, bincode_next::config::standard())
         .map_err(|e| anyhow!("bincode encode error: {}", e))?;

      // Data first, then meta — readers that only see meta without data are cleared
      // in load_meta. Atomic data write avoids torn blobs on crash.
      atomic_write(&path, &bytes).await?;
      self.save_meta(chain_id, block_number, coverage_start).await?;
      Ok(())
   }

   /// Load the full snapshot, append `delta`, bump coverage to `to_block`, and rewrite.
   ///
   /// Prefer this only on historical catch-up paths. Tip syncs should not call this —
   /// loading + rewriting the multi‑MB blob every few minutes is a large RSS peak.
   ///
   /// Empty `delta` with a higher `to_block` advances coverage only when the blob
   /// already has real events (no silent “meta-only” full coverage from nothing).
   pub async fn append_delta(
      &self,
      chain_id: u64,
      delta: Vec<SyncEvent>,
      to_block: u64,
   ) -> Result<(), anyhow::Error> {
      if delta.is_empty() {
         let snap = self.load(chain_id).await.map_err(|e| anyhow!("{}", e))?;
         if snap.block_number == 0 || snap.events.is_empty() {
            return Ok(());
         }
         if to_block > snap.block_number {
            return self.save_meta(chain_id, to_block, snap.coverage_start).await;
         }
         return Ok(());
      }

      let mut snapshot = self
         .load(chain_id)
         .await
         .map_err(|e| anyhow!("load snapshot for append: {}", e))?;
      snapshot.events.extend(delta);
      if to_block > snapshot.block_number {
         snapshot.block_number = to_block;
      }
      self.save(chain_id, snapshot).await
   }
}

/// Write `bytes` via `path.tmp` + rename so a crash mid-write cannot leave a
/// half-encoded blob that decodes as garbage or empty.
async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), anyhow::Error> {
   let tmp = path.with_extension("tmp");
   tokio::fs::write(&tmp, bytes).await?;
   tokio::fs::rename(&tmp, path).await?;
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
