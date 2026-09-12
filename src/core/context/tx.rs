use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use alloy_consensus::TxType;
use anyhow::anyhow;
use redb::{Database as RedbInner, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeus_eth::{
   alloy_primitives::{Address, TxHash},
   utils::NumericValue,
};

use crate::core::clear_signing::ClearDisplay;
use crate::core::persisted::{PersistedFile, file_path};
use crate::core::tx::{DecodedEvent, TransactionAnalysis, TransactionRich};
use crate::core::wallet_state_key::WalletStateKey;
use crate::utils::{TimeStamp, restrict_file_to_owner};

/// Transactions by chain and wallet address
pub type Transactions = HashMap<(u64, Address), Vec<TransactionRich>>;

const TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("tx_history");

/// redb's default page cache is 1 GiB. Tx history is read in small owner lists.
const PAGE_CACHE_BYTES: usize = 1024 * 1024;

const TX_DB_AAD: &[u8] = b"zeus-tx-history-v1";

struct TxRedbStore {
   inner: Arc<RwLock<RedbInner>>,
   key: WalletStateKey,
}

struct TxDBInner {
   db: TransactionsDB,
   store: Option<TxRedbStore>,
}

#[derive(Clone)]
pub struct TxDBHandle(Arc<RwLock<TxDBInner>>);

impl Default for TxDBHandle {
   fn default() -> Self {
      Self::new()
   }
}

impl Serialize for TxDBHandle {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: Serializer,
   {
      self.read(|db| db.serialize(serializer))
   }
}

impl<'de> Deserialize<'de> for TxDBHandle {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: Deserializer<'de>,
   {
      let db = TransactionsDB::deserialize(deserializer)?;
      Ok(Self(Arc::new(RwLock::new(TxDBInner {
         db,
         store: None,
      }))))
   }
}

impl TxDBHandle {
   pub fn new() -> Self {
      Self(Arc::new(RwLock::new(TxDBInner {
         db: TransactionsDB::new(),
         store: None,
      })))
   }

   pub fn dir() -> Result<PathBuf, anyhow::Error> {
      file_path(PersistedFile::TxHistory)
   }

   pub fn read<R>(&self, reader: impl FnOnce(&TransactionsDB) -> R) -> R {
      reader(&self.0.read().unwrap().db)
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut TransactionsDB) -> R) -> R {
      writer(&mut self.0.write().unwrap().db)
   }

   /// Open `tx_history.db`, creating it if needed, and load every owner list.
   pub fn open(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      Self::open_at(&Self::dir()?, key)
   }

   pub fn open_at(path: &Path, key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      if let Some(parent) = path.parent() {
         std::fs::create_dir_all(parent)?;
      }

      let inner = RedbInner::builder()
         .set_cache_size(PAGE_CACHE_BYTES)
         .create(path)
         .map_err(|e| anyhow!("open tx history db: {e}"))?;

      if let Err(e) = restrict_file_to_owner(path) {
         tracing::warn!("Failed to restrict {}: {e}", path.display());
      }

      let tx = inner.begin_write().map_err(|e| anyhow!("tx history db write txn: {e}"))?;
      {
         let _ = tx.open_table(TABLE).map_err(|e| anyhow!("tx history db open table: {e}"))?;
      }
      tx.commit().map_err(|e| anyhow!("tx history db commit: {e}"))?;

      let store = TxRedbStore {
         inner: Arc::new(RwLock::new(inner)),
         key: key.clone(),
      };
      let db = load_all(&store)?;

      Ok(Self(Arc::new(RwLock::new(TxDBInner {
         db,
         store: Some(store),
      }))))
   }

   /// Merge txs still embedded in WalletState. Returns true if anything was imported.
   pub fn import_legacy(&self, legacy: &TxDBHandle) -> Result<bool, anyhow::Error> {
      let legacy_map = legacy.read(|db| db.txs.clone());
      if legacy_map.is_empty() {
         return Ok(false);
      }

      let mut guard = self.0.write().unwrap();
      for ((chain, owner), txs) in legacy_map {
         let entry = guard.db.txs.entry((chain, owner)).or_default();
         merge_by_hash(entry, txs);
         persist_owner(&guard, chain, owner)?;
      }
      Ok(true)
   }

   pub fn add_tx(
      &self,
      chain: u64,
      owner: Address,
      tx: TransactionRich,
   ) -> Result<(), anyhow::Error> {
      let mut guard = self.0.write().unwrap();
      guard.db.add_tx(chain, owner, tx)?;
      persist_owner(&guard, chain, owner)
   }

   pub fn get_txs(&self, chain: u64, owner: Address) -> Option<Vec<TransactionRich>> {
      self.read(|db| db.get_txs(chain, owner).cloned())
   }

   pub fn get_tx_count(&self, chain: u64, owner: Address) -> usize {
      self.read(|db| db.get_tx_count(chain, owner))
   }

   pub fn txs_count(&self) -> usize {
      self.read(|db| db.all().count())
   }

   pub fn get_txs_paged(
      &self,
      chain: u64,
      owner: Address,
      page: usize,
      per_page: usize,
   ) -> Option<Vec<TransactionRich>> {
      self.read(|db| db.get_txs_paged(chain, owner, page, per_page))
   }

   /// Drop tx histories whose owner is not in `wallets`. Returns how many entries were removed.
   pub fn retain_wallets(&self, wallets: &HashSet<Address>) -> usize {
      let mut guard = self.0.write().unwrap();
      let before_keys: Vec<(u64, Address)> = guard.db.txs.keys().copied().collect();
      let removed = guard.db.retain_wallets(wallets);
      if removed == 0 {
         return 0;
      }
      for (chain, owner) in before_keys {
         if wallets.contains(&owner) {
            continue;
         }
         if let Err(e) = delete_owner(&guard, chain, owner) {
            tracing::error!("Failed to delete tx history for {owner} on chain {chain}: {e}");
         }
      }
      removed
   }
}

fn owner_key(chain: u64, owner: Address) -> [u8; 28] {
   let mut key = [0u8; 28];
   key[..8].copy_from_slice(&chain.to_be_bytes());
   key[8..].copy_from_slice(owner.as_slice());
   key
}

fn parse_owner_key(key: &[u8]) -> Result<(u64, Address), anyhow::Error> {
   if key.len() != 28 {
      return Err(anyhow!(
         "invalid tx history key length {}",
         key.len()
      ));
   }
   let chain = u64::from_be_bytes(key[..8].try_into().expect("8 bytes"));
   let owner = Address::from_slice(&key[8..]);
   Ok((chain, owner))
}

fn persist_owner(inner: &TxDBInner, chain: u64, owner: Address) -> Result<(), anyhow::Error> {
   let Some(store) = inner.store.as_ref() else {
      return Ok(());
   };

   let Some(txs) = inner.db.get_txs(chain, owner) else {
      return delete_owner(inner, chain, owner);
   };

   if txs.is_empty() {
      return delete_owner(inner, chain, owner);
   }

   let stored: Vec<StoredTx> = txs.iter().map(StoredTx::from).collect();
   let sealed = store.key.seal_json(&stored, TX_DB_AAD)?;
   let key = owner_key(chain, owner);

   let guard = store.inner.write().map_err(|e| anyhow!("tx history db lock: {e}"))?;
   let mut tx = guard.begin_write().map_err(|e| anyhow!("tx history db write txn: {e}"))?;
   tx.set_durability(Durability::Immediate)
      .map_err(|e| anyhow!("tx history db durability: {e}"))?;
   {
      let mut table = tx.open_table(TABLE).map_err(|e| anyhow!("tx history db open table: {e}"))?;
      table
         .insert(key.as_slice(), sealed.as_slice())
         .map_err(|e| anyhow!("tx history db insert: {e}"))?;
   }
   tx.commit().map_err(|e| anyhow!("tx history db commit: {e}"))?;
   Ok(())
}

fn delete_owner(inner: &TxDBInner, chain: u64, owner: Address) -> Result<(), anyhow::Error> {
   let Some(store) = inner.store.as_ref() else {
      return Ok(());
   };

   let key = owner_key(chain, owner);
   let guard = store.inner.write().map_err(|e| anyhow!("tx history db lock: {e}"))?;
   let mut tx = guard.begin_write().map_err(|e| anyhow!("tx history db write txn: {e}"))?;

   tx.set_durability(Durability::Immediate)
      .map_err(|e| anyhow!("tx history db durability: {e}"))?;
   {
      let mut table = tx.open_table(TABLE).map_err(|e| anyhow!("tx history db open table: {e}"))?;
      table.remove(key.as_slice()).map_err(|e| anyhow!("tx history db remove: {e}"))?;
   }
   tx.commit().map_err(|e| anyhow!("tx history db commit: {e}"))?;
   Ok(())
}

fn load_all(store: &TxRedbStore) -> Result<TransactionsDB, anyhow::Error> {
   let guard = store.inner.read().map_err(|e| anyhow!("tx history db lock: {e}"))?;
   let tx = guard.begin_read().map_err(|e| anyhow!("tx history db read txn: {e}"))?;
   let table = tx.open_table(TABLE).map_err(|e| anyhow!("tx history db open table: {e}"))?;

   let mut db = TransactionsDB::new();
   let iter = table.iter().map_err(|e| anyhow!("tx history db iter: {e}"))?;

   for item in iter {
      let (k, v) = item.map_err(|e| anyhow!("tx history db row: {e}"))?;
      let (chain, owner) = parse_owner_key(k.value())?;
      let stored: Vec<StoredTx> = store.key.open_json(v.value(), TX_DB_AAD)?;
      let mut out = Vec::with_capacity(stored.len());
      for s in stored {
         out.push(TransactionRich::try_from(s)?);
      }
      out.sort_by(|a, b| b.block.cmp(&a.block).then_with(|| b.timestamp.cmp(&a.timestamp)));
      db.txs.insert((chain, owner), out);
   }

   Ok(db)
}

fn merge_by_hash(dst: &mut Vec<TransactionRich>, src: Vec<TransactionRich>) {
   let existing: HashSet<TxHash> = dst.iter().map(|tx| tx.hash).collect();

   for tx in src {
      if !existing.contains(&tx.hash) {
         dst.push(tx);
      }
   }
   
   dst.sort_by(|a, b| b.block.cmp(&a.block).then_with(|| b.timestamp.cmp(&a.timestamp)));
}

/// Transaction history store (persisted in `tx_history.db`).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TransactionsDB {
   #[serde(default, with = "serde_txs")]
   txs: Transactions,
}

/// Storage DTO for vault serialization (`TxType` as `u8`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredTx {
   tx_type: u8,
   success: bool,
   chain: u64,
   block: u64,
   timestamp: TimeStamp,
   value_sent: NumericValue,
   value_sent_usd: NumericValue,
   eth_received: NumericValue,
   eth_received_usd: NumericValue,
   tx_cost: NumericValue,
   tx_cost_usd: NumericValue,
   hash: TxHash,
   contract_interact: bool,
   analysis: TransactionAnalysis,
   main_event: DecodedEvent,
   #[serde(default)]
   clear_display: Option<ClearDisplay>,
}

impl From<&TransactionRich> for StoredTx {
   fn from(tx: &TransactionRich) -> Self {
      Self {
         tx_type: tx.tx_type as u8,
         success: tx.success,
         chain: tx.chain,
         block: tx.block,
         timestamp: tx.timestamp,
         value_sent: tx.value_sent.clone(),
         value_sent_usd: tx.value_sent_usd.clone(),
         eth_received: tx.eth_received.clone(),
         eth_received_usd: tx.eth_received_usd.clone(),
         tx_cost: tx.tx_cost.clone(),
         tx_cost_usd: tx.tx_cost_usd.clone(),
         hash: tx.hash,
         contract_interact: tx.contract_interact,
         analysis: tx.analysis.clone(),
         main_event: tx.main_event.clone(),
         clear_display: tx.clear_display.clone(),
      }
   }
}

impl TryFrom<StoredTx> for TransactionRich {
   type Error = anyhow::Error;

   fn try_from(s: StoredTx) -> Result<Self, Self::Error> {
      let tx_type = TxType::try_from(s.tx_type)
         .map_err(|_| anyhow::anyhow!("invalid tx_type byte: {}", s.tx_type))?;
      Ok(Self {
         tx_type,
         success: s.success,
         chain: s.chain,
         block: s.block,
         timestamp: s.timestamp,
         value_sent: s.value_sent,
         value_sent_usd: s.value_sent_usd,
         eth_received: s.eth_received,
         eth_received_usd: s.eth_received_usd,
         tx_cost: s.tx_cost,
         tx_cost_usd: s.tx_cost_usd,
         hash: s.hash,
         contract_interact: s.contract_interact,
         analysis: s.analysis,
         main_event: s.main_event,
         clear_display: s.clear_display,
      })
   }
}

mod serde_txs {
   use super::*;
   use crate::core::serde_hashmap;

   pub fn serialize<S>(txs: &Transactions, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: Serializer,
   {
      let stored: HashMap<(u64, Address), Vec<StoredTx>> =
         txs.iter().map(|(k, v)| (*k, v.iter().map(StoredTx::from).collect())).collect();
      serde_hashmap::serialize(&stored, serializer)
   }

   pub fn deserialize<'de, D>(deserializer: D) -> Result<Transactions, D::Error>
   where
      D: Deserializer<'de>,
   {
      let stored: HashMap<(u64, Address), Vec<StoredTx>> =
         serde_hashmap::deserialize(deserializer)?;
      let mut txs = Transactions::new();
      for (k, list) in stored {
         let mut out = Vec::with_capacity(list.len());
         for s in list {
            let tx = TransactionRich::try_from(s).map_err(serde::de::Error::custom)?;
            out.push(tx);
         }
         txs.insert(k, out);
      }
      Ok(txs)
   }
}

impl TransactionsDB {
   pub fn new() -> Self {
      Self {
         txs: HashMap::new(),
      }
   }

   /// Append a transaction.
   pub fn add_tx(
      &mut self,
      chain: u64,
      owner: Address,
      tx: TransactionRich,
   ) -> Result<(), anyhow::Error> {
      let entry = self.txs.entry((chain, owner)).or_default();
      entry.push(tx);
      entry.sort_by(|a, b| b.block.cmp(&a.block).then_with(|| b.timestamp.cmp(&a.timestamp)));
      Ok(())
   }

   pub fn get_txs(&self, chain: u64, owner: Address) -> Option<&Vec<TransactionRich>> {
      self.txs.get(&(chain, owner))
   }

   pub fn get_tx_count(&self, chain: u64, owner: Address) -> usize {
      self.txs.get(&(chain, owner)).map_or(0, |v| v.len())
   }

   pub fn all(&self) -> impl Iterator<Item = &TransactionRich> {
      self.txs.values().flat_map(|v| v.iter())
   }

   pub fn get_txs_paged(
      &self,
      chain: u64,
      owner: Address,
      page: usize,
      per_page: usize,
   ) -> Option<Vec<TransactionRich>> {
      self.txs.get(&(chain, owner)).map(|txs| {
         let start = page * per_page;
         let end = (start + per_page).min(txs.len());
         if start >= txs.len() {
            Vec::new()
         } else {
            txs[start..end].to_vec()
         }
      })
   }

   /// Drop tx histories whose owner is not in `wallets`. Returns how many entries were removed.
   pub fn retain_wallets(&mut self, wallets: &HashSet<Address>) -> usize {
      let before = self.txs.len();
      self.txs.retain(|(_chain, owner), _| wallets.contains(owner));
      self.txs.shrink_to_fit();
      before.saturating_sub(self.txs.len())
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::alloy_primitives::{Address, TxHash};

   fn dummy_tx(hash_byte: u8, block: u64) -> TransactionRich {
      let mut tx = TransactionRich::dummy_clear_signed();
      tx.hash = TxHash::repeat_byte(hash_byte);
      tx.block = block;
      tx
   }

   #[test]
   fn redb_roundtrip_persists_owner_list() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("tx_history.db");
      let key = WalletStateKey::generate().unwrap();
      let owner = Address::repeat_byte(1);

      let db = TxDBHandle::open_at(&path, &key).unwrap();
      db.add_tx(1, owner, dummy_tx(2, 10)).unwrap();
      drop(db);

      let loaded = TxDBHandle::open_at(&path, &key).unwrap();
      let got = loaded.get_txs(1, owner).unwrap();
      assert_eq!(got.len(), 1);
      assert_eq!(got[0].hash, TxHash::repeat_byte(2));
      assert_eq!(got[0].block, 10);
   }

   #[test]
   fn import_legacy_writes_redb_and_dedupes() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("tx_history.db");
      let key = WalletStateKey::generate().unwrap();
      let owner = Address::repeat_byte(3);

      let db = TxDBHandle::open_at(&path, &key).unwrap();
      db.add_tx(1, owner, dummy_tx(1, 5)).unwrap();

      let legacy = TxDBHandle::new();
      legacy.add_tx(1, owner, dummy_tx(1, 5)).unwrap();
      legacy.add_tx(1, owner, dummy_tx(2, 6)).unwrap();

      assert!(db.import_legacy(&legacy).unwrap());
      let got = db.get_txs(1, owner).unwrap();
      assert_eq!(got.len(), 2);

      drop(db);
      let loaded = TxDBHandle::open_at(&path, &key).unwrap();
      assert_eq!(loaded.get_tx_count(1, owner), 2);
   }
}
