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
///
/// Deliberately exhaustive: a new variant must break every `match` in the consuming app, so the
/// compiler — not a silent wildcard — forces the update. The crate owns that breakage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WriteDurability {
   #[default]
   Immediate,
   None,
}

/// Accumulated puts/deletes applied in a single transaction when the backend supports it.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
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

#[derive(Debug)]
#[non_exhaustive]
pub enum DatabaseError {
   SerializationError(serde_json::Error),
   UnsupportedVersion(u32),
   StorageError(String),
   EncryptionError(String),
   DecryptionError(String),
   MissingCryptoKey,
}

impl std::fmt::Display for DatabaseError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::SerializationError(err) => write!(f, "Serialization error: {err}"),
         Self::UnsupportedVersion(version) => {
            write!(f, "Unsupported version: {version}")
         }
         Self::StorageError(err) => write!(f, "Storage error: {err}"),
         Self::EncryptionError(err) => write!(f, "Encryption error: {err}"),
         Self::DecryptionError(err) => write!(f, "Decryption error: {err}"),
         Self::MissingCryptoKey => f.write_str("Missing Railgun DB crypto key"),
      }
   }
}

impl std::error::Error for DatabaseError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::SerializationError(err) => Some(err),
         Self::UnsupportedVersion(_)
         | Self::StorageError(_)
         | Self::EncryptionError(_)
         | Self::DecryptionError(_)
         | Self::MissingCryptoKey => None,
      }
   }
}

impl From<serde_json::Error> for DatabaseError {
   fn from(err: serde_json::Error) -> Self {
      Self::SerializationError(err)
   }
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

#[cfg(test)]
mod error_tests {
   use std::error::Error as _;

   use super::*;

   fn io_error() -> serde_json::Error {
      serde_json::Error::io(std::io::Error::other("boom"))
   }

   #[test]
   fn renders_the_pinned_messages() {
      let expected = format!("Serialization error: {}", io_error());
      assert_eq!(
         DatabaseError::SerializationError(io_error()).to_string(),
         expected
      );
      assert_eq!(
         DatabaseError::UnsupportedVersion(2).to_string(),
         "Unsupported version: 2"
      );
      assert_eq!(
         DatabaseError::StorageError("boom".to_owned()).to_string(),
         "Storage error: boom"
      );
      assert_eq!(
         DatabaseError::EncryptionError("boom".to_owned()).to_string(),
         "Encryption error: boom"
      );
      assert_eq!(
         DatabaseError::DecryptionError("boom".to_owned()).to_string(),
         "Decryption error: boom"
      );
      assert_eq!(
         DatabaseError::MissingCryptoKey.to_string(),
         "Missing Railgun DB crypto key"
      );
   }

   #[test]
   fn the_serialization_variant_delegates_its_source() {
      let inner = io_error();
      let rendered = inner.to_string();
      assert_eq!(
         DatabaseError::SerializationError(inner).source().map(ToString::to_string),
         Some(rendered)
      );
   }

   #[test]
   fn the_payload_variants_have_no_source() {
      assert!(DatabaseError::UnsupportedVersion(2).source().is_none());
      assert!(DatabaseError::StorageError("boom".to_owned()).source().is_none());
      assert!(DatabaseError::EncryptionError("boom".to_owned()).source().is_none());
      assert!(DatabaseError::DecryptionError("boom".to_owned()).source().is_none());
      assert!(DatabaseError::MissingCryptoKey.source().is_none());
   }
}
