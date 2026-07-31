use alloy_primitives::Address;
use alloy_provider::{DynProvider, Provider, network::Ethereum};
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log as RpcLog};
use alloy_sol_types::SolEvent;
use std::{sync::Arc, time::Duration};
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::{debug, info, warn};

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
pub const DEFAULT_BLOCK_RANGE: u64 = 5_000;
pub const SEPOLIA_BLOCK_RANGE: u64 = 30_000;

pub const DEFAULT_CONCURRENCY: usize = 2;

/// Transient RPC failures (rate limits, timeouts, 5xx) are common on archive
/// `eth_getLogs`. Retry per chunk so one flake doesn't abort the whole sync.
const GET_LOGS_MAX_RETRIES: usize = 5;
/// Base delay between retries; multiplied by attempt number (linear backoff).
const GET_LOGS_RETRY_BASE_DELAY_MS: u64 = 500;

fn default_block_range(chain: u64) -> u64 {
   match chain {
      1 => DEFAULT_BLOCK_RANGE,
      11155111 => SEPOLIA_BLOCK_RANGE,
      _ => DEFAULT_BLOCK_RANGE,
   }
}

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
      let block_range = Arc::new(Mutex::new(default_block_range(chain_id)));
      Self {
         provider: Arc::new(Mutex::new(DynProvider::new(provider))),
         chain_id,
         railgun_address,
         syncing: Arc::new(Mutex::new(false)),
         concurrency: Arc::new(Mutex::new(2)),
         block_range,
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

   /// `eth_getLogs` for a single filter with linear backoff retries.
   async fn get_logs_with_retry(
      client: &DynProvider<Ethereum>,
      filter: &Filter,
      from_block: u64,
      to_block: u64,
   ) -> Result<Vec<RpcLog>, SyncerError> {
      let mut attempt = 0usize;
      loop {
         match client.get_logs(filter).await {
            Ok(logs) => return Ok(logs),
            Err(e) => {
               attempt += 1;
               if attempt > GET_LOGS_MAX_RETRIES {
                  debug!(
                     "get_logs failed for blocks {}-{} after {} attempts: {}",
                     from_block, to_block, GET_LOGS_MAX_RETRIES, e
                  );
                  return Err(SyncerError::new(e));
               }

               let delay_ms = GET_LOGS_RETRY_BASE_DELAY_MS.saturating_mul(attempt as u64);
               debug!(
                  "get_logs failed for blocks {}-{} (attempt {}/{}): {} — retrying in {}ms",
                  from_block, to_block, attempt, GET_LOGS_MAX_RETRIES, e, delay_ms
               );
               tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
         }
      }
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

               let log_chunk =
                  Self::get_logs_with_retry(&client, &local_filter, start_block, end_block).await?;
               let mut logs_lock = logs_clone.lock().await;
               logs_lock.extend(log_chunk);
               Ok(())
            });

            tasks.push(task);
            start_block = end_block + 1;
         }

         // Fail the whole fetch if any chunk exhausted retries — partial logs
         // would silently produce wrong trees / missing nullifiers.
         for task in tasks {
            match task.await {
               Ok(Ok(())) => {}
               Ok(Err(e)) => return Err(e),
               Err(e) => {
                  debug!("get_logs task join error: {:?}", e);
                  return Err(SyncerError::new(e));
               }
            }
         }

         let mut logs = Arc::try_unwrap(logs).unwrap().into_inner();
         // Concurrent chunks finish out of order sort for deterministic parse
         // and stable leaf application.
         logs.sort_by(|a, b| {
            let ba = a.block_number.unwrap_or(0);
            let bb = b.block_number.unwrap_or(0);
            ba.cmp(&bb)
               .then_with(|| a.log_index.unwrap_or(0).cmp(&b.log_index.unwrap_or(0)))
         });
         return Ok(logs);
      }

      Self::get_logs_with_retry(&client, &filter, from_block, to_block).await
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

   async fn set_concurrency(&self, concurrency: usize) {
      self.set_concurrency(concurrency).await;
   }

   async fn set_block_range(&self, block_range: u64) {
      self.set_block_range(block_range).await;
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
      // Snapshot coverage decides tip delta (RPC only) vs historical (blob + optional RPC).
      // Tip syncs do not load the multi‑MB blob every tick — trees live in redb.
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

      // Pure tip: from_block past snapshot coverage.
      if SnapshotLoader::is_tip_sync(snapshot_block, from_block) {
         debug!(
            "Tip sync {}-{} (snapshot_block={})",
            from_block, to_block, snapshot_block
         );
         let logs = self.get_logs(from_block, to_block).await?;
         let mut events = Self::parse_logs(logs)?;
         SnapshotLoader::sort_events(&mut events);
         debug!("Tip delta events len {}", events.len());

         if let Some(loader) = &self.snapshot_loader {
            if SnapshotLoader::should_refresh(snapshot_block, to_block) {
               debug!(
                  "Refreshing events snapshot from block {} to {}",
                  snapshot_block, to_block
               );
               if let Err(e) =
                  self.refresh_events_snapshot(loader, from_block, to_block, &events).await
               {
                  warn!("Failed to refresh events snapshot: {}", e);
               }
            }
         }

         return Ok(events);
      }

      debug!(
         "Historical/cold sync {}-{} (snapshot_block={})",
         from_block, to_block, snapshot_block
      );

      // Historical / catch-up / cold-start: blob slice + RPC for holes and tail.
      let (mut full_events, events_block, coverage_start) =
         if let Some(loader) = &self.snapshot_loader {
            match loader.load(self.chain_id).await {
               Ok(s) => (s.events, s.block_number, s.coverage_start),
               Err(e) => {
                  warn!(
                     "Failed to load event snapshot (will start fresh): {}",
                     e
                  );
                  (Vec::new(), 0, 0)
               }
            }
         } else {
            (Vec::new(), 0, 0)
         };

      let mut events: Vec<SyncEvent> = Vec::new();

      // Optional RPC prefix when the blob only covers [coverage_start, events_block]
      // and the caller needs blocks before coverage_start (new field; legacy=0 skips).
      if coverage_start > 0 && from_block < coverage_start && events_block > 0 {
         let prefix_to = coverage_start.saturating_sub(1).min(to_block);
         if from_block <= prefix_to {
            debug!(
               "Historical RPC prefix {}-{} (blob coverage_start={})",
               from_block, prefix_to, coverage_start
            );
            let logs = self.get_logs(from_block, prefix_to).await?;
            let mut prefix = Self::parse_logs(logs)?;
            SnapshotLoader::sort_events(&mut prefix);
            events.extend(prefix);
         }
      }

      // Slice blob for the overlap with the requested range.
      if events_block > 0 {
         let slice_from = if coverage_start > 0 {
            from_block.max(coverage_start)
         } else {
            from_block
         };

         let slice_to = to_block.min(events_block);
         
         if slice_from <= slice_to {
            events.extend(
               full_events
                  .iter()
                  .filter(|ev| {
                     let b = ev.block_number();
                     b >= slice_from && b <= slice_to
                  })
                  .cloned(),
            );
         }
      }

      // Tail after blob tip (or full range when blob empty).
      let fetch_from = if events_block == 0 {
         from_block
      } else {
         events_block.saturating_add(1).max(from_block)
      };

      debug!(
         "Historical fetch delta from {} to {} (events_block={} coverage_start={})",
         fetch_from, to_block, events_block, coverage_start
      );

      let mut tail_delta = Vec::new();
      if fetch_from <= to_block {
         let logs = self.get_logs(fetch_from, to_block).await?;
         tail_delta = Self::parse_logs(logs)?;
         SnapshotLoader::sort_events(&mut tail_delta);
         debug!("Delta Events len {}", tail_delta.len());
         events.extend(tail_delta.iter().cloned());
      }

      SnapshotLoader::sort_events(&mut events);

      // Persist snapshot only when we can keep contiguous coverage honest.
      if let Some(loader) = &self.snapshot_loader {
         if let Err(e) = self
            .persist_historical_snapshot(
               loader,
               from_block,
               to_block,
               events_block,
               coverage_start,
               &mut full_events,
               &tail_delta,
            )
            .await
         {
            warn!("Failed to save event snapshot: {}", e);
         }
      }

      Ok(events)
   }

   /// Update on-disk snapshot after a historical sync without creating coverage holes.
   async fn persist_historical_snapshot(
      &self,
      loader: &SnapshotLoader,
      from_block: u64,
      to_block: u64,
      events_block: u64,
      coverage_start: u64,
      full_events: &mut Vec<SyncEvent>,
      tail_delta: &[SyncEvent],
   ) -> Result<(), anyhow::Error> {
      // Extending an existing contiguous blob with a successful tail fetch.
      if events_block > 0 && !full_events.is_empty() {
         if to_block > events_block {
            if !tail_delta.is_empty() {
               full_events.extend(tail_delta.iter().cloned());
               let updated = EventsSnapshot {
                  events: std::mem::take(full_events),
                  block_number: to_block,
                  coverage_start,
               };
               debug!(
                  "Full Events len {} (coverage_start={} .. {})",
                  updated.events.len(),
                  coverage_start,
                  to_block
               );
               loader.save(self.chain_id, updated).await?;
            } else {
               // Empty tail after successful RPC: advance watermark only.
               loader.save_meta(self.chain_id, to_block, coverage_start).await?;
            }
         }
         return Ok(());
      }

      // Fresh blob: seed with explicit coverage_start = from_block so later
      // cold starts from deployment RPC any prefix below that. Never claim
      // coverage starting at 0 unless we actually fetched from deployment.
      if events_block == 0 && !tail_delta.is_empty() {
         let updated = EventsSnapshot {
            events: tail_delta.to_vec(),
            block_number: to_block,
            coverage_start: from_block,
         };
         debug!(
            "Seeding events snapshot {}-{} ({} events)",
            from_block,
            to_block,
            updated.events.len()
         );
         loader.save(self.chain_id, updated).await?;
      }

      Ok(())
   }

   /// Extend the on-disk events snapshot up to `to_block` (full load + rewrite).
   ///
   /// Called rarely from the tip path when the snapshot lags by
   /// [`super::snapshot::EVENTS_SNAPSHOT_REFRESH_BLOCK_INTERVAL`]. Reuses
   /// `tip_events` already fetched for `tip_from..=to_block` so we only RPC the
   /// missing prefix after the blob's last covered block.
   ///
   /// Never bootstraps a full-coverage snapshot from tip-only events — that
   /// creates a hole below `tip_from` and poisons later historical resyncs.
   async fn refresh_events_snapshot(
      &self,
      loader: &SnapshotLoader,
      tip_from: u64,
      to_block: u64,
      tip_events: &[SyncEvent],
   ) -> Result<(), anyhow::Error> {
      let mut snapshot = loader
         .load(self.chain_id)
         .await
         .map_err(|e| anyhow::anyhow!("load snapshot for refresh: {}", e))?;

      let events_block = snapshot.block_number;

      debug!(
         "Refreshing events snapshot: events_block={} tip_from={} to_block={} lag={}",
         events_block,
         tip_from,
         to_block,
         to_block.saturating_sub(events_block)
      );

      // Empty blob: do NOT write tip-only data as if it covered history.
      // Cold/historical path is responsible for seeding a complete blob.
      if events_block == 0 || snapshot.events.is_empty() {
         debug!(
            "Skipping tip snapshot bootstrap with empty blob (avoid gappy coverage {}-{})",
            tip_from, to_block
         );
         return Ok(());
      }

      let gap_from = events_block.saturating_add(1);
      if gap_from > to_block {
         debug!(
            "Snapshot already covers to_block (events_block={})",
            events_block
         );
         return Ok(());
      }

      let mut delta = Vec::new();

      // Prefix between blob coverage and the tip fetch start.
      if tip_from > gap_from {
         let early_to = tip_from - 1;
         debug!(
            "Snapshot refresh fetching prefix {}-{}",
            gap_from, early_to
         );
         let logs = self
            .get_logs(gap_from, early_to)
            .await
            .map_err(|e| anyhow::anyhow!("fetch snapshot prefix: {}", e))?;
         delta = Self::parse_logs(logs).map_err(|e| anyhow::anyhow!("{}", e))?;
      }

      if tip_from >= gap_from {
         delta.extend(tip_events.iter().cloned());
      } else {
         delta.extend(tip_events.iter().filter(|ev| ev.block_number() >= gap_from).cloned());
      }

      debug!(
         "Snapshot refresh delta len {} (events_block {} -> {})",
         delta.len(),
         events_block,
         to_block
      );

      snapshot.events.extend(delta);
      snapshot.block_number = to_block;
      // Keep existing coverage_start (legacy 0 or real start).
      loader.save(self.chain_id, snapshot).await?;
      info!("Events snapshot refreshed to block {}", to_block);
      Ok(())
   }

   fn parse_logs(logs: Vec<RpcLog>) -> Result<Vec<SyncEvent>, SyncerError> {
      let mut events = Vec::new();

      for log in logs {
         let block_number = log.block_number.unwrap_or(0);
         let tx_hash = log.transaction_hash.unwrap_or_default();
         let topic = log.topics().first().cloned().unwrap_or_default();

         if let Ok(decoded) = <RailgunSmartWallet::Shield as SolEvent>::decode_log(&log.inner) {
            let mut shield_events = parse_shield(&decoded.data, block_number)?;
            events.append(&mut shield_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Transact as SolEvent>::decode_log(&log.inner) {
            let mut tx_events = parse_transact(&decoded.data, block_number)?;
            events.append(&mut tx_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Nullified as SolEvent>::decode_log(&log.inner) {
            let mut null_events = parse_nullified(&decoded.data, block_number)?;
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
            let mut null_events = parse_legacy_nullifiers(&decoded.data, block_number)?;
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
            let mut tx_events = parse_legacy_transact(&decoded.data, block_number)?;
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
