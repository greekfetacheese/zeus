use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

use crate::core::{serde_helpers, utils::data_dir};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   types::ChainId,
   utils::{NumericValue, format_number},
};

const FILE_NAME: &str = "portfolio.json";

/// Currencies that the user owns,
///
/// since we dont have access to any 3rd party indexers to auto populate this data
///
/// the user has to add them manually
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Portfolio {
   /// The tokens that we have in the portofolio
   pub currencies: Vec<Currency>,

   /// The owner of the portfolio
   pub owner: Address,

   /// Native Balance
   pub balance: NumericValue,

   /// USD value
   pub value: NumericValue,
}

impl Portfolio {
   /// An empty portfolio with the native currency of the chain
   pub fn empty(chain: u64, owner: Address) -> Self {
      let chain = ChainId::new(chain).unwrap_or_default();
      let currencies = vec![Currency::from_native(
         NativeCurrency::from_chain_id(chain.id()).unwrap(),
      )];
      Self {
         currencies,
         owner,
         balance: NumericValue::default(),
         value: NumericValue::default(),
      }
   }

   pub fn new(currencies: Vec<Currency>, owner: Address) -> Self {
      Self {
         currencies,
         owner,
         balance: NumericValue::default(),
         value: NumericValue::default(),
      }
   }

   pub fn update_balance(&mut self, balance: U256, currency_decimals: u8) {
      self.balance = NumericValue::currency_balance(balance, currency_decimals);
   }

   pub fn update_value(&mut self, value: f64) {
      let formatted = format_number(&value.to_string(), 4, true);
      self.value = NumericValue {
         wei: None,
         f64: value,
         formatted,
      };
   }

   pub fn add_currency(&mut self, currency: Currency) {
      self.currencies.push(currency);
   }

   pub fn remove_currency(&mut self, currency: &Currency) {
      self.currencies.retain(|c| c != currency);
   }

   /// Return all the ERC20 tokens in the portfolio
   pub fn erc20_tokens(&self) -> Vec<ERC20Token> {
      let mut tokens = Vec::new();
      for currency in &self.currencies {
         if currency.is_erc20() {
            tokens.push(currency.erc20().cloned().unwrap());
         }
      }
      tokens
   }

   pub fn currencies(&self) -> &Vec<Currency> {
      &self.currencies
   }
}

impl From<Arc<Portfolio>> for Portfolio {
   fn from(portfolio: Arc<Portfolio>) -> Self {
      Self {
         currencies: portfolio.currencies.clone(),
         owner: portfolio.owner.clone(),
         balance: portfolio.balance.clone(),
         value: portfolio.value.clone(),
      }
   }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortfolioDB {
   #[serde(with = "serde_helpers")]
   pub portfolios: HashMap<(u64, Address), Arc<Portfolio>>,
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

   pub fn get_portfolio(&self, chain_id: u64, owner: Address) -> Option<Arc<Portfolio>> {
      let key = (chain_id, owner);
      self.portfolios.get(&key).cloned()
   }

   pub fn add_currency(&mut self, chain_id: u64, owner: Address, currency: Currency) {
      let portfolio = self.get_portfolio_mut(chain_id, owner);
      if portfolio.is_none() {
         let mut portfolio = Portfolio::empty(chain_id, owner);
         portfolio.add_currency(currency.clone());
         self.insert_portfolio(chain_id, owner, portfolio);
      } else {
         let portfolio = portfolio.unwrap();
         portfolio.add_currency(currency.clone());
      }
   }

   pub fn remove_currency(&mut self, chain_id: u64, owner: Address, currency: &Currency) {
      let portfolio = self.get_portfolio_mut(chain_id, owner);
      if portfolio.is_none() {
         return;
      }
      let portfolio = portfolio.unwrap();
      portfolio.remove_currency(currency);
   }

   pub fn get_portfolio_mut(&mut self, chain_id: u64, owner: Address) -> Option<&mut Portfolio> {
      let key = (chain_id, owner);
      self.portfolios.get_mut(&key).map(|arc| Arc::make_mut(arc))
   }

   pub fn insert_portfolio(&mut self, chain_id: u64, owner: Address, portfolio: Portfolio) {
      let key = (chain_id, owner);
      self.portfolios.insert(key, Arc::new(portfolio));
   }
}
