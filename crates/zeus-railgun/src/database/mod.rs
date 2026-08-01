pub mod crypto;
pub mod memory;
pub mod railgun_db;
pub mod redb;

pub use crypto::{ENCRYPTED_ENVELOPE_VERSION, RailgunDbKey};
pub use railgun_db::RailgunDB;
pub use redb::RedbDatabase;

/// Durability requested for a write batch.
///
/// - [`WriteDurability::Immediate`]: fsync before commit returns (default for notes/trees).
/// - [`WriteDurability::None`]: atomic in-process, may be lost on crash until a later
///   Immediate commit. Safe for pure watermarks that can be re-derived by re-sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WriteDurability {
   #[default]
   Immediate,
   None,
}

/// Accumulated puts/deletes applied in a single transaction when the backend supports it.
#[derive(Debug, Default, Clone)]
pub struct WriteBatch {
   pub puts: Vec<(Vec<u8>, Vec<u8>)>,
   pub deletes: Vec<Vec<u8>>,
}

impl WriteBatch {
   pub fn new() -> Self {
      Self::default()
   }

   pub fn is_empty(&self) -> bool {
      self.puts.is_empty() && self.deletes.is_empty()
   }

   pub fn put(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
      self.puts.push((key.into(), value.into()));
   }

   pub fn delete(&mut self, key: impl Into<Vec<u8>>) {
      self.deletes.push(key.into());
   }

   pub fn len_ops(&self) -> usize {
      self.puts.len() + self.deletes.len()
   }
}

/// Key-value database interface.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait Database: crate::MaybeSend {
   async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DatabaseError>;
   async fn set(&self, key: &[u8], value: &[u8]) -> Result<(), DatabaseError>;
   async fn delete(&self, key: &[u8]) -> Result<(), DatabaseError>;

   /// Apply many puts/deletes in one atomic unit (one txn on redb).
   async fn apply_batch(
      &self,
      batch: WriteBatch,
      durability: WriteDurability,
   ) -> Result<(), DatabaseError> {
      // Default: sequential fallback (memory / simple backends).
      let _ = durability;
      for (k, v) in batch.puts {
         self.set(&k, &v).await?;
      }
      for k in batch.deletes {
         self.delete(&k).await?;
      }
      Ok(())
   }

   async fn compact(&self) -> Result<bool, DatabaseError> {
      Ok(false)
   }

   /// Key used to AEAD-seal sensitive envelopes (accounts, POI).
   ///
   /// Required for account/POI read-write. Public tree/index watermarks do not
   /// need it.
   fn crypto_key(&self) -> Option<&crypto::RailgunDbKey> {
      None
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
   #[error("Encryption error: {0}")]
   EncryptionError(String),
   #[error("Decryption error: {0}")]
   DecryptionError(String),
   #[error("Missing Railgun DB crypto key")]
   MissingCryptoKey,
}
