use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use alloy_consensus::TxType;
use bincode_next::serde::{decode_from_slice, encode_to_vec};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use zeus_eth::{
   alloy_primitives::{Address, TxHash},
   utils::NumericValue,
};

use crate::core::{
   context::data_dir,
   tx::{DecodedEvent, TransactionAnalysis, TransactionRich},
};
use crate::utils::TimeStamp;

/// redb file for transaction history
pub const TRANSACTIONS_DB_FILE: &str = "transactions.db";

const TXS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("txs");

/// Key: chain(8 BE) || owner(20) || ts_inverted(8 BE) || hash(32) = 68 bytes
const TX_KEY_LEN: usize = 8 + 20 + 8 + 32;

/// Transactions by chain and wallet address (in-memory cache while DB is open)
pub type Transactions = HashMap<(u64, Address), Vec<TransactionRich>>;

#[derive(Clone)]
pub struct TxDBHandle(Arc<RwLock<TransactionsDB>>);

impl TxDBHandle {
   pub fn new() -> Self {
      Self(Arc::new(RwLock::new(TransactionsDB::new())))
   }

   pub fn read<R>(&self, reader: impl FnOnce(&TransactionsDB) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut TransactionsDB) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   pub fn is_open(&self) -> bool {
      self.read(|db| db.is_open())
   }

   pub fn db_path(&self) -> Result<PathBuf, anyhow::Error> {
      TransactionsDB::db_path()
   }

   pub fn open_and_load(&self) -> Result<(), anyhow::Error> {
      self.write(|db| db.open_and_load())
   }

   pub fn close(&self) {
      self.write(|db| db.close())
   }

   pub fn add_tx(
      &self,
      chain: u64,
      owner: Address,
      tx: TransactionRich,
   ) -> Result<(), anyhow::Error> {
      self.write(|db| db.add_tx(chain, owner, tx))
   }

   pub fn get_txs(&self, chain: u64, owner: Address) -> Option<Vec<TransactionRich>> {
      self.read(|db| db.get_txs(chain, owner).cloned())
   }

   pub fn get_tx_count(&self, chain: u64, owner: Address) -> usize {
      self.read(|db| db.get_tx_count(chain, owner))
   }

   pub fn txs_count(&self) -> usize {
      self.read(|db| db.all_cached().count())
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
}

/// On-demand transaction history store.
///
/// The redb handle and decoded cache are only held while open (history UI session).
/// Writes from the send path open the DB briefly when it is closed.
#[derive(Debug, Default)]
pub struct TransactionsDB {
   /// Open redb database (None when closed)
   db: Option<Database>,
   /// Decoded txs loaded for UI use, cleared on [`Self::close`]
   cache: Transactions,
}

/// Storage DTO for bincode.
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
      })
   }
}

fn encode_tx(tx: &TransactionRich) -> Result<Vec<u8>, anyhow::Error> {
   let stored = StoredTx::from(tx);
   encode_to_vec(&stored, bincode_next::config::standard())
      .map_err(|e| anyhow::anyhow!("bincode encode tx: {e}"))
}

fn decode_tx(bytes: &[u8]) -> Result<TransactionRich, anyhow::Error> {
   let (stored, _): (StoredTx, _) = decode_from_slice(bytes, bincode_next::config::standard())
      .map_err(|e| anyhow::anyhow!("bincode decode tx: {e}"))?;
   TransactionRich::try_from(stored)
}

impl TransactionsDB {
   pub fn new() -> Self {
      Self {
         db: None,
         cache: HashMap::new(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.db.is_some()
   }

   pub fn db_path() -> Result<PathBuf, anyhow::Error> {
      Ok(data_dir()?.join(TRANSACTIONS_DB_FILE))
   }

   /// Open the redb file and load all transactions into the memory cache.
   ///
   /// No-op if already open. Migrates `transactions.json` when still present.
   pub fn open_and_load(&mut self) -> Result<(), anyhow::Error> {
      if self.db.is_some() {
         return Ok(());
      }

      let path = Self::db_path()?;
      let db = if path.exists() {
         Database::open(&path)?
      } else {
         Database::create(&path)?
      };

      // Ensure table exists (cheap/no-op when already present)
      {
         let txn = db.begin_write()?;
         {
            let _ = txn.open_table(TXS_TABLE)?;
         }
         txn.commit()?;
      }

      self.cache = Self::load_all(&db)?;
      self.db = Some(db);
      Ok(())
   }

   /// Drop the redb handle and free the in-memory cache.
   pub fn close(&mut self) {
      self.db = None;
      self.cache = HashMap::new();
   }

   /// Append a transaction and persist it.
   ///
   /// If the DB is already open (history UI), updates the cache too.
   /// If closed, opens briefly for the write and closes without keeping a full cache.
   pub fn add_tx(
      &mut self,
      chain: u64,
      owner: Address,
      tx: TransactionRich,
   ) -> Result<(), anyhow::Error> {
      let keep_open = self.db.is_some();

      if self.db.is_none() {
         // Open without loading full cache
         let path = Self::db_path()?;
         let db = if path.exists() {
            Database::open(&path)?
         } else {
            Database::create(&path)?
         };

         {
            let txn = db.begin_write()?;
            {
               let _ = txn.open_table(TXS_TABLE)?;
            }
            txn.commit()?;
         }

         self.db = Some(db);
      }

      {
         let db = self.db.as_ref().ok_or_else(|| anyhow::anyhow!("tx db not open"))?;
         Self::write_tx(db, chain, owner, &tx)?;
      }

      if keep_open {
         let entry = self.cache.entry((chain, owner)).or_default();
         entry.push(tx);
         entry.sort_by(|a, b| b.block.cmp(&a.block).then_with(|| b.timestamp.cmp(&a.timestamp)));
      } else {
         // Temporary open for write only release memory
         self.close();
      }

      Ok(())
   }

   pub fn get_txs(&self, chain: u64, owner: Address) -> Option<&Vec<TransactionRich>> {
      self.cache.get(&(chain, owner))
   }

   pub fn get_tx_count(&self, chain: u64, owner: Address) -> usize {
      self.cache.get(&(chain, owner)).map_or(0, |v| v.len())
   }

   /// Snapshot of all cached txs (newest first across the full set when sorted by caller).
   pub fn all_cached(&self) -> impl Iterator<Item = &TransactionRich> {
      self.cache.values().flat_map(|v| v.iter())
   }

   pub fn get_txs_paged(
      &self,
      chain: u64,
      owner: Address,
      page: usize,
      per_page: usize,
   ) -> Option<Vec<TransactionRich>> {
      self.cache.get(&(chain, owner)).map(|txs| {
         let start = page * per_page;
         let end = (start + per_page).min(txs.len());
         if start >= txs.len() {
            Vec::new()
         } else {
            txs[start..end].to_vec()
         }
      })
   }

   fn load_all(db: &Database) -> Result<Transactions, anyhow::Error> {
      let mut txs: Transactions = HashMap::new();
      let read_txn = db.begin_read()?;
      let table = match read_txn.open_table(TXS_TABLE) {
         Ok(t) => t,
         Err(redb::TableError::TableDoesNotExist(_)) => return Ok(txs),
         Err(e) => return Err(e.into()),
      };

      for item in table.iter()? {
         let (_key, value) = item?;
         let tx = decode_tx(value.value())?;
         let chain = tx.chain;
         let owner = tx.sender();
         txs.entry((chain, owner)).or_default().push(tx);
      }

      for list in txs.values_mut() {
         list.sort_by(|a, b| b.block.cmp(&a.block).then_with(|| b.timestamp.cmp(&a.timestamp)));
      }

      Ok(txs)
   }

   fn write_tx(
      db: &Database,
      chain: u64,
      owner: Address,
      tx: &TransactionRich,
   ) -> Result<(), anyhow::Error> {
      let key = tx_key(chain, owner, &tx.timestamp, &tx.hash);
      let value = encode_tx(tx)?;

      let txn = db.begin_write()?;
      {
         let mut table = txn.open_table(TXS_TABLE)?;
         table.insert(key.as_slice(), value.as_slice())?;
      }
      txn.commit()?;
      Ok(())
   }
}

fn timestamp_sort_key(ts: &TimeStamp) -> u64 {
   match ts {
      TimeStamp::Seconds(s) => *s,
      TimeStamp::Millis(m) => m / 1000,
   }
}

fn tx_key(chain: u64, owner: Address, timestamp: &TimeStamp, hash: &TxHash) -> Vec<u8> {
   let mut key = Vec::with_capacity(TX_KEY_LEN);
   key.extend_from_slice(&chain.to_be_bytes());
   key.extend_from_slice(owner.as_slice());
   // Newest first under range scans
   let inverted = u64::MAX - timestamp_sort_key(timestamp);
   key.extend_from_slice(&inverted.to_be_bytes());
   key.extend_from_slice(hash.as_slice());
   key
}
