use std::path::Path;
use std::sync::{Arc, RwLock};

use redb::backends::InMemoryBackend;
use redb::{Database as RedbInner, Durability, ReadableDatabase, TableDefinition};
use tokio::task;

use crate::database::{DatabaseError, RailgunDbKey, WriteBatch, WriteDurability};

const TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("railgun_kv");

/// redb-backed persistent KV store for Railgun state.
///
/// This is a good choice for desktop wallets because it is embedded,
/// fast, and has good durability guarantees.
#[derive(Clone)]
pub struct RedbDatabase {
   inner: Arc<RwLock<RedbInner>>,
   crypto_key: RailgunDbKey,
}

impl RedbDatabase {
   pub fn new(path: impl AsRef<Path>, crypto_key: RailgunDbKey) -> Result<Self, redb::Error> {
      let inner = if path.as_ref().exists() {
         RedbInner::open(path.as_ref())?
      } else {
         RedbInner::create(path.as_ref())?
      };

      Self::from_inner(inner, crypto_key)
   }

   /// In-memory redb (tests). Same API as a file-backed DB.
   pub fn in_memory(crypto_key: RailgunDbKey) -> Result<Self, redb::Error> {
      let inner = RedbInner::builder().create_with_backend(InMemoryBackend::new())?;
      Self::from_inner(inner, crypto_key)
   }

   fn from_inner(inner: RedbInner, crypto_key: RailgunDbKey) -> Result<Self, redb::Error> {
      let tx = inner.begin_write()?;
      {
         let _ = tx.open_table(TABLE);
      }
      tx.commit()?;

      Ok(Self {
         inner: Arc::new(RwLock::new(inner)),
         crypto_key,
      })
   }

   pub fn crypto_key(&self) -> &RailgunDbKey {
      &self.crypto_key
   }

   /// Compact the underlying redb file to reclaim unused space.
   pub async fn compact(&self) -> Result<bool, DatabaseError> {
      let inner = self.inner.clone();

      task::spawn_blocking(move || -> Result<bool, DatabaseError> {
         let mut guard = inner.write().map_err(|e| DatabaseError::StorageError(e.to_string()))?;

         let did_compact =
            guard.compact().map_err(|e| DatabaseError::StorageError(e.to_string()))?;

         Ok(did_compact)
      })
      .await
      .map_err(|e| DatabaseError::StorageError(e.to_string()))?
   }

   fn map_durability(d: WriteDurability) -> Durability {
      match d {
         WriteDurability::Immediate => Durability::Immediate,
         WriteDurability::None => Durability::None,
      }
   }

   pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DatabaseError> {
      let inner = self.inner.clone();
      let key = key.to_vec();

      task::spawn_blocking(
         move || -> Result<Option<Vec<u8>>, DatabaseError> {
            let guard = inner.read().map_err(|e| DatabaseError::StorageError(e.to_string()))?;

            let tx = guard.begin_read().map_err(|e| DatabaseError::StorageError(e.to_string()))?;
            let table: redb::ReadOnlyTable<&[u8], &[u8]> =
               tx.open_table(TABLE).map_err(|e| DatabaseError::StorageError(e.to_string()))?;

            match table.get(key.as_slice()) {
               Ok(Some(v)) => Ok(Some(v.value().to_vec())),
               Ok(None) => Ok(None),
               Err(e) => Err(DatabaseError::StorageError(e.to_string())),
            }
         },
      )
      .await
      .map_err(|e| DatabaseError::StorageError(e.to_string()))?
   }

   pub async fn set(&self, key: &[u8], value: &[u8]) -> Result<(), DatabaseError> {
      let mut batch = WriteBatch::new();
      batch.put(key.to_vec(), value.to_vec());
      self.apply_batch(batch, WriteDurability::Immediate).await
   }

   pub async fn delete(&self, key: &[u8]) -> Result<(), DatabaseError> {
      let mut batch = WriteBatch::new();
      batch.delete(key.to_vec());
      self.apply_batch(batch, WriteDurability::Immediate).await
   }

   pub async fn apply_batch(
      &self,
      batch: WriteBatch,
      durability: WriteDurability,
   ) -> Result<(), DatabaseError> {
      if batch.is_empty() {
         return Ok(());
      }

      let inner = self.inner.clone();
      let durability = Self::map_durability(durability);

      task::spawn_blocking(move || -> Result<(), DatabaseError> {
         let guard = inner.write().map_err(|e| DatabaseError::StorageError(e.to_string()))?;

         let mut tx =
            guard.begin_write().map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         tx.set_durability(durability)
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         {
            let mut table =
               tx.open_table(TABLE).map_err(|e| DatabaseError::StorageError(e.to_string()))?;
            for (key, value) in &batch.puts {
               table
                  .insert(key.as_slice(), value.as_slice())
                  .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
            }
            for key in &batch.deletes {
               table
                  .remove(key.as_slice())
                  .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
            }
         }
         tx.commit().map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         Ok(())
      })
      .await
      .map_err(|e| DatabaseError::StorageError(e.to_string()))?
   }

   pub async fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>, DatabaseError> {
      let inner = self.inner.clone();
      let prefix = prefix.to_vec();

      task::spawn_blocking(move || -> Result<Vec<Vec<u8>>, DatabaseError> {
         let guard = inner.read().map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         let tx = guard.begin_read().map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         let table: redb::ReadOnlyTable<&[u8], &[u8]> =
            tx.open_table(TABLE).map_err(|e| DatabaseError::StorageError(e.to_string()))?;

         let mut keys = Vec::new();
         let range = table
            .range(prefix.as_slice()..)
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         for item in range {
            let (k, _) = item.map_err(|e| DatabaseError::StorageError(e.to_string()))?;
            let key = k.value();
            if !key.starts_with(prefix.as_slice()) {
               break;
            }
            keys.push(key.to_vec());
         }
         Ok(keys)
      })
      .await
      .map_err(|e| DatabaseError::StorageError(e.to_string()))?
   }
}
