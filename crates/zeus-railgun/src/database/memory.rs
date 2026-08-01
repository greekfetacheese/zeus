use std::collections::HashMap;

use futures::lock::Mutex;

use crate::database::{Database, DatabaseError, RailgunDbKey, WriteBatch, WriteDurability};

/// Basic in-memory KV database implementation.
pub struct MemoryDatabase {
   store: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
   crypto_key: RailgunDbKey,
}

impl MemoryDatabase {
   pub fn new(crypto_key: RailgunDbKey) -> Self {
      Self {
         store: Mutex::new(HashMap::new()),
         crypto_key,
      }
   }
}

impl Default for MemoryDatabase {
   fn default() -> Self {
      Self::new(RailgunDbKey::generate().expect("generate test db key"))
   }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Database for MemoryDatabase {
   async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DatabaseError> {
      let store = self.store.lock().await;
      Ok(store.get(key).cloned())
   }

   async fn set(&self, key: &[u8], value: &[u8]) -> Result<(), DatabaseError> {
      let mut store = self.store.lock().await;
      store.insert(key.to_vec(), value.to_vec());
      Ok(())
   }

   async fn delete(&self, key: &[u8]) -> Result<(), DatabaseError> {
      let mut store = self.store.lock().await;
      store.remove(key);
      Ok(())
   }

   async fn apply_batch(
      &self,
      batch: WriteBatch,
      _durability: WriteDurability,
   ) -> Result<(), DatabaseError> {
      let mut store = self.store.lock().await;
      for (k, v) in batch.puts {
         store.insert(k, v);
      }
      for k in batch.deletes {
         store.remove(&k);
      }
      Ok(())
   }

   fn crypto_key(&self) -> Option<&RailgunDbKey> {
      Some(&self.crypto_key)
   }
}
