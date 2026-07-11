pub mod normalize_tree_position;
pub mod rpc;
pub mod subsquid;
pub mod subsquid_types;
pub mod types;

pub use rpc::RpcSyncer;
pub use types::*;

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
