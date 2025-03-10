use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::core::{serde_helpers, utils::*};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   types,
};
use zeus_token_list::{ARBITRUM, BASE, BINANCE_SMART_CHAIN, ETHEREUM, OPTIMISM, tokens::UniswapToken};

const FILE_NAME: &str = "currencies.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyDB {
   #[serde(with = "serde_helpers")]
   pub currencies: HashMap<u64, Arc<Vec<Currency>>>,
}

impl Default for CurrencyDB {
    fn default() -> Self {
        let mut currencies = CurrencyDB::new();
        currencies.load_default_currencies().unwrap_or_default();
        currencies
    }
}

impl CurrencyDB {

   pub fn new() -> Self {
      Self {
         currencies: HashMap::new(),
      }
   }

    pub fn load_from_file() -> Result<Self, anyhow::Error> {
        let dir = data_dir()?.join(FILE_NAME);
        let data = std::fs::read(dir)?;
        let db = serde_json::from_slice(&data)?;
        Ok(db)
    }

    pub fn save(&self) -> Result<(), anyhow::Error> {
        let db = serde_json::to_string(&self)?;
        let dir = data_dir()?.join(FILE_NAME);
        std::fs::write(dir, db)?;
        Ok(())
    }

   pub fn get_currencies(&self, chain_id: u64) -> Arc<Vec<Currency>> {
      self.currencies.get(&chain_id).cloned().unwrap_or_default()
   }

   /// Remove any duplicate currencies
   pub fn dedup(&mut self) {
      for (_, currencies) in self.currencies.iter_mut() {
         let currencies_mut = Arc::make_mut(currencies);
         currencies_mut.dedup();
      }
   }

   pub fn insert_currency(&mut self, chain_id: u64, currency: Currency) {
      if let Some(currencies_arc) = self.currencies.get_mut(&chain_id) {
         let currencies = Arc::make_mut(currencies_arc);
         currencies.push(currency);
      } else {
         self.currencies.insert(chain_id, Arc::new(vec![currency]));
      }
   }

   pub fn remove_currency(&mut self, chain_id: u64, currency: &Currency) {
      if let Some(currencies_arc) = self.currencies.get_mut(&chain_id) {
         let currencies = Arc::make_mut(currencies_arc);
         currencies.retain(|c| c != currency);
      }
   }

   pub fn load_default_currencies(&mut self) -> Result<(), anyhow::Error> {
      // Native Currencies

      // Ethereum
      let eth_native = NativeCurrency::from_chain_id(types::ETH)?;
      self.insert_currency(types::ETH, Currency::from_native(eth_native.clone()));

      // Binance Smart Chain
      let bnb_native = NativeCurrency::from_chain_id(types::BSC)?;
      self.insert_currency(types::BSC, Currency::from_native(bnb_native));

      // Optimism
      self.insert_currency(types::OPTIMISM, Currency::from_native(eth_native.clone()));

      // Base Network
      self.insert_currency(types::BASE, Currency::from_native(eth_native.clone()));

      // Arbitrum
      self.insert_currency(types::ARBITRUM, Currency::from_native(eth_native));

      // Load the default token list
      let mut default_tokens: Vec<ERC20Token> = Vec::new();
      let eth_tokens: Vec<UniswapToken> = serde_json::from_str(ETHEREUM)?;
      let op_tokens: Vec<UniswapToken> = serde_json::from_str(OPTIMISM)?;
      let base_tokens: Vec<UniswapToken> = serde_json::from_str(BASE)?;
      let arbitrum_tokens: Vec<UniswapToken> = serde_json::from_str(ARBITRUM)?;
      let bnb_tokens: Vec<UniswapToken> = serde_json::from_str(BINANCE_SMART_CHAIN)?;

      for token in eth_tokens {
         let erc20 = ERC20Token {
            address: Address::from_str(&token.address)?,
            chain_id: token.chain_id,
            symbol: token.symbol,
            name: token.name,
            decimals: token.decimals,
            total_supply: U256::ZERO,
         };
         default_tokens.push(erc20);
      }

      for token in op_tokens {
         let erc20 = ERC20Token {
            address: Address::from_str(&token.address)?,
            chain_id: token.chain_id,
            symbol: token.symbol,
            name: token.name,
            decimals: token.decimals,
            total_supply: U256::ZERO,
         };
         default_tokens.push(erc20);
      }

      for token in base_tokens {
         let erc20 = ERC20Token {
            address: Address::from_str(&token.address)?,
            chain_id: token.chain_id,
            symbol: token.symbol,
            name: token.name,
            decimals: token.decimals,
            total_supply: U256::ZERO,
         };
         default_tokens.push(erc20);
      }

      for token in arbitrum_tokens {
         let erc20 = ERC20Token {
            address: Address::from_str(&token.address)?,
            chain_id: token.chain_id,
            symbol: token.symbol,
            name: token.name,
            decimals: token.decimals,
            total_supply: U256::ZERO,
         };
         default_tokens.push(erc20);
      }

      for token in bnb_tokens {
         let erc20 = ERC20Token {
            address: Address::from_str(&token.address)?,
            chain_id: token.chain_id,
            symbol: token.symbol,
            name: token.name,
            decimals: token.decimals,
            total_supply: U256::ZERO,
         };
         default_tokens.push(erc20);
      }

      for token in default_tokens {
         let chain_id = token.chain_id;
         let currency = Currency::from_erc20(token);
         self.insert_currency(chain_id, currency);
      }

      Ok(())
   }
}
