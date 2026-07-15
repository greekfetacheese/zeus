use bincode_next::serde::{decode_from_slice, encode_to_vec};
use serde::{Deserialize, Serialize};

use crate::{
   account::address::RailgunAddress,
   database::{Database, DatabaseError},
   indexer::{
      indexed_account::IndexedAccountState, txid_indexer::TxidIndexerState,
      utxo_indexer::UtxoIndexerState,
   },
   merkle_tree::RailgunMerkleTreeState,
   poi::provider::PoiProviderState,
};

/// Database trait extension with Railgun-specific methods for storing and retrieving typed state.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait RailgunDB: Database + crate::MaybeSend {
   async fn get_utxo_indexer(&self) -> Result<UtxoIndexerState, DatabaseError> {
      let key = utxo_indexer_key();
      let Some(bytes) = self.get(&key).await? else {
         return Ok(Default::default());
      };

      deserialize_versioned(&bytes)
   }

   async fn set_utxo_indexer(&self, state: &UtxoIndexerState) -> Result<(), DatabaseError> {
      self.write_envelope(&utxo_indexer_key(), 2, state).await
   }

   async fn get_account(
      &self,
      addr: &RailgunAddress,
   ) -> Result<IndexedAccountState, DatabaseError> {
      let key = account_key(addr);
      let Some(bytes) = self.get(&key).await? else {
         return Ok(Default::default());
      };

      deserialize_versioned(&bytes)
   }

   async fn set_account(
      &self,
      addr: &RailgunAddress,
      state: &IndexedAccountState,
   ) -> Result<(), DatabaseError> {
      self.write_envelope(&account_key(addr), 2, state).await
   }

   async fn get_utxo_tree(
      &self,
      tree_number: u32,
   ) -> Result<Option<RailgunMerkleTreeState>, DatabaseError> {
      let key = utxo_tree_key(tree_number);
      let Some(bytes) = self.get(&key).await? else {
         return Ok(None);
      };

      deserialize_versioned_tree(&bytes)
   }

   async fn set_utxo_tree(
      &self,
      tree_number: u32,
      state: RailgunMerkleTreeState,
   ) -> Result<(), DatabaseError> {
      self.write_envelope(&utxo_tree_key(tree_number), 2, &state).await
   }

   async fn get_txid_indexer(&self) -> Result<TxidIndexerState, DatabaseError> {
      let key = txid_indexer_key();
      let Some(bytes) = self.get(&key).await? else {
         return Ok(Default::default());
      };

      deserialize_versioned(&bytes)
   }

   async fn set_txid_indexer(&self, state: &TxidIndexerState) -> Result<(), DatabaseError> {
      self.write_envelope(&txid_indexer_key(), 2, state).await
   }

   async fn get_txid_tree(
      &self,
      tree_number: u32,
   ) -> Result<Option<RailgunMerkleTreeState>, DatabaseError> {
      let key = txid_tree_key(tree_number);
      let Some(bytes) = self.get(&key).await? else {
         return Ok(None);
      };

      deserialize_versioned_tree(&bytes)
   }

   async fn set_txid_tree(
      &self,
      tree_number: u32,
      state: RailgunMerkleTreeState,
   ) -> Result<(), DatabaseError> {
      self.write_envelope(&txid_tree_key(tree_number), 2, &state).await
   }

   async fn get_poi_provider(&self) -> Result<PoiProviderState, DatabaseError> {
      let key = poi_provider_key();
      let Some(bytes) = self.get(&key).await? else {
         return Ok(Default::default());
      };

      deserialize_versioned(&bytes)
   }

   async fn set_poi_provider(&self, state: &PoiProviderState) -> Result<(), DatabaseError> {
      self.write_envelope(&poi_provider_key(), 2, state).await
   }

   async fn write_envelope<S: Serialize + crate::MaybeSend>(
      &self,
      key: &[u8],
      version: u32,
      data: &S,
   ) -> Result<(), DatabaseError> {
      let bytes = serialize_envelope(version, data)?;
      self.set(key, &bytes).await?;
      Ok(())
   }
}

impl<D: Database + ?Sized> RailgunDB for D {}

// v1: legacy JSON (for reading old DBs)
// v2: bincode (compact binary, used for all new writes)

#[derive(Serialize, Deserialize)]
struct JsonEnvelope {
   pub v: u32,
   pub data: serde_json::Value,
}

#[derive(Serialize, Deserialize)]
struct BincodeEnvelope {
   pub v: u32,
   pub data: Vec<u8>,
}

fn serialize_envelope<T: Serialize>(version: u32, data: &T) -> Result<Vec<u8>, DatabaseError> {
   match version {
      // v2 and above use bincode for the payload
      v if v >= 2 => {
         let payload = encode_to_vec(data, bincode_next::config::standard())
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         let env = BincodeEnvelope { v, data: payload };
         encode_to_vec(&env, bincode_next::config::standard())
            .map_err(|e| DatabaseError::StorageError(e.to_string()))
      }
      // v1 uses JSON (legacy)
      1 => {
         let env = JsonEnvelope {
            v: 1,
            data: serde_json::to_value(data)?,
         };
         Ok(serde_json::to_vec(&env)?)
      }
      _ => Err(DatabaseError::UnsupportedVersion(version)),
   }
}

/// Deserialize small states (indexers, accounts, poi)
fn deserialize_versioned<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, DatabaseError> {
   // Try bincode v2+ first
   if let Ok((env, _)) =
      decode_from_slice::<BincodeEnvelope, _>(bytes, bincode_next::config::standard())
   {
      if env.v >= 2 {
         let (val, _) = decode_from_slice::<_, _>(&env.data, bincode_next::config::standard())
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         return Ok(val);
      }
   }

   // Fallback to legacy JSON v1
   let env: JsonEnvelope = serde_json::from_slice(bytes)?;
   match env.v {
      1 => serde_json::from_value(env.data).map_err(Into::into),
      v => Err(DatabaseError::UnsupportedVersion(v)),
   }
}

/// Deserialize tree states (big Vec<Vec<U256>> benefit a lot from bincode)
fn deserialize_versioned_tree(
   bytes: &[u8],
) -> Result<Option<RailgunMerkleTreeState>, DatabaseError> {
   // Try bincode v2+
   if let Ok((env, _)) =
      decode_from_slice::<BincodeEnvelope, _>(bytes, bincode_next::config::standard())
   {
      if env.v >= 2 {
         let (state, _) = decode_from_slice::<_, _>(&env.data, bincode_next::config::standard())
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         return Ok(Some(state));
      }
   }

   // Legacy JSON
   let env: JsonEnvelope = serde_json::from_slice(bytes)?;
   match env.v {
      1 => Ok(Some(serde_json::from_value(env.data)?)),
      v => Err(DatabaseError::UnsupportedVersion(v)),
   }
}

fn utxo_indexer_key() -> Vec<u8> {
   b"utxo_indexer".to_vec()
}

fn account_key(addr: &RailgunAddress) -> Vec<u8> {
   format!("account:{:?}", addr).into_bytes()
}

fn utxo_tree_key(tree_number: u32) -> Vec<u8> {
   format!("utxo_tree:{}", tree_number).into_bytes()
}

fn txid_indexer_key() -> Vec<u8> {
   b"txid_indexer".to_vec()
}

fn txid_tree_key(tree_number: u32) -> Vec<u8> {
   format!("txid_tree:{}", tree_number).into_bytes()
}

fn poi_provider_key() -> Vec<u8> {
   b"poi_provider".to_vec()
}
