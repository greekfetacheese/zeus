use alloy_primitives::Address;
use alloy_provider::{Provider, network::Ethereum};
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log as RpcLog};
use alloy_sol_types::SolEvent;
use std::sync::Arc;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::{error, debug, warn};

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
pub struct RpcSyncer<P: Provider<Ethereum>> {
   provider: P,
   chain_id: u64,
   railgun_address: Address,
   syncing: Arc<Mutex<bool>>,
   concurrency: Arc<Mutex<usize>>,
   block_range: Arc<Mutex<u64>>,
   snapshot_loader: Option<SnapshotLoader>,
}

impl<P: Provider<Ethereum> + Clone + 'static> RpcSyncer<P> {
   pub fn new(provider: P, chain_id: u64, railgun_address: Address) -> Self {
      Self {
         provider,
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
      let client = self.provider.clone();

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
                  start_block,
                  end_block
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
impl<P: Provider<Ethereum> + Clone + 'static> UtxoSyncer for RpcSyncer<P> {
   async fn latest_block(&self) -> Result<u64, SyncerError> {
      let latest = self.provider.get_block_number().await.map_err(|e| SyncerError::new(e))?;
      Ok(latest)
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

      // Load any previously saved snapshot of historical SyncEvents.
      // This allows very fast "from scratch" syncs for new signers.
      let mut snapshot = EventsSnapshot::default();
      if let Some(loader) = &self.snapshot_loader {
         match loader.load(self.chain_id).await {
            Ok(s) => snapshot = s,
            Err(e) => warn!(
               "Failed to load event snapshot (will start fresh): {}",
               e
            ),
         }
      }

      let mut events = Vec::new();
      let cached_events = std::mem::take(&mut snapshot.events);
      let snapshot_block = snapshot.block_number;
      debug!("Latest snapshot block {}", snapshot_block);

      let missing = to_block.saturating_sub(snapshot_block);
      debug!("Missing blocks {} ", missing);

      // Serve any relevant historical events from the snapshot
      for ev in &cached_events {
         let b = ev.block_number();
         if b >= from_block && b <= to_block {
            events.push(ev.clone());
         }
      }

      // Only fetch the delta we don't have yet
      let fetch_from = snapshot_block.saturating_add(1).max(from_block);
      debug!(
         "Fetching delta from {} to {}",
         fetch_from, to_block
      );

      if fetch_from <= to_block {
         let logs = self.get_logs(fetch_from, to_block).await?;

         let mut delta = Vec::new();

         for log in logs {
            let block_number = log.block_number.unwrap_or(0);
            let block_timestamp = log.block_timestamp.unwrap_or(0);
            let tx_hash = log.transaction_hash.unwrap_or_default();
            let topic = log.topics().first().clone().unwrap_or_default();

            if let Ok(decoded) = <RailgunSmartWallet::Shield as SolEvent>::decode_log(&log.inner) {
               let mut shield_events = parse_shield(&decoded.data, block_number)?;
               delta.append(&mut shield_events);
               continue;
            }

            if let Ok(decoded) = <RailgunSmartWallet::Transact as SolEvent>::decode_log(&log.inner)
            {
               let mut tx_events = parse_transact(&decoded.data, block_timestamp)?;
               delta.append(&mut tx_events);
               continue;
            }

            if let Ok(decoded) = <RailgunSmartWallet::Nullified as SolEvent>::decode_log(&log.inner)
            {
               let mut null_events = parse_nullified(&decoded.data, block_timestamp)?;
               delta.append(&mut null_events);
               continue;
            }

            // Legacy events
            if let Ok(decoded) =
               <RailgunLegacy::CommitmentBatch as SolEvent>::decode_log(&log.inner)
            {
               let mut legacy_events = parse_legacy_commitment_batch(&decoded.data, block_number)?;
               delta.append(&mut legacy_events);
               continue;
            }

            if let Ok(decoded) = <RailgunLegacy::Nullifiers as SolEvent>::decode_log(&log.inner) {
               let mut null_events = parse_legacy_nullifiers(&decoded.data, block_timestamp)?;
               delta.append(&mut null_events);
               continue;
            }

            if let Ok(decoded) =
               <RailgunLegacy::GeneratedCommitmentBatch as SolEvent>::decode_log(&log.inner)
            {
               let mut legacy_events =
                  parse_legacy_generated_commitment_batch(&decoded.data, block_number)?;
               delta.append(&mut legacy_events);
               continue;
            }

            if let Ok(decoded) = <RailgunLegacy::Transact as SolEvent>::decode_log(&log.inner) {
               let mut tx_events = parse_legacy_transact(&decoded.data, block_timestamp)?;
               delta.append(&mut tx_events);
               continue;
            }

            if let Ok(decoded) = <RailgunLegacy::Shield as SolEvent>::decode_log(&log.inner) {
               let mut shield_events = parse_legacy_shield(&decoded.data, block_number)?;
               delta.append(&mut shield_events);
               continue;
            }

            if let Ok(decoded) = <RailgunLegacy::Unshield as SolEvent>::decode_log(&log.inner) {
               let _ = parse_legacy_unshield(&decoded.data, block_number); // parsed for completeness
               continue;
            }

            debug!(
               "Unknown Log block_number: {} tx_hash: {} topic: {}",
               block_number,
               tx_hash,
               topic
            );
         }

         debug!("Delta Events len {}", delta.len());

         events.extend(delta.clone());

         // Merge delta into the full historical list and persist
         let mut full_events = cached_events;
         full_events.extend(delta);
         debug!("Full Events len {}", full_events.len());

         let updated = EventsSnapshot {
            events: full_events,
            block_number: to_block,
         };

         if let Some(loader) = &self.snapshot_loader {
            if let Err(e) = loader.save(self.chain_id, updated).await {
               warn!("Failed to save event snapshot: {}", e);
            }
         }
      } else {
         // No delta to fetch. We still "covered" up to to_block via the snapshot.
         // (We don't rewrite the snapshot file here to avoid unnecessary IO.)
      }

      self.set_syncing(false).await;

      Ok(events)
   }
}
