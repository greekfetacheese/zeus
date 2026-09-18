use alloy_primitives::Address;
use alloy_provider::{DynProvider, Provider, network::Ethereum};
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log as RpcLog};
use alloy_sol_types::SolEvent;
use std::{sync::Arc, time::Duration};
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::{debug, warn};

use crate::{
   abi::{legacy::RailgunLegacy, railgun::RailgunSmartWallet},
   indexer::{
      parse_legacy_commitment_batch, parse_legacy_generated_commitment_batch,
      parse_legacy_nullifiers, parse_legacy_shield, parse_legacy_transact, parse_legacy_unshield,
      parse_nullified, parse_shield, parse_transact,
      syncer::{SyncEvent, SyncerError, snapshot::SnapshotLoader},
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
pub const DEFAULT_BLOCK_RANGE: u64 = 3_000;
pub const SEPOLIA_BLOCK_RANGE: u64 = 30_000;

pub const DEFAULT_CONCURRENCY: usize = 2;

/// Max blocks applied/held per sync step (memory bound). Independent of get_logs size.
pub const MAX_SYNC_WINDOW: u64 = 100_000;

/// Mainnet block after which get_logs ranges should not stay at the sparse-era size.
pub const MAINNET_DENSE_LOGS_BLOCK: u64 = 20_000_000;
pub const DENSE_LOGS_BLOCK_RANGE: u64 = 5_000;

pub const BLOCK_RANGE_DECREMENT: u64 = 500;
pub const MIN_BLOCK_RANGE: u64 = 100;

pub fn apply_dense_logs_cap(chain_id: u64, window_from: u64, block_range: u64) -> u64 {
   if chain_id == 1 && window_from >= MAINNET_DENSE_LOGS_BLOCK {
      block_range.min(DENSE_LOGS_BLOCK_RANGE)
   } else {
      block_range
   }
}

/// `Ok(new_range)` to retry; `Err(())` to abort (already at min).
pub fn shrink_block_range_on_invalid_root(block_range: u64) -> Result<u64, ()> {
   if block_range <= MIN_BLOCK_RANGE {
      return Err(());
   }
   Ok(block_range.saturating_sub(BLOCK_RANGE_DECREMENT).max(MIN_BLOCK_RANGE))
}

pub fn sync_window_end(window_from: u64, to_block: u64) -> u64 {
   window_from.saturating_add(MAX_SYNC_WINDOW - 1).min(to_block)
}

/// Result of [`RpcSyncer::fetch`]: events to apply, plus the RPC-only delta for later persist.
pub struct FetchedEvents {
   pub events: Vec<SyncEvent>,
   /// RPC-parsed events not already in the snapshot (prefix and/or tail).
   /// Empty when the window is a pure snapshot slice.
   pub rpc_delta: Vec<SyncEvent>,
   pub fetch_from: u64,
   pub fetch_to: u64,
   /// Snapshot covered tip at fetch time (`0` if none).
   pub events_block: u64,
}

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
#[derive(Clone)]
pub struct RpcSyncer {
   /// Type-erased provider so it can be swapped at runtime via [`RpcSyncer::set_provider`].
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

   pub fn chain_id(&self) -> u64 {
      self.chain_id
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

   pub async fn latest_block(&self) -> Result<u64, SyncerError> {
      let client = self.provider.lock().await.clone();
      let latest = client.get_block_number().await.map_err(|e| SyncerError::new(e))?;
      Ok(latest)
   }

   /// Fetch+parse only. Does not write the snapshot.
   ///
   /// Unlike [`RpcSyncer::sync`], this does not take the `is_syncing` flag — the
   /// indexer serializes syncs itself.
   pub async fn fetch(&self, from_block: u64, to_block: u64) -> Result<FetchedEvents, SyncerError> {
      if from_block > to_block {
         return Ok(FetchedEvents {
            events: Vec::new(),
            rpc_delta: Vec::new(),
            fetch_from: from_block,
            fetch_to: to_block,
            events_block: 0,
         });
      }

      debug!(
         "Starting RPC fetch from {} to {}",
         from_block, to_block
      );

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

      // Pure tip: from_block past snapshot coverage. Always append the delta
      // (one open chunk) — no full rewrite, no 30k-block refresh lag.
      if SnapshotLoader::is_tip_sync(snapshot_block, from_block) {
         debug!(
            "Tip sync {}-{} (snapshot_block={})",
            from_block, to_block, snapshot_block
         );
         let logs = self.get_logs(from_block, to_block).await?;
         let mut events = Self::parse_logs(logs)?;
         SnapshotLoader::sort_events(&mut events);
         debug!("Tip delta events len {}", events.len());

         return Ok(FetchedEvents {
            rpc_delta: events.clone(),
            events,
            fetch_from: from_block,
            fetch_to: to_block,
            events_block: snapshot_block,
         });
      }

      debug!(
         "Historical/cold sync {}-{} (snapshot_block={})",
         from_block, to_block, snapshot_block
      );

      let coverage = if let Some(loader) = &self.snapshot_loader {
         match loader.coverage(self.chain_id).await {
            Ok(c) => c,
            Err(e) => {
               warn!(
                  "Failed to load event snapshot coverage (will start fresh): {}",
                  e
               );
               Default::default()
            }
         }
      } else {
         Default::default()
      };
      let events_block = coverage.block_number;
      let coverage_start = coverage.coverage_start;

      let mut events: Vec<SyncEvent> = Vec::new();
      let mut rpc_delta: Vec<SyncEvent> = Vec::new();

      // Optional RPC prefix when the blob only covers [coverage_start, events_block]
      // and the caller needs blocks before coverage_start (legacy=0 skips).
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
            rpc_delta.extend(prefix.iter().cloned());
            events.extend(prefix);
         }
      }

      // Overlap with the requested range — load_range is the returned vec, no clone.
      if events_block > 0 {
         let slice_from = if coverage_start > 0 {
            from_block.max(coverage_start)
         } else {
            from_block
         };
         let slice_to = to_block.min(events_block);
         if slice_from <= slice_to {
            if let Some(loader) = &self.snapshot_loader {
               let mut ranged = loader.load_range(self.chain_id, slice_from, slice_to).await?;
               events.append(&mut ranged);
            }
         }
      }

      // Tail after snapshot tip (or full range when snapshot empty).
      let tail_from = if events_block == 0 {
         from_block
      } else {
         events_block.saturating_add(1).max(from_block)
      };

      debug!(
         "Historical fetch delta from {} to {} (events_block={} coverage_start={})",
         tail_from, to_block, events_block, coverage_start
      );

      if tail_from <= to_block {
         let logs = self.get_logs(tail_from, to_block).await?;
         let mut tail_delta = Self::parse_logs(logs)?;
         SnapshotLoader::sort_events(&mut tail_delta);
         debug!("Delta Events len {}", tail_delta.len());
         rpc_delta.extend(tail_delta.iter().cloned());
         events.extend(tail_delta);
      }

      SnapshotLoader::sort_events(&mut events);

      Ok(FetchedEvents {
         events,
         rpc_delta,
         fetch_from: from_block,
         fetch_to: to_block,
         events_block,
      })
   }

   pub async fn sync(&self, from_block: u64, to_block: u64) -> Result<Vec<SyncEvent>, SyncerError> {
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
      let result = async {
         let fetched = self.fetch(from_block, to_block).await?;
         if let Err(e) = self.persist_fetched(&fetched).await {
            warn!("Failed to save event snapshot: {}", e);
         }
         Ok(fetched.events)
      }
      .await;
      self.set_syncing(false).await;
      result
   }

   /// Persist RPC-fetched events after the caller has verified the window.
   ///
   /// Seed/append only — never lowers `coverage_start` (no rewrite). Prefix
   /// events below an existing snapshot are applied by the indexer but not
   /// written. No-op when this syncer has no snapshot loader.
   pub async fn persist_fetched(&self, fetched: &FetchedEvents) -> Result<(), anyhow::Error> {
      let Some(loader) = &self.snapshot_loader else {
         return Ok(());
      };

      if SnapshotLoader::is_tip_sync(fetched.events_block, fetched.fetch_from) {
         return loader
            .append(
               self.chain_id,
               &fetched.rpc_delta,
               fetched.fetch_to,
               None,
            )
            .await;
      }

      let tail_owned: Vec<SyncEvent>;
      let tail = if fetched.events_block == 0 {
         fetched.rpc_delta.as_slice()
      } else {
         tail_owned = fetched
            .rpc_delta
            .iter()
            .filter(|e| e.block_number() > fetched.events_block)
            .cloned()
            .collect();
         tail_owned.as_slice()
      };

      self
         .persist_historical_snapshot(
            loader,
            fetched.fetch_from,
            fetched.fetch_to,
            fetched.events_block,
            tail,
         )
         .await
   }
}

impl RpcSyncer {
   /// Update on-disk snapshot after a historical sync without creating coverage holes.
   async fn persist_historical_snapshot(
      &self,
      loader: &SnapshotLoader,
      from_block: u64,
      to_block: u64,
      events_block: u64,
      tail_delta: &[SyncEvent],
   ) -> Result<(), anyhow::Error> {
      if events_block > 0 {
         if to_block > events_block {
            if !tail_delta.is_empty() {
               loader.append(self.chain_id, tail_delta, to_block, None).await?;
            } else {
               loader.advance_meta(self.chain_id, to_block).await?;
            }
         }
         return Ok(());
      }

      // Fresh snapshot: seed with coverage_start = from_block. Never tip-bootstrap.
      if !tail_delta.is_empty() {
         debug!(
            "Seeding events snapshot {}-{} ({} events)",
            from_block,
            to_block,
            tail_delta.len()
         );
         loader
            .append(
               self.chain_id,
               tail_delta,
               to_block,
               Some(from_block),
            )
            .await?;
      }

      Ok(())
   }

   fn parse_logs(logs: Vec<RpcLog>) -> Result<Vec<SyncEvent>, SyncerError> {
      let mut events = Vec::new();

      for log in logs {
         let block_number = log.block_number.unwrap_or(0);
         let tx_hash = log.transaction_hash.unwrap_or_default();
         let timestamp = log.block_timestamp.unwrap_or(0);
         let topic = log.topics().first().cloned().unwrap_or_default();

         if let Ok(decoded) = <RailgunSmartWallet::Shield as SolEvent>::decode_log(&log.inner) {
            let mut shield_events = parse_shield(&decoded.data, block_number, timestamp, tx_hash)?;
            events.append(&mut shield_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Transact as SolEvent>::decode_log(&log.inner) {
            let mut tx_events = parse_transact(&decoded.data, block_number, timestamp, tx_hash)?;
            events.append(&mut tx_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Nullified as SolEvent>::decode_log(&log.inner) {
            let mut null_events = parse_nullified(&decoded.data, block_number, timestamp, tx_hash)?;
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
            let mut null_events =
               parse_legacy_nullifiers(&decoded.data, block_number, timestamp, tx_hash)?;
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
            let mut tx_events =
               parse_legacy_transact(&decoded.data, block_number, timestamp, tx_hash)?;
            events.append(&mut tx_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Shield as SolEvent>::decode_log(&log.inner) {
            let mut shield_events =
               parse_legacy_shield(&decoded.data, block_number, timestamp, tx_hash)?;
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

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn apply_dense_logs_cap_leaves_sparse_era_unchanged() {
      assert_eq!(
         apply_dense_logs_cap(1, 19_999_999, 30_000),
         30_000
      );
   }

   #[test]
   fn apply_dense_logs_cap_caps_mainnet_at_20m() {
      assert_eq!(apply_dense_logs_cap(1, 20_000_000, 30_000), 5_000);
   }

   #[test]
   fn apply_dense_logs_cap_does_not_raise_smaller_range() {
      assert_eq!(apply_dense_logs_cap(1, 20_000_000, 3_000), 3_000);
   }

   #[test]
   fn apply_dense_logs_cap_skips_sepolia() {
      assert_eq!(
         apply_dense_logs_cap(11_155_111, 20_000_000, 30_000),
         30_000
      );
   }

   #[test]
   fn shrink_block_range_on_invalid_root_decrements_then_floors() {
      assert_eq!(
         shrink_block_range_on_invalid_root(30_000),
         Ok(29_500)
      );
      assert_eq!(shrink_block_range_on_invalid_root(600), Ok(100));
      assert_eq!(shrink_block_range_on_invalid_root(100), Err(()));
   }

   #[test]
   fn sync_window_end_is_inclusive_and_clamps() {
      assert_eq!(
         sync_window_end(14_693_013, 30_000_000),
         14_793_012
      );
      assert_eq!(
         sync_window_end(29_950_000, 30_000_000),
         30_000_000
      );
   }
}
