use std::path::Path;
use std::sync::{Arc, RwLock};

use redb::{Database as RedbInner, ReadableDatabase, TableDefinition};
use tokio::task;

use crate::database::{Database, DatabaseError};

const TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("railgun_kv");

/// redb-backed persistent KV store for Railgun state.
///
/// This is a good choice for desktop wallets because it is embedded,
/// fast, and has good durability guarantees.
pub struct RedbDatabase {
   inner: Arc<RwLock<RedbInner>>,
}

impl RedbDatabase {
   pub fn new(path: impl AsRef<Path>) -> Result<Self, redb::Error> {
      let inner = if path.as_ref().exists() {
         RedbInner::open(path.as_ref())?
      } else {
         RedbInner::create(path.as_ref())?
      };

      // Ensure the table exists (cheap/no-op on subsequent opens)
      let tx = inner.begin_write()?;
      {
         let _ = tx.open_table(TABLE);
      }
      tx.commit()?;

      Ok(Self {
         inner: Arc::new(RwLock::new(inner)),
      })
   }

   /// Compact the underlying redb file to reclaim unused space.
   pub async fn compact(&self) -> Result<bool, DatabaseError> {
      let inner = self.inner.clone();

      task::spawn_blocking(move || -> Result<bool, DatabaseError> {
         let mut guard = inner
            .write()
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;

         let did_compact = guard
            .compact()
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;

         Ok(did_compact)
      })
      .await
      .map_err(|e| DatabaseError::StorageError(e.to_string()))?
   }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Database for RedbDatabase {
   async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DatabaseError> {
      let inner = self.inner.clone();
      let key = key.to_vec();

      task::spawn_blocking(
         move || -> Result<Option<Vec<u8>>, DatabaseError> {
            let guard = inner
               .read()
               .map_err(|e| DatabaseError::StorageError(e.to_string()))?;

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

   async fn set(&self, key: &[u8], value: &[u8]) -> Result<(), DatabaseError> {
      let inner = self.inner.clone();
      let key = key.to_vec();
      let value = value.to_vec();

      task::spawn_blocking(move || -> Result<(), DatabaseError> {
         let guard = inner
            .write()
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;

         let tx = guard.begin_write().map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         {
            let mut table =
               tx.open_table(TABLE).map_err(|e| DatabaseError::StorageError(e.to_string()))?;
            table
               .insert(key.as_slice(), value.as_slice())
               .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         }
         tx.commit().map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         Ok(())
      })
      .await
      .map_err(|e| DatabaseError::StorageError(e.to_string()))?
   }

   async fn delete(&self, key: &[u8]) -> Result<(), DatabaseError> {
      let inner = self.inner.clone();
      let key = key.to_vec();

      task::spawn_blocking(move || -> Result<(), DatabaseError> {
         let guard = inner
            .write()
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;

         let tx = guard.begin_write().map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         {
            let mut table =
               tx.open_table(TABLE).map_err(|e| DatabaseError::StorageError(e.to_string()))?;
            table
               .remove(key.as_slice())
               .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         }
         tx.commit().map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         Ok(())
      })
      .await
      .map_err(|e| DatabaseError::StorageError(e.to_string()))?
   }

   async fn compact(&self) -> Result<bool, DatabaseError> {
      RedbDatabase::compact(self).await
   }
}
