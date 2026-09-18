use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
use tracing::{debug, error, info, warn};

use super::types::*;
use crate::indexer::syncer::{self, SyncerError, snapshot::SnapshotLoader};

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

   pub async fn latest_block(&self) -> Result<u64, SyncerError> {
      Ok(self.fetch_latest_block().await?)
   }

   pub async fn sync(
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

      // Snapshot coverage splits tip vs historical. Tip always appends (one chunk).
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
            if let Err(e) = loader.append(self.chain_id, &delta, to_block, None).await {
               warn!("Failed to append event snapshot: {}", e);
            }
         }

         return Ok(delta);
      }

      debug!(
         "Historical/cold sync {}-{} (snapshot_block={})",
         from_block, to_block, snapshot_block
      );

      let coverage = if let Some(loader) = &self.snapshot_loader {
         match loader.coverage(self.chain_id).await {
            Ok(c) => c,
            Err(e) => {
               warn!("Failed to load event snapshot coverage: {}", e);
               Default::default()
            }
         }
      } else {
         Default::default()
      };
      let events_block = coverage.block_number;

      let mut events = if let Some(loader) = &self.snapshot_loader {
         if events_block > 0 {
            loader
               .load_range(
                  self.chain_id,
                  from_block,
                  to_block.min(events_block),
               )
               .await?
         } else {
            Vec::new()
         }
      } else {
         Vec::new()
      };

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
         if let Some(loader) = &self.snapshot_loader {
            if events_block > 0 && to_block > events_block {
               if let Err(e) = loader.advance_meta(self.chain_id, to_block).await {
                  warn!("Failed to advance event snapshot meta: {}", e);
               }
            }
         }
         return Ok(events);
      }

      events.extend(delta.iter().cloned());

      if let Some(loader) = &self.snapshot_loader {
         // Subsquid is not used by the running app today, but the standalone
         // `zeus-railgun-snapshot` CLI (`--source subsquid`) still exercises this
         // path, so it stays functional. Seeding mirrors RpcSyncer
         // (coverage_start = from_block) so a fresh snapshot never claims history
         // it did not fetch; the previous blob format hardcoded 0 here.
         let seed = if events_block == 0 {
            Some(from_block)
         } else {
            None
         };
         if let Err(e) = loader.append(self.chain_id, &delta, to_block, seed).await {
            error!("Failed to save event snapshot: {}", e);
         }
      }

      Ok(events)
   }

   pub async fn sync_operations(
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
   async fn fetch_latest_block(&self) -> Result<u64, SubsquidSyncerError> {
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
