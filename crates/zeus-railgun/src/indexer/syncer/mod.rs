pub mod normalize_tree_position;
pub mod subsquid;
pub mod subsquid_types;
pub mod types;

pub use types::*;

use alloy_primitives::Address;
use alloy_provider::{Provider, network::Ethereum};
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log as RpcLog};
use alloy_sol_types::SolEvent;
use std::sync::Arc;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};

use crate::{
   abi::railgun::RailgunSmartWallet,
   indexer::{parse_nullified, parse_shield, parse_transact},
};

/// Syncers that fetch full operation data.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait TxidSyncer: crate::MaybeSend {
   async fn latest_block(&self) -> Result<u64, SyncerError>;
   async fn sync(&self, from_block: u64, to_block: u64) -> Result<Vec<Operation>, SyncerError>;
}

/// Syncers that emit note-level events.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait UtxoSyncer: crate::MaybeSend {
   async fn latest_block(&self) -> Result<u64, SyncerError>;
   async fn sync(&self, from_block: u64, to_block: u64) -> Result<Vec<SyncEvent>, SyncerError>;
}

/// An implementation of a syncer that uses a Json RPC client
///
/// and fetches all the `SyncEvent` from the Railgun contract on-chain.
///
/// Requires an archive node.
pub struct Syncer<P: Provider<Ethereum>> {
   provider: P,
   railgun_address: Address,
   syncing: Arc<Mutex<bool>>,
   concurrency: Arc<Mutex<usize>>,
   block_range: Arc<Mutex<u64>>,
}

impl<P: Provider<Ethereum> + Clone + 'static> Syncer<P> {
   pub fn new(provider: P, railgun_address: Address) -> Self {
      Self {
         provider,
         railgun_address,
         syncing: Arc::new(Mutex::new(false)),
         concurrency: Arc::new(Mutex::new(2)),
         block_range: Arc::new(Mutex::new(30_000)),
      }
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
      tracing::debug!(
         "Fetching logs from block {} to {}",
         from_block,
         to_block
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
               tracing::debug!(
                  "Quering Logs for block range: {} - {}",
                  start_block,
                  end_block
               );

               let local_filter = filter_clone
                  .from_block(BlockNumberOrTag::Number(start_block))
                  .to_block(BlockNumberOrTag::Number(end_block));

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
                  tracing::error!("Error fetching logs: {:?}", e);
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
impl<P: Provider<Ethereum> + Clone + 'static> UtxoSyncer for Syncer<P> {
   async fn latest_block(&self) -> Result<u64, SyncerError> {
      let latest = self.provider.get_block_number().await.map_err(|e| SyncerError::new(e))?;
      Ok(latest)
   }

   async fn sync(&self, from_block: u64, to_block: u64) -> Result<Vec<SyncEvent>, SyncerError> {
      if self.is_syncing().await {
         tracing::info!("Syncer is already syncing");
         return Ok(vec![]);
      }

      self.set_syncing(true).await;

      let logs = self.get_logs(from_block, to_block).await?;

      let mut events = Vec::new();

      for log in logs {
         let block_number = log.block_number.unwrap_or(0);
         let block_timestamp = log.block_timestamp.unwrap_or(0);

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
      }

      self.set_syncing(false).await;

      Ok(events)
   }
}
