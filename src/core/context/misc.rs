use crate::core::{TransactionRich, context::data_dir, serde_hashmap};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeus_bip32::{BIP32_HARDEN, DerivationPath};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::ERC20Token,
   utils::NumericValue,
};

pub const PORTFOLIO_FILE: &str = "portfolio.json";
pub const TRANSACTIONS_FILE: &str = "transactions.json";
pub const V3_POSITIONS_FILE: &str = "v3_positions.json";
pub const DISCOVERED_WALLETS_FILE: &str = "discovered_wallets.json";

/// Currencies that the user owns,
///
/// since we dont have access to any 3rd party indexers to auto populate this data
///
/// the user has to add them manually
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Portfolio {
   pub tokens: Vec<ERC20Token>,
   pub chain_id: u64,
   pub owner: Address,
   pub value: NumericValue,
}

impl Portfolio {
   pub fn new(owner: Address, chain_id: u64) -> Self {
      Self {
         tokens: Vec::new(),
         chain_id,
         owner,
         value: NumericValue::default(),
      }
   }

   pub fn add_token(&mut self, token: ERC20Token) {
      if self.tokens.contains(&token) {
         return;
      }
      self.tokens.push(token);
   }

   pub fn has_token(&self, token: &ERC20Token) -> bool {
      self.tokens.contains(token)
   }

   pub fn remove_token(&mut self, token: &ERC20Token) {
      self.tokens.retain(|t| t != token);
   }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortfolioDB {
   #[serde(with = "serde_hashmap")]
   pub portfolios: HashMap<(u64, Address), Portfolio>,
}

impl PortfolioDB {
   pub fn new() -> Self {
      Self {
         portfolios: HashMap::new(),
      }
   }

   /// Load from file
   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(PORTFOLIO_FILE);
      let data = std::fs::read(dir)?;
      let db = serde_json::from_slice(&data)?;
      Ok(db)
   }

   /// Save to file
   pub fn save(&self) -> Result<(), anyhow::Error> {
      let db = serde_json::to_string(&self)?;
      let dir = data_dir()?.join(PORTFOLIO_FILE);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn get(&self, chain_id: u64, owner: Address) -> Portfolio {
      let key = (chain_id, owner);
      self.portfolios.get(&key).cloned().unwrap_or(Portfolio::new(owner, chain_id))
   }

   pub fn get_all(&self, chain_id: u64) -> Vec<Portfolio> {
      let mut portfolios = self.portfolios.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>();
      portfolios.retain(|p| p.chain_id == chain_id);
      portfolios
   }

   pub fn insert_portfolio(&mut self, chain_id: u64, owner: Address, portfolio: Portfolio) {
      let key = (chain_id, owner);
      self.portfolios.insert(key, portfolio);
   }

   pub fn get_tokens(&self, chain_id: u64, owner: Address) -> Vec<ERC20Token> {
      let portfolio = self.get(chain_id, owner);
      portfolio.tokens.clone()
   }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredWallet {
   pub address: Address,
   pub path: DerivationPath,
   pub index: u32,
}

fn default_concurrency() -> usize {
   2
}

fn default_batch_size() -> usize {
   20
}

/// Discovered wallets that derived from a `SecureHDWallet`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredWallets {
   #[serde(with = "serde_hashmap")]
   pub balances: HashMap<(u64, Address), U256>,
   pub master_wallet_address: Option<Address>,
   pub wallets: Vec<DiscoveredWallet>,
   /// Current index, starting from [BIP32_HARDEN]
   pub index: u32,
   /// Number of concurrent requests
   #[serde(default = "default_concurrency")]
   pub concurrency: usize,
   /// Batch size
   #[serde(default = "default_batch_size")]
   pub batch_size: usize,
}

impl DiscoveredWallets {
   pub fn new() -> Self {
      Self {
         balances: HashMap::new(),
         master_wallet_address: None,
         wallets: Vec::new(),
         index: BIP32_HARDEN,
         concurrency: default_concurrency(),
         batch_size: default_batch_size(),
      }
   }

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(DISCOVERED_WALLETS_FILE);
      let data = std::fs::read(dir)?;
      let db = serde_json::from_slice(&data)?;
      Ok(db)
   }

   pub fn save(&self) -> Result<(), anyhow::Error> {
      let db = serde_json::to_string(&self)?;
      let dir = data_dir()?.join(DISCOVERED_WALLETS_FILE);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn add_wallet(&mut self, address: Address, path: DerivationPath, index: u32) {
      self.wallets.push(DiscoveredWallet {
         address,
         path,
         index,
      });
   }
}
