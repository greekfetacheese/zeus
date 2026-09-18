pub mod crypto;
pub mod railgun_db;
pub mod redb;

pub use crypto::{ENCRYPTED_ENVELOPE_VERSION, RailgunDbKey};
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

/// True when `err` (or any cause in its chain) is redb refusing to open a file
/// that already has a live `Database` handle in this process.
///
/// redb takes an exclusive per-file lock, so a second open — e.g. a stale
/// `RailgunProvider` still holding `railgun:{chain}.db` or
/// `events-snapshot:{chain}.db` — fails this way. Callers use this to retry
/// instead of matching on the error message text, which is not part of redb's
/// API contract.
///
/// Both redb error flavours are checked: `RedbDatabase::new` surfaces
/// [`redb::Error`], while the snapshot loader's `Database::create` surfaces
/// [`redb::DatabaseError`].
pub fn is_database_already_open(err: &anyhow::Error) -> bool {
   err.chain().any(|cause| {
      matches!(
         cause.downcast_ref::<::redb::DatabaseError>(),
         Some(::redb::DatabaseError::DatabaseAlreadyOpen)
      ) || matches!(
         cause.downcast_ref::<::redb::Error>(),
         Some(::redb::Error::DatabaseAlreadyOpen)
      )
   })
}
