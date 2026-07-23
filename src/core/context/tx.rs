use std::collections::HashMap;
use zeus_eth::alloy_primitives::Address;

use crate::core::{context::data_dir, serde_hashmap, tx::TransactionRich};
use serde::{Deserialize, Serialize};

pub const TRANSACTIONS_FILE: &str = "transactions.json";

/// Transactions by chain and wallet address
pub type Transactions = HashMap<(u64, Address), Vec<TransactionRich>>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionsDB {
   #[serde(with = "serde_hashmap")]
   pub txs: Transactions,
}

impl TransactionsDB {
   pub fn new() -> Self {
      Self {
         txs: HashMap::new(),
      }
   }

   /// Load from file
   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(TRANSACTIONS_FILE);
      let data = std::fs::read(dir)?;
      let db = serde_json::from_slice(&data)?;
      Ok(db)
   }

   /// Save to file
   pub fn save(&self) -> Result<(), anyhow::Error> {
      let db = serde_json::to_string(&self)?;
      let dir = data_dir()?.join(TRANSACTIONS_FILE);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn add_tx(&mut self, chain: u64, owner: Address, tx: TransactionRich) {
      self.txs.entry((chain, owner)).or_default().push(tx);
      // sort the txs by newest to oldest
      self.txs.get_mut(&(chain, owner)).unwrap().sort_by(|a, b| b.block.cmp(&a.block));
   }

   pub fn get_txs(&self, chain: u64, owner: Address) -> Option<&Vec<TransactionRich>> {
      self.txs.get(&(chain, owner))
   }

   pub fn get_tx_count(&self, chain: u64, owner: Address) -> usize {
      self.txs.get(&(chain, owner)).map_or(0, |v| v.len())
   }

   pub fn get_txs_paged(
      &self,
      chain: u64,
      owner: Address,
      page: usize,
      per_page: usize,
   ) -> Option<Vec<TransactionRich>> {
      self.txs.get(&(chain, owner)).map(|txs| {
         let mut sorted_txs = txs.clone();
         sorted_txs.sort_by(|a, b| b.block.cmp(&a.block));
         let start = page * per_page;
         let end = (start + per_page).min(sorted_txs.len());
         sorted_txs[start..end].to_vec()
      })
   }
}
