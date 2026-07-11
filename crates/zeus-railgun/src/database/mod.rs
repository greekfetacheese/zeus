pub mod redb;
//pub mod fs;
pub mod memory;
pub mod railgun_db;

pub use railgun_db::RailgunDB;
pub use redb::RedbDatabase;

/// Key-value database interface.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait Database: crate::MaybeSend {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DatabaseError>;
    async fn set(&self, key: &[u8], value: &[u8]) -> Result<(), DatabaseError>;
    async fn delete(&self, key: &[u8]) -> Result<(), DatabaseError>;
    async fn compact(&self) -> Result<bool, DatabaseError> {
        Ok(false)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u32),
    #[error("Storage error: {0}")]
    StorageError(String),
}