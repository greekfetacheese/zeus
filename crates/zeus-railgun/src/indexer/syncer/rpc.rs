use alloy_primitives::Address;
use alloy_provider::{DynProvider, Provider, network::Ethereum};
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log as RpcLog};
use alloy_sol_types::SolEvent;
use std::sync::Arc;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::{debug, error, warn};

use crate::{
   abi::{legacy::RailgunLegacy, railgun::RailgunSmartWallet},
   indexer::{
      parse_legacy_commitment_batch, parse_legacy_generated_commitment_batch,
      parse_legacy_nullifiers, parse_legacy_shield, parse_legacy_transact, parse_legacy_unshield,
      parse_nullified, parse_shield, parse_transact,
      syncer::{
         SyncEvent, SyncerError, UtxoSyncer,
         snapshot::{EventsSnapshot, SnapshotLoader},
      },
   },
};

/// @greekfetacheese:
///
/// This block range seems to work with a paid rpc
/// not really sure if it can work with free nodes
///
/// Ideally we want a runtime adjustment but the issue is if we increase the block range
/// the rpc provider doesnt error out instead it doesnt return all the events for the specified
/// block range.
///
/// So its impossible to find out the block_range limit for each provider
const DEFAULT_BLOCK_RANGE: u64 = 5_000;

/// An implementation of a syncer that uses a Json RPC client
///
/// and fetches all the `SyncEvent` from the Railgun contract on-chain.
///
/// Requires an archive node.
pub struct RpcSyncer {
   /// Type-erased provider so it can be swapped at runtime via [`UtxoSyncer::set_provider`].
   ///
   /// Stored behind a shared `Mutex` so the swap works through `Arc<dyn UtxoSyncer>`
   /// (which only offers `&self`).
   provider: Arc<Mutex<DynProvider<Ethereum>>>,
   chain_id: u64,
   railgun_address: Address,
   syncing: Arc<Mutex<bool>>,
   concurrency: Arc<Mutex<usize>>,
   block_range: Arc<Mutex<u64>>,
   snapshot_loader: Option<SnapshotLoader>,
}

impl RpcSyncer {
   pub fn new(
      provider: impl Provider<Ethereum> + 'static,
      chain_id: u64,
      railgun_address: Address,
   ) -> Self {
      Self {
         provider: Arc::new(Mutex::new(DynProvider::new(provider))),
         chain_id,
         railgun_address,
         syncing: Arc::new(Mutex::new(false)),
         concurrency: Arc::new(Mutex::new(2)),
         block_range: Arc::new(Mutex::new(DEFAULT_BLOCK_RANGE)),
         snapshot_loader: None,
      }
   }

   pub fn with_snapshot_loader(mut self, snapshot_loader: SnapshotLoader) -> Self {
      self.snapshot_loader = Some(snapshot_loader);
      self
   }

   pub async fn set_provider(&self, provider: DynProvider<Ethereum>) {
      *self.provider.lock().await = provider;
   }

   pub async fn is_syncing(&self) -> bool {
      *self.syncing.lock().await
   }

   pub async fn set_syncing(&self, syncing: bool) {
      *self.syncing.lock().await = syncing;
   }

   pub async fn concurrency(&self) -> usize {
      *self.concurrency.lock().await
   }

   pub async fn set_concurrency(&self, concurrency: usize) {
      *self.concurrency.lock().await = concurrency;
   }

   pub async fn block_range(&self) -> u64 {
      *self.block_range.lock().await
   }

   pub async fn set_block_range(&self, block_range: u64) {
      *self.block_range.lock().await = block_range;
   }

   async fn get_logs(&self, from_block: u64, to_block: u64) -> Result<Vec<RpcLog>, SyncerError> {
      debug!(
         "Fetching logs from block {} to {}",
         from_block, to_block
      );

      let address = self.railgun_address;
      let concurrency = self.concurrency().await;
      let block_range = self.block_range().await;

      let filter = Filter::new()
         .address(address)
         .from_block(BlockNumberOrTag::Number(from_block))
         .to_block(BlockNumberOrTag::Number(to_block));

      let logs = Arc::new(Mutex::new(Vec::new()));
      let semaphore = Arc::new(Semaphore::new(concurrency));
      let client = self.provider.lock().await.clone();

      let mut tasks: Vec<JoinHandle<Result<(), SyncerError>>> = Vec::new();

      if to_block.saturating_sub(from_block) > block_range {
         let mut start_block = from_block;

         while start_block <= to_block {
            let end_block = std::cmp::min(start_block + block_range, to_block);
            let client = client.clone();
            let logs_clone = Arc::clone(&logs);
            let filter_clone = filter.clone();
            let semaphore = semaphore.clone();

            let task: tokio::task::JoinHandle<Result<(), SyncerError>> = tokio::spawn(async move {
               let _permit = semaphore.acquire_owned().await.map_err(SyncerError::new)?;
               debug!(
                  "Quering Logs for block range: {} - {}",
                  start_block, end_block
               );

               let local_filter = filter_clone
                  .from_block(BlockNumberOrTag::Number(start_block))
                  .to_block(BlockNumberOrTag::Number(end_block));

               // TODO: Add retry logic, if one call fails the whole sync fails
               let log_chunk = client.get_logs(&local_filter).await.map_err(SyncerError::new)?;
               let mut logs_lock = logs_clone.lock().await;
               logs_lock.extend(log_chunk);
               Ok(())
            });

            tasks.push(task);
            start_block = end_block + 1;
         }

         for task in tasks {
            match task.await {
               Ok(_) => {}
               Err(e) => {
                  error!("Error fetching logs: {:?}", e);
               }
            }
         }

         return Ok(Arc::try_unwrap(logs).unwrap().into_inner());
      }

      let logs = client.get_logs(&filter).await.map_err(SyncerError::new)?;
      Ok(logs)
   }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl UtxoSyncer for RpcSyncer {
   async fn latest_block(&self) -> Result<u64, SyncerError> {
      let client = self.provider.lock().await.clone();
      let latest = client.get_block_number().await.map_err(|e| SyncerError::new(e))?;
      Ok(latest)
   }

   async fn set_provider(&self, provider: DynProvider<Ethereum>) {
      let provider_cell = self.provider.clone();
      *provider_cell.lock().await = provider;
   }

   async fn sync(&self, from_block: u64, to_block: u64) -> Result<Vec<SyncEvent>, SyncerError> {
      if from_block > to_block {
         return Ok(vec![]);
      }

      if self.is_syncing().await {
         debug!("Syncer is already syncing");
         return Ok(vec![]);
      }

      debug!(
         "Starting RPC sync from {} to {}",
         from_block, to_block
      );

      self.set_syncing(true).await;
      let result = self.sync_inner(from_block, to_block).await;
      self.set_syncing(false).await;
      result
   }
}

impl RpcSyncer {
   async fn sync_inner(
      &self,
      from_block: u64,
      to_block: u64,
   ) -> Result<Vec<SyncEvent>, SyncerError> {
      // Snapshot coverage (events present in the on-disk blob). Used only to decide whether
      // this call is a tip delta (no snapshot I/O) vs historical replay (load blob).
      // Tip syncs intentionally do NOT load/append/rewrite the multi‑MB events file —
      // trees + account state are already persisted by the indexer; the snapshot is only
      // a bootstrap cache for low-from historical catch-up (new signers).
      let snapshot_block = if let Some(loader) = &self.snapshot_loader {
         match loader.load_meta(self.chain_id).await {
            Ok(b) => b,
            Err(e) => {
               warn!(
                  "Failed to load event snapshot meta (will start fresh): {}",
                  e
               );
               0
            }
         }
      } else {
         0
      };

      debug!("Latest snapshot block {}", snapshot_block);

      // Pure tip path: caller already past the snapshot. Fetch only the requested
      // range and never touch the snapshot file (empty or non-empty delta).
      if from_block > snapshot_block {
         debug!(
            "Tip sync {}-{}: no events-snapshot I/O",
            from_block, to_block
         );
         let logs = self.get_logs(from_block, to_block).await?;
         let events = Self::parse_logs(logs)?;
         debug!("Tip delta events len {}", events.len());
         return Ok(events);
      }

      // Historical / catch-up path: serve cached events then fetch the tail.
      let (mut full_events, events_block) = if let Some(loader) = &self.snapshot_loader {
         match loader.load(self.chain_id).await {
            Ok(s) => (s.events, s.block_number),
            Err(e) => {
               warn!(
                  "Failed to load event snapshot (will start fresh): {}",
                  e
               );
               (Vec::new(), 0)
            }
         }
      } else {
         (Vec::new(), 0)
      };

      let mut events: Vec<SyncEvent> = full_events
         .iter()
         .filter(|ev| {
            let b = ev.block_number();
            b >= from_block && b <= to_block
         })
         .cloned()
         .collect();

      // Use the blob's own coverage, not a tip-advanced meta watermark.
      let fetch_from = events_block.saturating_add(1).max(from_block);
      debug!(
         "Historical fetch delta from {} to {} (events_block={})",
         fetch_from, to_block, events_block
      );

      if fetch_from > to_block {
         return Ok(events);
      }

      let logs = self.get_logs(fetch_from, to_block).await?;
      let delta = Self::parse_logs(logs)?;
      debug!("Delta Events len {}", delta.len());

      if delta.is_empty() {
         return Ok(events);
      }

      events.extend(delta.iter().cloned());

      if let Some(loader) = &self.snapshot_loader {
         full_events.extend(delta);
         debug!("Full Events len {}", full_events.len());
         let updated = EventsSnapshot {
            events: full_events,
            block_number: to_block,
         };
         if let Err(e) = loader.save(self.chain_id, updated).await {
            warn!("Failed to save event snapshot: {}", e);
         }
      }

      Ok(events)
   }

   fn parse_logs(logs: Vec<RpcLog>) -> Result<Vec<SyncEvent>, SyncerError> {
      let mut events = Vec::new();

      for log in logs {
         let block_number = log.block_number.unwrap_or(0);
         let block_timestamp = log.block_timestamp.unwrap_or(0);
         let tx_hash = log.transaction_hash.unwrap_or_default();
         let topic = log.topics().first().clone().unwrap_or_default();

         if let Ok(decoded) = <RailgunSmartWallet::Shield as SolEvent>::decode_log(&log.inner) {
            let mut shield_events = parse_shield(&decoded.data, block_number)?;
            events.append(&mut shield_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Transact as SolEvent>::decode_log(&log.inner) {
            let mut tx_events = parse_transact(&decoded.data, block_timestamp)?;
            events.append(&mut tx_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Nullified as SolEvent>::decode_log(&log.inner) {
            let mut null_events = parse_nullified(&decoded.data, block_timestamp)?;
            events.append(&mut null_events);
            continue;
         }

         // Legacy events
         if let Ok(decoded) = <RailgunLegacy::CommitmentBatch as SolEvent>::decode_log(&log.inner) {
            let mut legacy_events = parse_legacy_commitment_batch(&decoded.data, block_number)?;
            events.append(&mut legacy_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Nullifiers as SolEvent>::decode_log(&log.inner) {
            let mut null_events = parse_legacy_nullifiers(&decoded.data, block_timestamp)?;
            events.append(&mut null_events);
            continue;
         }

         if let Ok(decoded) =
            <RailgunLegacy::GeneratedCommitmentBatch as SolEvent>::decode_log(&log.inner)
         {
            let mut legacy_events =
               parse_legacy_generated_commitment_batch(&decoded.data, block_number)?;
            events.append(&mut legacy_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Transact as SolEvent>::decode_log(&log.inner) {
            let mut tx_events = parse_legacy_transact(&decoded.data, block_timestamp)?;
            events.append(&mut tx_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Shield as SolEvent>::decode_log(&log.inner) {
            let mut shield_events = parse_legacy_shield(&decoded.data, block_number)?;
            events.append(&mut shield_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Unshield as SolEvent>::decode_log(&log.inner) {
            let _ = parse_legacy_unshield(&decoded.data, block_number); // parsed for completeness
            continue;
         }

         debug!(
            "Unknown Log block_number: {} tx_hash: {} topic: {}",
            block_number, tx_hash, topic
         );
      }

      Ok(events)
   }
}
