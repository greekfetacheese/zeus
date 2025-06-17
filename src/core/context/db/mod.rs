pub mod balances;
pub mod currencies;
pub mod portfolio;

pub use balances::BalanceDB;
pub use currencies::CurrencyDB;
pub use portfolio::{Portfolio, PortfolioDB};

use crate::core::{
   serde_hashmap,
   utils::{data_dir, tx::TxSummary},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeus_eth::{alloy_primitives::{Address, U256}, amm::{DexKind, FeeAmount}, currency::Currency, utils::NumericValue};

pub const TRANSACTIONS_FILE: &str = "transactions.json";

pub const V3_POSITIONS_FILE: &str = "v3_positions.json";

/// Transactions by chain and wallet address
pub type Transactions = HashMap<(u64, Address), Vec<TxSummary>>;


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

   pub fn add_tx(&mut self, chain: u64, owner: Address, summary: TxSummary) {
      self.txs.entry((chain, owner)).or_default().push(summary);
      // sort the txs by newest to oldest
      self
         .txs
         .get_mut(&(chain, owner))
         .unwrap()
         .sort_by(|a, b| b.block.cmp(&a.block));
   }

   pub fn get_txs(&self, chain: u64, owner: Address) -> Option<&Vec<TxSummary>> {
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
   ) -> Option<Vec<TxSummary>> {
      self.txs.get(&(chain, owner)).map(|txs| {
         let mut sorted_txs = txs.clone();
         sorted_txs.sort_by(|a, b| b.block.cmp(&a.block));
         let start = page * per_page;
         let end = (start + per_page).min(sorted_txs.len());
         sorted_txs[start..end].to_vec()
      })
   }
}

/// Uniswap V3 Positions by chain and wallet address
pub type V3Positions = HashMap<(u64, Address), Vec<V3Position>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3Position {
   pub chain_id: u64,
   pub owner: Address,
   pub dex: DexKind,
   /// The block which this position was created
   pub block: u64,
   pub timestamp: u64,
   /// Id of the position
   pub id: U256,
   /// Nonce for permits
   pub nonce: U256,
   /// Address that is approved for spending
   pub operator: Address,
   pub token0: Currency,
   pub token1: Currency,
   /// Fee tier of the pool
   pub fee: FeeAmount,
   pub pool_address: Address,
   pub tick_lower: i32,
   pub tick_upper: i32,
   pub liquidity: u128,
   pub fee_growth_inside0_last_x128: U256,
   pub fee_growth_inside1_last_x128: U256,
   /// Amount0 of token0
   pub amount0: NumericValue,
   /// Amount1 of token1
   pub amount1: NumericValue,
   /// Unclaimed fees
   pub tokens_owed0: NumericValue,
   /// Unclaimed fees
   pub tokens_owed1: NumericValue,

   pub apr: f64, 
}

impl PartialEq for V3Position {
   fn eq(&self, other: &Self) -> bool {
      self.id == other.id
   }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct V3PositionsDB {
   #[serde(with = "serde_hashmap")]
   pub positions: V3Positions,
}

impl V3PositionsDB {
   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(V3_POSITIONS_FILE);
      let data = std::fs::read(dir)?;
      let db = serde_json::from_slice(&data)?;
      Ok(db)
   }

   pub fn save(&self) -> Result<(), anyhow::Error> {
      let db = serde_json::to_string(&self)?;
      let dir = data_dir()?.join(V3_POSITIONS_FILE);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn get(&self, chain: u64, owner: Address) -> Vec<V3Position> {
      self
         .positions
         .get(&(chain, owner))
         .cloned()
         .unwrap_or_default()
   }

   pub fn insert(&mut self, chain: u64, owner: Address, position: V3Position) {
      self.remove(chain, owner, &position);
      self.positions.entry((chain, owner)).or_default().push(position);
   }

   pub fn remove(&mut self, chain: u64, owner: Address, position: &V3Position) {
      self
         .positions
         .get_mut(&(chain, owner))
         .map(|p| p.retain(|p| p != position));
   }
}
