use alloy_provider::{DynProvider, network::Ethereum};
use serde::{Serialize, de::DeserializeOwned};
use tracing::{debug, error, info, warn};
use web_time::Duration;

use super::types::*;
use crate::indexer::syncer::{
   self, SyncerError, TxidSyncer, UtxoSyncer,
   snapshot::{EventsSnapshot, SnapshotLoader},
};

/// Subsquid UTXO & TXID syncer.
///
/// Railgun maintains an official index for each supported chain. Syncing from subsquid
/// is significantly faster than syncing from the chain directly, as we can fetch much larger ranges
/// of events directly via graphql.
pub struct SubsquidSyncer {
   client: reqwest::Client,
   url: String,
   batch_size: u64,
   max_retries: usize,
   retry_delay: Duration,
   chain_id: u64,

   /// Override for latest block, used for testing to sync to a specific block.
   latest_block_override: Option<u64>,

   /// Snapshot loader for storing synced events.
   snapshot_loader: Option<SnapshotLoader>,
}

const COMMITMENTS_QUERY: &str = include_str!("./graphql/commitments.graphql");
const NULLIFIERS_QUERY: &str = include_str!("./graphql/nullifiers.graphql");
const OPERATIONS_QUERY: &str = include_str!("./graphql/operations.graphql");
const BLOCK_NUMBER_QUERY: &str = include_str!("./graphql/block_number.graphql");

impl SubsquidSyncer {
   pub fn new(url: impl Into<String>, chain_id: u64) -> Self {
      Self {
         client: reqwest::Client::new(),
         url: url.into(),
         batch_size: 20000,
         max_retries: 10,
         retry_delay: Duration::from_secs(1),
         latest_block_override: None,
         chain_id,
         snapshot_loader: None,
      }
   }

   /// Sets the latest block override, which causes the syncer to only sync
   /// up to this block. Used in testing to sync against chain forks.
   #[cfg(test)]
   pub fn with_latest_block(mut self, latest_block: u64) -> Self {
      self.latest_block_override = Some(latest_block);
      self
   }

   pub fn with_snapshot_loader(mut self, snapshot_loader: SnapshotLoader) -> Self {
      self.snapshot_loader = Some(snapshot_loader);
      self
   }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl UtxoSyncer for SubsquidSyncer {
   async fn latest_block(&self) -> Result<u64, SyncerError> {
      Ok(self.latest_block().await?)
   }

   async fn sync(
      &self,
      from_block: u64,
      to_block: u64,
   ) -> Result<Vec<syncer::SyncEvent>, SyncerError> {
      if from_block > to_block {
         return Ok(vec![]);
      }

      debug!(
         "Starting Subsquid note sync {}-{}",
         from_block, to_block
      );

      // Snapshot coverage used only to split tip vs historical. Tip path does no
      // snapshot I/O — see RpcSyncer for rationale.
      let snapshot_block = if let Some(loader) = &self.snapshot_loader {
         match loader.load_meta(self.chain_id).await {
            Ok(b) => b,
            Err(e) => {
               warn!("Failed to load event snapshot meta: {}", e);
               0
            }
         }
      } else {
         0
      };

      debug!("Latest snapshot block {}", snapshot_block);

      if SnapshotLoader::is_tip_sync(snapshot_block, from_block) {
         debug!(
            "Tip sync {}-{} (snapshot_block={})",
            from_block, to_block, snapshot_block
         );
         let mut delta = Vec::new();
         let mut commitments = self.commitments(from_block, to_block).await?;
         delta.append(&mut commitments);
         let mut nullifiers = self.nullifiers(from_block, to_block).await?;
         delta.append(&mut nullifiers);
         debug!("Tip delta events len {}", delta.len());

         if let Some(loader) = &self.snapshot_loader {
            if SnapshotLoader::should_refresh(snapshot_block, to_block) {
               if let Err(e) =
                  self.refresh_events_snapshot(loader, from_block, to_block, &delta).await
               {
                  warn!("Failed to refresh events snapshot: {}", e);
               }
            }
         }

         return Ok(delta);
      }

      debug!(
         "Historical/cold sync {}-{} (snapshot_block={})",
         from_block, to_block, snapshot_block
      );

      let (mut full_events, events_block) = if let Some(loader) = &self.snapshot_loader {
         match loader.load(self.chain_id).await {
            Ok(s) => (s.events, s.block_number),
            Err(e) => {
               warn!("Failed to load event snapshot: {}", e);
               (Vec::new(), 0)
            }
         }
      } else {
         (Vec::new(), 0)
      };

      let mut events: Vec<syncer::SyncEvent> = full_events
         .iter()
         .filter(|ev| {
            let b = ev.block_number();
            b >= from_block && b <= to_block
         })
         .cloned()
         .collect();

      let fetch_from = if events_block == 0 {
         from_block
      } else {
         events_block.saturating_add(1).max(from_block)
      };
      debug!(
         "Historical fetch delta from {} to {} (events_block={})",
         fetch_from, to_block, events_block
      );

      if fetch_from > to_block {
         return Ok(events);
      }

      let mut delta = Vec::new();
      let mut commitments = self.commitments(fetch_from, to_block).await?;
      delta.append(&mut commitments);
      let mut nullifiers = self.nullifiers(fetch_from, to_block).await?;
      delta.append(&mut nullifiers);

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
            error!("Failed to save event snapshot: {}", e);
         }
      }

      Ok(events)
   }

   async fn set_provider(&self, _provider: DynProvider<Ethereum>) {
      // SubsquidSyncer uses a reqwest client, not an alloy provider; no-op.
   }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl TxidSyncer for SubsquidSyncer {
   async fn latest_block(&self) -> Result<u64, SyncerError> {
      Ok(self.latest_block().await?)
   }

   async fn sync(
      &self,
      from_block: u64,
      to_block: u64,
   ) -> Result<Vec<syncer::Operation>, SyncerError> {
      if from_block > to_block {
         return Ok(vec![]);
      }

      debug!(
         "Starting Subsquid operation sync {}-{}",
         from_block, to_block
      );

      let operations = self.operations(from_block, to_block).await?;
      Ok(operations)
   }
}

impl SubsquidSyncer {
   /// Extend the on-disk events snapshot up to `to_block` (full load + rewrite).
   ///
   /// See RpcSyncer::refresh_events_snapshot — same policy, Subsquid fetch.
   async fn refresh_events_snapshot(
      &self,
      loader: &SnapshotLoader,
      tip_from: u64,
      to_block: u64,
      tip_events: &[syncer::SyncEvent],
   ) -> Result<(), anyhow::Error> {
      let mut snapshot = loader
         .load(self.chain_id)
         .await
         .map_err(|e| anyhow::anyhow!("load snapshot for refresh: {}", e))?;

      let events_block = snapshot.block_number;

      debug!(
         "Refreshing events snapshot (subsquid): events_block={} tip_from={} to_block={} lag={}",
         events_block,
         tip_from,
         to_block,
         to_block.saturating_sub(events_block)
      );

      if events_block == 0 || snapshot.events.is_empty() {
         let updated = EventsSnapshot {
            events: tip_events.to_vec(),
            block_number: to_block,
         };
         loader.save(self.chain_id, updated).await?;
         info!(
            "Events snapshot bootstrapped from tip range {}-{} ({} events)",
            tip_from,
            to_block,
            tip_events.len()
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

      if tip_from > gap_from {
         let early_to = tip_from - 1;
         debug!(
            "Snapshot refresh fetching prefix {}-{}",
            gap_from, early_to
         );
         let mut commitments = self
            .commitments(gap_from, early_to)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
         delta.append(&mut commitments);
         let mut nullifiers = self
            .nullifiers(gap_from, early_to)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
         delta.append(&mut nullifiers);
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
      loader.save(self.chain_id, snapshot).await?;
      info!("Events snapshot refreshed to block {}", to_block);
      Ok(())
   }

   async fn latest_block(&self) -> Result<u64, SubsquidSyncerError> {
      if let Some(override_block) = self.latest_block_override {
         return Ok(override_block);
      }

      let data: BlockNumberResponse = self.post_retry(BLOCK_NUMBER_QUERY, ()).await?;
      let latest_block = data.transactions.first().map(|tx| tx.block_number).unwrap_or(0);
      Ok(latest_block)
   }

   async fn commitments(
      &self,
      from: u64,
      to: u64,
   ) -> Result<Vec<syncer::SyncEvent>, SubsquidSyncerError> {
      self
         .fetch_paged(
            "commitments",
            COMMITMENTS_QUERY,
            from,
            to,
            |data: CommitmentsResponse| {
               let last = data.commitments.last();
               let id = last.map(|c| c.id.clone()).unwrap_or_default();
               let block = last.map(|c| c.block_number).unwrap_or(0);
               let items = data.commitments.into_iter().map(syncer::SyncEvent::from).collect();
               (items, id, block)
            },
         )
         .await
   }

   async fn nullifiers(
      &self,
      from: u64,
      to: u64,
   ) -> Result<Vec<syncer::SyncEvent>, SubsquidSyncerError> {
      self
         .fetch_paged(
            "nullifiers",
            NULLIFIERS_QUERY,
            from,
            to,
            |data: NullifiersResponse| {
               let last = data.nullifiers.last();
               let id = last.map(|c| c.id.clone()).unwrap_or_default();
               let block = last.map(|c| c.block_number).unwrap_or(0);
               let items = data.nullifiers.into_iter().map(syncer::SyncEvent::from).collect();
               (items, id, block)
            },
         )
         .await
   }

   async fn operations(
      &self,
      from: u64,
      to: u64,
   ) -> Result<Vec<syncer::Operation>, SubsquidSyncerError> {
      self
         .fetch_paged(
            "operations",
            OPERATIONS_QUERY,
            from,
            to,
            |data: OperationsResponse| {
               let last = data.operations.last();
               let id = last.map(|c| c.id.clone()).unwrap_or_default();
               let block = last.map(|c| c.block_number).unwrap_or(0);
               let items = data.operations.into_iter().map(syncer::Operation::from).collect();
               (items, id, block)
            },
         )
         .await
   }

   async fn fetch_paged<R, T, F>(
      &self,
      name: &str,
      query: &'static str,
      from: u64,
      to: u64,
      map_fn: F,
   ) -> Result<Vec<T>, SubsquidSyncerError>
   where
      R: DeserializeOwned,
      F: Fn(R) -> (Vec<T>, String, u64), // Returns (items, last_id, last_block)
   {
      let mut id_gt = String::new();
      let mut all_items = Vec::new();

      loop {
         let vars = QueryVars {
            id_gt: id_gt.clone(),
            block_number_gte: from,
            block_number_lte: to,
            limit: self.batch_size,
         };

         let data: R = self.post_retry(query, vars).await?;
         let (items, last_id, last_block) = map_fn(data);

         if items.is_empty() {
            break;
         }

         id_gt = last_id;
         all_items.extend(items);

         info!(
            "{}/{} ({} {})",
            last_block,
            to,
            all_items.len(),
            name
         );
      }

      Ok(all_items)
   }

   async fn post_retry<V: Serialize, R: DeserializeOwned>(
      &self,
      query: &'static str,
      variables: V,
   ) -> Result<R, SubsquidSyncerError> {
      let body = GraphqlRequest { query, variables };

      let mut attempt = 0;
      loop {
         match self.post_graphql(&body).await {
            Ok(data) => return Ok(data),
            Err(e) => {
               attempt += 1;
               if attempt > self.max_retries {
                  return Err(e);
               }

               warn!(
                  "GraphQL request failed (attempt {}/{}): {}",
                  attempt, self.max_retries, e
               );
               tokio::time::sleep(self.retry_delay).await;
            }
         }
      }
   }

   async fn post_graphql<V: Serialize, R: DeserializeOwned>(
      &self,
      body: &GraphqlRequest<V>,
   ) -> Result<R, SubsquidSyncerError> {
      let resp = self.client.post(&self.url).json(&body).send().await?;
      if !resp.status().is_success() {
         return Err(SubsquidSyncerError::Request(
            resp.status(),
            resp.text().await.unwrap_or_default(),
         ));
      }

      let value: serde_json::Value = resp.json().await?;
      let graphql_resp: GraphqlResponse<R> = serde_json::from_value(value)?;
      if let Some(errors) = graphql_resp.errors {
         return Err(SubsquidSyncerError::GraphQL(
            errors.into_iter().map(|e| e.message).collect::<Vec<_>>().join("; "),
         ));
      }

      let Some(data) = graphql_resp.data else {
         return Err(SubsquidSyncerError::GraphQL(
            "No data in response".to_string(),
         ));
      };

      Ok(data)
   }
}

#[derive(Debug, thiserror::Error)]
enum SubsquidSyncerError {
   #[error("Serde error: {0}")]
   Serde(#[from] serde_json::Error),
   #[error("HTTP error: {0}")]
   Http(#[from] reqwest::Error),
   #[error("Request failed with status {0}: {1}")]
   Request(reqwest::StatusCode, String),
   #[error("GraphQL error: {0}")]
   GraphQL(String),
}

impl From<SubsquidSyncerError> for SyncerError {
   fn from(e: SubsquidSyncerError) -> Self {
      SyncerError::new(e)
   }
}
