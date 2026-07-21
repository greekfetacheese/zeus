use std::path::PathBuf;

use anyhow::anyhow;
use bincode_next::serde::{decode_from_slice, encode_to_vec};
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use super::types::{SyncEvent, SyncerError};

/// A snapshot of the synced events with the latest synced block number.
/// Used to speed up full re-syncs (e.g. when registering a new Railgun signer)
/// by replaying historical events from disk instead of hitting RPC/Subsquid again.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EventsSnapshot {
   pub events: Vec<SyncEvent>,
   pub block_number: u64,
}

/// Lightweight coverage watermark for the events snapshot.
///
/// Tip syncs only need this (not the full `events` Vec). Kept in a separate file so
/// interval syncs do not deserialize tens of MB of history on every tick.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct EventsSnapshotMeta {
   pub block_number: u64,
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

   /// Returns the highest block the snapshot is known to cover.
   ///
   /// Prefers the tiny `.meta` file. On first run after upgrade (meta missing),
   /// falls back to loading the full snapshot once and writes the meta file.
   pub async fn load_meta(&self, chain_id: u64) -> Result<u64, SyncerError> {
      let meta_path = self.cache_dir.join(self.meta_filename(chain_id));

      if meta_path.exists() {
         match tokio::fs::read(&meta_path).await {
            Ok(data) if !data.is_empty() => {
               match decode_from_slice::<EventsSnapshotMeta, _>(
                  &data,
                  bincode_next::config::standard(),
               ) {
                  Ok((meta, _)) => return Ok(meta.block_number),
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

      // Migration / first use: pay the full load once, then keep meta warm.
      let snapshot = self.load(chain_id).await?;
      let block_number = snapshot.block_number;
      if block_number > 0 {
         if let Err(e) = self.save_meta(chain_id, block_number).await {
            debug!("Failed to write snapshot meta after fallback load: {}", e);
         }
      }
      Ok(block_number)
   }

   pub async fn save_meta(&self, chain_id: u64, block_number: u64) -> Result<(), anyhow::Error> {
      let dir = &self.cache_dir;
      tokio::fs::create_dir_all(dir).await?;

      let path = dir.join(self.meta_filename(chain_id));
      let meta = EventsSnapshotMeta { block_number };
      let bytes = encode_to_vec(&meta, bincode_next::config::standard())
         .map_err(|e| anyhow!("bincode encode error: {}", e))?;
      tokio::fs::write(path, bytes).await?;
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

      let mut snapshot =
         match decode_from_slice::<EventsSnapshot, _>(&data, bincode_next::config::standard()) {
            Ok((snapshot, _len)) => snapshot,
            Err(e) => {
               // Corrupt or incompatible snapshot (e.g. old format after code change).
               // Delete it so we don't keep failing, and start fresh.
               error!(
                  "Event snapshot decode failed ({}). Deleting corrupt snapshot and starting fresh.",
                  e
               );
               let _ = tokio::fs::remove_file(&path).await;
               return Ok(EventsSnapshot::default());
            }
         };

      // Meta is the coverage watermark source of truth when tip syncs advance it
      // without rewriting the full events blob (empty delta).
      let meta_path = self.cache_dir.join(self.meta_filename(chain_id));
      if meta_path.exists() {
         if let Ok(data) = tokio::fs::read(&meta_path).await {
            if let Ok((meta, _)) = decode_from_slice::<EventsSnapshotMeta, _>(
               &data,
               bincode_next::config::standard(),
            ) {
               if meta.block_number > snapshot.block_number {
                  snapshot.block_number = meta.block_number;
               }
            }
         }
      }

      Ok(snapshot)
   }

   pub async fn save(&self, chain_id: u64, snapshot: EventsSnapshot) -> Result<(), anyhow::Error> {
      let dir = &self.cache_dir;
      tokio::fs::create_dir_all(dir).await?;

      let path = dir.join(self.filename(chain_id));
      let block_number = snapshot.block_number;

      let bytes = encode_to_vec(&snapshot, bincode_next::config::standard())
         .map_err(|e| anyhow!("bincode encode error: {}", e))?;

      tokio::fs::write(path, bytes).await?;
      self.save_meta(chain_id, block_number).await?;
      Ok(())
   }

   /// Load the full snapshot, append `delta`, bump coverage to `to_block`, and rewrite.
   ///
   /// Only call when `delta` is non-empty — empty tip advances should use [`save_meta`].
   pub async fn append_delta(
      &self,
      chain_id: u64,
      delta: Vec<SyncEvent>,
      to_block: u64,
   ) -> Result<(), anyhow::Error> {
      if delta.is_empty() {
         return self.save_meta(chain_id, to_block).await;
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
