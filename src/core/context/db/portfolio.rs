use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::{serde_hashmap, context::data_dir};
use zeus_eth::{alloy_primitives::Address, currency::{ERC20Token, Currency}, utils::NumericValue};

const FILE_NAME: &str = "portfolio.json";

/// Currencies that the user owns,
///
/// since we dont have access to any 3rd party indexers to auto populate this data
///
/// the user has to add them manually
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Portfolio {
   pub tokens: Vec<Currency>,
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

   pub fn add_token(&mut self, token: Currency) {
      self.tokens.push(token);
   }

   pub fn has_token(&self, token: &Currency) -> bool {
      self.tokens.contains(token)
   }

   pub fn remove_token(&mut self, token: &Currency) {
      self.tokens.retain(|t| t != token);
   }

   pub fn get_tokens(&self) -> Vec<ERC20Token> {
      let mut tokens = Vec::new();
      for token in &self.tokens {
         let erc20 = token.to_erc20().into_owned();
         tokens.push(erc20);
      }
      tokens
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
      let dir = data_dir()?.join(FILE_NAME);
      let data = std::fs::read(dir)?;
      let db = serde_json::from_slice(&data)?;
      Ok(db)
   }

   /// Save to file
   pub fn save(&self) -> Result<(), anyhow::Error> {
      let db = serde_json::to_string(&self)?;
      let dir = data_dir()?.join(FILE_NAME);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn get(&self, chain_id: u64, owner: Address) -> Portfolio {
      let key = (chain_id, owner);
      self
         .portfolios
         .get(&key)
         .cloned()
         .unwrap_or(Portfolio::new(owner, chain_id))
   }

   pub fn get_all(&self, chain_id: u64) -> Vec<Portfolio> {
     let mut portfolios = self.portfolios
         .iter()
         .map(|(_, p)| p.clone())
         .collect::<Vec<_>>();
      portfolios.retain(|p| p.chain_id == chain_id);
      portfolios
   }

   pub fn insert_portfolio(&mut self, chain_id: u64, owner: Address, portfolio: Portfolio) {
      let key = (chain_id, owner);
      self.portfolios.insert(key, portfolio);
   }

   pub fn get_tokens(&self, chain_id: u64, owner: Address) -> Vec<ERC20Token> {
      let portfolio = self.get(chain_id, owner);
      portfolio.get_tokens()
   }
}