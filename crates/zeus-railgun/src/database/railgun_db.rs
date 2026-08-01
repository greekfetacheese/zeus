use std::collections::BTreeSet;

use bincode_next::serde::{decode_from_slice, encode_to_vec};
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

use crate::{
   account::address::RailgunAddress,
   database::{
      Database, DatabaseError, RailgunDbKey, WriteBatch, WriteDurability,
      crypto::ENCRYPTED_ENVELOPE_VERSION,
   },
   indexer::{
      indexed_account::IndexedAccountState, txid_indexer::TxidIndexerState,
      utxo_indexer::UtxoIndexerState,
   },
   merkle_tree::RailgunMerkleTreeState,
   poi::provider::PoiProviderState,
};

/// Leaves per chunk for incremental merkle tree persistence.
///
/// Tip syncs that only touch the end of a tree rewrite O(chunk) bytes instead of
/// the full leaf vector / full level cache.
pub const TREE_LEAF_CHUNK: u32 = 1024;

/// Envelope version for chunked leaf tree storage (meta + chunks).
const TREE_CHUNK_FORMAT: u32 = 3;

fn require_crypto_key(db: &(impl Database + ?Sized)) -> Result<&RailgunDbKey, DatabaseError> {
   db.crypto_key().ok_or(DatabaseError::MissingCryptoKey)
}

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
      let mut batch = WriteBatch::new();
      put_envelope(&mut batch, &utxo_indexer_key(), 2, state)?;
      self.apply_batch(batch, WriteDurability::Immediate).await
   }

   async fn get_account(
      &self,
      addr: &RailgunAddress,
   ) -> Result<IndexedAccountState, DatabaseError> {
      let storage_key = account_key(addr);
      let Some(bytes) = self.get(&storage_key).await? else {
         return Ok(Default::default());
      };

      let crypto = require_crypto_key(self)?;
      deserialize_versioned_sensitive(&bytes, crypto, &storage_key)
   }

   async fn set_account(
      &self,
      addr: &RailgunAddress,
      state: &IndexedAccountState,
   ) -> Result<(), DatabaseError> {
      let mut batch = WriteBatch::new();
      let crypto = require_crypto_key(self)?;
      put_account(&mut batch, addr, state, crypto)?;
      self.apply_batch(batch, WriteDurability::Immediate).await
   }

   /// Load UTXO tree leaves. Supports legacy full-level blobs and chunked leaf format.
   ///
   /// Returns `(leaves, loaded_from_legacy_blob)`.
   async fn get_utxo_tree_leaves(
      &self,
      tree_number: u32,
   ) -> Result<Option<(Vec<U256>, bool)>, DatabaseError> {
      load_tree_leaves(self, Kind::Utxo, tree_number).await
   }

   /// Legacy helper: load full tree state (rebuilds levels from leaves).
   async fn get_utxo_tree(
      &self,
      tree_number: u32,
   ) -> Result<Option<RailgunMerkleTreeState>, DatabaseError> {
      let Some((leaves, _)) = self.get_utxo_tree_leaves(tree_number).await? else {
         return Ok(None);
      };
      let tree = crate::merkle_tree::RailgunMerkleTree::from_leaves(tree_number, leaves);
      Ok(Some(tree.state()))
   }

   async fn set_utxo_tree(
      &self,
      tree_number: u32,
      state: RailgunMerkleTreeState,
   ) -> Result<(), DatabaseError> {
      let leaves = state.tree.first().cloned().unwrap_or_default();
      let root = state.tree.last().and_then(|l| l.first().copied()).unwrap_or(U256::ZERO);
      let mut batch = WriteBatch::new();
      let chunks: BTreeSet<u32> = all_chunk_indices(leaves.len()).into_iter().collect();
      push_tree_save(
         &mut batch,
         Kind::Utxo,
         tree_number,
         &leaves,
         root,
         &chunks,
         true,
      )?;
      self.apply_batch(batch, WriteDurability::Immediate).await
   }

   async fn get_txid_indexer(&self) -> Result<TxidIndexerState, DatabaseError> {
      let key = txid_indexer_key();
      let Some(bytes) = self.get(&key).await? else {
         return Ok(Default::default());
      };

      deserialize_versioned(&bytes)
   }

   async fn set_txid_indexer(&self, state: &TxidIndexerState) -> Result<(), DatabaseError> {
      let mut batch = WriteBatch::new();
      put_envelope(&mut batch, &txid_indexer_key(), 2, state)?;
      self.apply_batch(batch, WriteDurability::Immediate).await
   }

   async fn get_txid_tree_leaves(
      &self,
      tree_number: u32,
   ) -> Result<Option<(Vec<U256>, bool)>, DatabaseError> {
      load_tree_leaves(self, Kind::Txid, tree_number).await
   }

   async fn get_txid_tree(
      &self,
      tree_number: u32,
   ) -> Result<Option<RailgunMerkleTreeState>, DatabaseError> {
      let Some((leaves, _)) = self.get_txid_tree_leaves(tree_number).await? else {
         return Ok(None);
      };
      let tree = crate::merkle_tree::RailgunMerkleTree::from_leaves(tree_number, leaves);
      Ok(Some(tree.state()))
   }

   async fn set_txid_tree(
      &self,
      tree_number: u32,
      state: RailgunMerkleTreeState,
   ) -> Result<(), DatabaseError> {
      let leaves = state.tree.first().cloned().unwrap_or_default();
      let root = state.tree.last().and_then(|l| l.first().copied()).unwrap_or(U256::ZERO);
      let mut batch = WriteBatch::new();
      let chunks: BTreeSet<u32> = all_chunk_indices(leaves.len()).into_iter().collect();
      push_tree_save(
         &mut batch,
         Kind::Txid,
         tree_number,
         &leaves,
         root,
         &chunks,
         true,
      )?;
      self.apply_batch(batch, WriteDurability::Immediate).await
   }

   async fn get_poi_provider(&self) -> Result<PoiProviderState, DatabaseError> {
      let storage_key = poi_provider_key();
      let Some(bytes) = self.get(&storage_key).await? else {
         return Ok(Default::default());
      };

      let crypto = require_crypto_key(self)?;
      deserialize_versioned_sensitive(&bytes, crypto, &storage_key)
   }

   async fn set_poi_provider(&self, state: &PoiProviderState) -> Result<(), DatabaseError> {
      let mut batch = WriteBatch::new();
      let crypto = require_crypto_key(self)?;
      put_poi_provider(&mut batch, state, crypto)?;
      self.apply_batch(batch, WriteDurability::Immediate).await
   }

   async fn write_envelope<S: Serialize + crate::MaybeSend>(
      &self,
      key: &[u8],
      version: u32,
      data: &S,
   ) -> Result<(), DatabaseError> {
      let mut batch = WriteBatch::new();
      put_envelope(&mut batch, key, version, data)?;
      self.apply_batch(batch, WriteDurability::Immediate).await
   }

   /// One-shot indexer save: watermark + dirty tree chunks + dirty accounts.
   async fn apply_utxo_save_batch(
      &self,
      batch: WriteBatch,
      durability: WriteDurability,
   ) -> Result<(), DatabaseError> {
      self.apply_batch(batch, durability).await
   }
}

impl<D: Database + ?Sized> RailgunDB for D {}

// ---------------------------------------------------------------------------
// Chunked tree persistence
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Kind {
   Utxo,
   Txid,
}

impl Kind {
   fn legacy_key(self, tree_number: u32) -> Vec<u8> {
      match self {
         Kind::Utxo => utxo_tree_key(tree_number),
         Kind::Txid => txid_tree_key(tree_number),
      }
   }

   fn meta_key(self, tree_number: u32) -> Vec<u8> {
      match self {
         Kind::Utxo => format!("utxo_tree:{}:meta", tree_number).into_bytes(),
         Kind::Txid => format!("txid_tree:{}:meta", tree_number).into_bytes(),
      }
   }

   fn chunk_key(self, tree_number: u32, chunk: u32) -> Vec<u8> {
      match self {
         Kind::Utxo => format!("utxo_tree:{}:c:{}", tree_number, chunk).into_bytes(),
         Kind::Txid => format!("txid_tree:{}:c:{}", tree_number, chunk).into_bytes(),
      }
   }
}

#[derive(Serialize, Deserialize)]
struct TreeMeta {
   pub number: u32,
   pub leaf_count: u32,
   pub root: U256,
   pub chunk_size: u32,
}

/// Encode helpers shared with indexers building a [`WriteBatch`].
pub fn put_envelope<S: Serialize>(
   batch: &mut WriteBatch,
   key: &[u8],
   version: u32,
   data: &S,
) -> Result<(), DatabaseError> {
   let bytes = serialize_envelope(version, data)?;
   batch.put(key.to_vec(), bytes);
   Ok(())
}

fn put_envelope_encrypted<S: Serialize>(
   batch: &mut WriteBatch,
   key: &[u8],
   data: &S,
   crypto: &RailgunDbKey,
) -> Result<(), DatabaseError> {
   let bytes = serialize_envelope_encrypted(data, crypto, key)?;
   batch.put(key.to_vec(), bytes);
   Ok(())
}

pub fn put_account(
   batch: &mut WriteBatch,
   addr: &RailgunAddress,
   state: &IndexedAccountState,
   crypto: &RailgunDbKey,
) -> Result<(), DatabaseError> {
   let key = account_key(addr);
   put_envelope_encrypted(batch, &key, state, crypto)
}

pub fn put_utxo_indexer(
   batch: &mut WriteBatch,
   state: &UtxoIndexerState,
) -> Result<(), DatabaseError> {
   put_envelope(batch, &utxo_indexer_key(), 2, state)
}

pub fn put_txid_indexer(
   batch: &mut WriteBatch,
   state: &TxidIndexerState,
) -> Result<(), DatabaseError> {
   put_envelope(batch, &txid_indexer_key(), 2, state)
}

pub fn put_poi_provider(
   batch: &mut WriteBatch,
   state: &PoiProviderState,
   crypto: &RailgunDbKey,
) -> Result<(), DatabaseError> {
   let key = poi_provider_key();
   put_envelope_encrypted(batch, &key, state, crypto)
}

/// Push meta + selected leaf chunks for a UTXO tree into `batch`.
///
/// When `migrate_from_legacy` is true, also deletes the legacy monolithic blob key
/// and expects `dirty_chunks` to cover every chunk that holds leaves.
pub fn push_utxo_tree_save(
   batch: &mut WriteBatch,
   tree_number: u32,
   leaves: &[U256],
   root: U256,
   dirty_chunks: &BTreeSet<u32>,
   migrate_from_legacy: bool,
) -> Result<(), DatabaseError> {
   push_tree_save(
      batch,
      Kind::Utxo,
      tree_number,
      leaves,
      root,
      dirty_chunks,
      migrate_from_legacy,
   )
}

pub fn push_txid_tree_save(
   batch: &mut WriteBatch,
   tree_number: u32,
   leaves: &[U256],
   root: U256,
   dirty_chunks: &BTreeSet<u32>,
   migrate_from_legacy: bool,
) -> Result<(), DatabaseError> {
   push_tree_save(
      batch,
      Kind::Txid,
      tree_number,
      leaves,
      root,
      dirty_chunks,
      migrate_from_legacy,
   )
}

fn push_tree_save(
   batch: &mut WriteBatch,
   kind: Kind,
   tree_number: u32,
   leaves: &[U256],
   root: U256,
   dirty_chunks: &BTreeSet<u32>,
   migrate_from_legacy: bool,
) -> Result<(), DatabaseError> {
   let meta = TreeMeta {
      number: tree_number,
      leaf_count: leaves.len() as u32,
      root,
      chunk_size: TREE_LEAF_CHUNK,
   };
   put_envelope(
      batch,
      &kind.meta_key(tree_number),
      TREE_CHUNK_FORMAT,
      &meta,
   )?;

   let chunk_size = TREE_LEAF_CHUNK as usize;
   for &chunk_idx in dirty_chunks {
      let start = chunk_idx as usize * chunk_size;
      if start >= leaves.len() {
         // Tree shrank (should not happen for Railgun UTXO trees), delete stale chunk.
         batch.delete(kind.chunk_key(tree_number, chunk_idx));
         continue;
      }
      let end = (start + chunk_size).min(leaves.len());
      let slice = &leaves[start..end];
      put_envelope(
         batch,
         &kind.chunk_key(tree_number, chunk_idx),
         TREE_CHUNK_FORMAT,
         &slice.to_vec(),
      )?;
   }

   if migrate_from_legacy {
      batch.delete(kind.legacy_key(tree_number));
   }

   Ok(())
}

async fn load_tree_leaves<D: Database + ?Sized>(
   db: &D,
   kind: Kind,
   tree_number: u32,
) -> Result<Option<(Vec<U256>, bool)>, DatabaseError> {
   // Prefer chunked format.
   if let Some(meta_bytes) = db.get(&kind.meta_key(tree_number)).await? {
      let meta: TreeMeta = deserialize_versioned(&meta_bytes)?;
      if meta.leaf_count == 0 {
         return Ok(Some((Vec::new(), false)));
      }
      let chunk_size = if meta.chunk_size == 0 {
         TREE_LEAF_CHUNK
      } else {
         meta.chunk_size
      } as usize;
      let n_chunks = (meta.leaf_count as usize).div_ceil(chunk_size);
      let mut leaves = Vec::with_capacity(meta.leaf_count as usize);
      for chunk_idx in 0..n_chunks {
         let Some(bytes) = db.get(&kind.chunk_key(tree_number, chunk_idx as u32)).await? else {
            return Err(DatabaseError::StorageError(format!(
               "missing tree chunk {} for tree {}",
               chunk_idx, tree_number
            )));
         };
         let chunk: Vec<U256> = deserialize_versioned(&bytes)?;
         leaves.extend(chunk);
      }
      if leaves.len() != meta.leaf_count as usize {
         // Truncate or pad is wrong, trust leaf vector length after load.
         leaves.truncate(meta.leaf_count as usize);
      }
      return Ok(Some((leaves, false)));
   }

   // Legacy monolithic full-level blob.
   if let Some(bytes) = db.get(&kind.legacy_key(tree_number)).await? {
      let state = deserialize_versioned_tree(&bytes)?
         .ok_or_else(|| DatabaseError::StorageError("empty legacy tree blob".into()))?;
      let leaves = state.tree.first().cloned().unwrap_or_default();
      return Ok(Some((leaves, true)));
   }

   Ok(None)
}

/// Chunk indices covering leaf range `[start, end)` (end exclusive).
pub fn dirty_chunks_for_range(start: usize, end: usize) -> BTreeSet<u32> {
   let mut set = BTreeSet::new();
   if end <= start {
      return set;
   }
   let chunk_size = TREE_LEAF_CHUNK as usize;
   let first = start / chunk_size;
   let last = (end - 1) / chunk_size;
   for c in first..=last {
      set.insert(c as u32);
   }
   set
}

pub fn all_chunk_indices(leaf_count: usize) -> BTreeSet<u32> {
   dirty_chunks_for_range(0, leaf_count)
}

// v1: legacy JSON (for reading old DBs)
// v2: bincode (compact binary, used for public / non-sensitive writes)
// v3: chunked tree meta/chunks (same bincode envelope)
// v4: bincode payload sealed with RailgunDbKey (accounts, POI)

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
      // v2 and above (below encrypted) use bincode for the payload (plaintext)
      v if v >= 2 && v < ENCRYPTED_ENVELOPE_VERSION => {
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

fn serialize_envelope_encrypted<T: Serialize>(
   data: &T,
   crypto: &RailgunDbKey,
   aad: &[u8],
) -> Result<Vec<u8>, DatabaseError> {
   let payload = encode_to_vec(data, bincode_next::config::standard())
      .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
   let sealed = crypto.seal(&payload, aad)?;
   let env = BincodeEnvelope {
      v: ENCRYPTED_ENVELOPE_VERSION,
      data: sealed,
   };
   encode_to_vec(&env, bincode_next::config::standard())
      .map_err(|e| DatabaseError::StorageError(e.to_string()))
}

/// Deserialize public / non-sensitive states (indexers, tree meta/chunks)
fn deserialize_versioned<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, DatabaseError> {
   // Try bincode v2+ first
   if let Ok((env, _)) =
      decode_from_slice::<BincodeEnvelope, _>(bytes, bincode_next::config::standard())
   {
      if env.v >= ENCRYPTED_ENVELOPE_VERSION {
         return Err(DatabaseError::StorageError(
            "encrypted envelope requires crypto key path".into(),
         ));
      }
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

/// Deserialize sensitive states (accounts, POI): supports plaintext v1/v2 migration + v4 sealed.
fn deserialize_versioned_sensitive<T: for<'de> Deserialize<'de>>(
   bytes: &[u8],
   crypto: &RailgunDbKey,
   aad: &[u8],
) -> Result<T, DatabaseError> {
   if let Ok((env, _)) =
      decode_from_slice::<BincodeEnvelope, _>(bytes, bincode_next::config::standard())
   {
      if env.v >= ENCRYPTED_ENVELOPE_VERSION {
         let plain = crypto.open(&env.data, aad)?;
         let (val, _) = decode_from_slice::<_, _>(&plain, bincode_next::config::standard())
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         return Ok(val);
      }
      if env.v >= 2 {
         // Legacy plaintext bincode — accepted for one-shot migration on next write.
         let (val, _) = decode_from_slice::<_, _>(&env.data, bincode_next::config::standard())
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         return Ok(val);
      }
   }

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

#[cfg(test)]
mod tests {
   use super::*;
   use crate::database::memory::MemoryDatabase;
   use crate::merkle_tree::RailgunMerkleTree;

   fn test_db() -> MemoryDatabase {
      MemoryDatabase::new(RailgunDbKey::generate().unwrap())
   }

   #[tokio::test]
   async fn chunked_roundtrip_and_incremental() {
      let db = test_db();
      let mut tree = RailgunMerkleTree::new(0);
      let leaves: Vec<U256> = (0..1500u64).map(U256::from).collect();
      tree.insert_leaves(&leaves, 0);

      let mut batch = WriteBatch::new();
      let chunks = all_chunk_indices(leaves.len());
      push_utxo_tree_save(
         &mut batch,
         0,
         tree.leaves(),
         tree.root().into(),
         &chunks,
         false,
      )
      .unwrap();
      db.apply_batch(batch, WriteDurability::Immediate).await.unwrap();

      let (loaded, legacy) = db.get_utxo_tree_leaves(0).await.unwrap().unwrap();
      assert!(!legacy);
      assert_eq!(loaded, leaves);

      // Append a few leaves — only last chunk should be rewritten.
      let extra: Vec<U256> = (1500..1510u64).map(U256::from).collect();
      tree.insert_leaves(&extra, 1500);
      let dirty = dirty_chunks_for_range(1500, 1510);
      assert_eq!(dirty.iter().copied().collect::<Vec<_>>(), vec![1]);

      let mut batch = WriteBatch::new();
      push_utxo_tree_save(
         &mut batch,
         0,
         tree.leaves(),
         tree.root().into(),
         &dirty,
         false,
      )
      .unwrap();
      // meta + 1 chunk
      assert_eq!(batch.puts.len(), 2);
      db.apply_batch(batch, WriteDurability::Immediate).await.unwrap();

      let (loaded, _) = db.get_utxo_tree_leaves(0).await.unwrap().unwrap();
      assert_eq!(loaded.len(), 1510);
      assert_eq!(loaded, tree.leaves());

      let rebuilt = RailgunMerkleTree::from_leaves(0, loaded);
      assert_eq!(rebuilt.root(), tree.root());
   }

   #[tokio::test]
   async fn legacy_blob_load_and_migrate() {
      let db = test_db();
      let mut tree = RailgunMerkleTree::new(2);
      let leaves: Vec<U256> = (0..50u64).map(U256::from).collect();
      tree.insert_leaves(&leaves, 0);

      // Write legacy monolithic blob via old path (v2 envelope of full state).
      let mut batch = WriteBatch::new();
      put_envelope(&mut batch, &utxo_tree_key(2), 2, &tree.state()).unwrap();
      db.apply_batch(batch, WriteDurability::Immediate).await.unwrap();

      let (loaded, legacy) = db.get_utxo_tree_leaves(2).await.unwrap().unwrap();
      assert!(legacy);
      assert_eq!(loaded, leaves);

      // Migrate
      let mut batch = WriteBatch::new();
      let chunks = all_chunk_indices(loaded.len());
      push_utxo_tree_save(
         &mut batch,
         2,
         &loaded,
         tree.root().into(),
         &chunks,
         true,
      )
      .unwrap();
      db.apply_batch(batch, WriteDurability::Immediate).await.unwrap();

      assert!(db.get(&utxo_tree_key(2)).await.unwrap().is_none());
      let (loaded2, legacy2) = db.get_utxo_tree_leaves(2).await.unwrap().unwrap();
      assert!(!legacy2);
      assert_eq!(loaded2, leaves);
   }

   #[tokio::test]
   async fn account_encrypted_roundtrip_and_plaintext_migrate() {
      use crate::account::address::RailgunAddress;
      use crate::indexer::indexed_account::IndexedAccountState;

      let crypto = RailgunDbKey::generate().unwrap();
      let db = MemoryDatabase::new(crypto.clone());

      let legacy_state = IndexedAccountState {
         notes: vec![],
         synced_block: 42,
      };
      let seed: [u8; 64] = rand::random();
      let sec = secure_types::SecureArray::from_slice(&seed).unwrap();
      let addr = RailgunAddress::new(&sec, 0, None).unwrap();

      let mut batch = WriteBatch::new();
      put_envelope(&mut batch, &account_key(&addr), 2, &legacy_state).unwrap();
      db.apply_batch(batch, WriteDurability::Immediate).await.unwrap();

      let loaded = db.get_account(&addr).await.unwrap();
      assert_eq!(loaded.synced_block, 42);

      // Rewrite encrypted
      db.set_account(&addr, &loaded).await.unwrap();
      let raw = db.get(&account_key(&addr)).await.unwrap().unwrap();
      let (env, _) =
         decode_from_slice::<BincodeEnvelope, _>(&raw, bincode_next::config::standard()).unwrap();
      assert_eq!(env.v, ENCRYPTED_ENVELOPE_VERSION);

      let loaded2 = db.get_account(&addr).await.unwrap();
      assert_eq!(loaded2.synced_block, 42);

      // Wrong key cannot open
      let db_bad = MemoryDatabase::new(RailgunDbKey::generate().unwrap());
      db_bad.set(&account_key(&addr), &raw).await.unwrap();
      assert!(db_bad.get_account(&addr).await.is_err());
   }

   #[tokio::test]
   async fn large_tree_save_load_and_continue() {
      let db = test_db();
      let n1 = 65123usize;
      let leaves1: Vec<U256> = (0..n1 as u64).map(|i| U256::from(i * 17 + 3)).collect();
      let mut tree = RailgunMerkleTree::new(2);
      tree.insert_leaves(&leaves1, 0);
      let root1 = tree.root();

      let mut batch = WriteBatch::new();
      let chunks = all_chunk_indices(n1);
      push_utxo_tree_save(
         &mut batch,
         2,
         tree.leaves(),
         root1.into(),
         &chunks,
         false,
      )
      .unwrap();
      db.apply_batch(batch, WriteDurability::Immediate).await.unwrap();

      let (loaded, legacy) = db.get_utxo_tree_leaves(2).await.unwrap().unwrap();
      assert!(!legacy);
      assert_eq!(loaded.len(), n1);
      assert_eq!(loaded, leaves1);
      let mut tree2 = RailgunMerkleTree::from_leaves(2, loaded);
      assert_eq!(tree2.root(), root1, "reload root mismatch");

      // Continue like tip sync
      let extra: Vec<U256> =
         (n1 as u64..(n1 as u64 + 500)).map(|i| U256::from(i * 17 + 3)).collect();
      tree.insert_leaves(&extra, n1);
      tree2.insert_leaves(&extra, n1);
      assert_eq!(
         tree.root(),
         tree2.root(),
         "incremental after load mismatch"
      );

      // Continuous build
      let mut cont = RailgunMerkleTree::new(2);
      let all: Vec<U256> = (0..(n1 as u64 + 500)).map(|i| U256::from(i * 17 + 3)).collect();
      cont.insert_leaves(&all, 0);
      assert_eq!(cont.root(), tree2.root());
   }
}
