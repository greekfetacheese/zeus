use std::path::PathBuf;

use anyhow::anyhow;
use bincode_next::serde::{decode_from_slice, encode_to_vec};
use serde::{Deserialize, Serialize};
use tracing::error;

use super::types::{SyncEvent, SyncerError};

/// A snapshot of the synced events with the latest synced block number.
/// Used to speed up full re-syncs (e.g. when registering a new Railgun signer)
/// by replaying historical events from disk instead of hitting RPC/Subsquid again.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EventsSnapshot {
   pub events: Vec<SyncEvent>,
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

      match decode_from_slice::<EventsSnapshot, _>(&data, bincode_next::config::standard()) {
         Ok((snapshot, _len)) => Ok(snapshot),
         Err(e) => {
            // Corrupt or incompatible snapshot (e.g. old format after code change).
            // Delete it so we don't keep failing, and start fresh.
            error!(
               "Event snapshot decode failed ({}). Deleting corrupt snapshot and starting fresh.",
               e
            );
            let _ = tokio::fs::remove_file(&path).await;
            Ok(EventsSnapshot::default())
         }
      }
   }

   pub async fn save(&self, chain_id: u64, snapshot: EventsSnapshot) -> Result<(), anyhow::Error> {
      let dir = &self.cache_dir;
      tokio::fs::create_dir_all(dir).await?;

      let path = dir.join(self.filename(chain_id));

      let bytes = encode_to_vec(&snapshot, bincode_next::config::standard())
         .map_err(|e| anyhow!("bincode encode error: {}", e))?;

      tokio::fs::write(path, bytes).await?;
      Ok(())
   }
}
