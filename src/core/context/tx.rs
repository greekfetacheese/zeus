use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use alloy_consensus::TxType;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeus_eth::{
   alloy_primitives::{Address, TxHash},
   utils::NumericValue,
};

use crate::core::tx::{DecodedEvent, TransactionAnalysis, TransactionRich};
use crate::core::clear_signing::ClearDisplay;
use crate::utils::TimeStamp;

/// Transactions by chain and wallet address
pub type Transactions = HashMap<(u64, Address), Vec<TransactionRich>>;

#[derive(Clone)]
pub struct TxDBHandle(Arc<RwLock<TransactionsDB>>);

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
      Ok(Self(Arc::new(RwLock::new(db))))
   }
}

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
      self.write(|db| db.retain_wallets(wallets))
   }
}

/// Transaction history store (persisted inside the encrypted vault).
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

   /// Append a transaction (persisted with the vault on shutdown / vault save).
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
