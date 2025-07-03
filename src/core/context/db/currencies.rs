use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::core::{serde_hashmap, utils::*};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   types::{ARBITRUM, BASE, BSC, ETH, OPTIMISM},
};
use zeus_token_list::{TOKEN_DATA, TokenData};

use bincode::{config::standard, decode_from_slice};

const FILE_NAME: &str = "currencies.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyDB {
   #[serde(with = "serde_hashmap")]
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

   /// Get the ERC20Tokens for the given chain
   pub fn get_erc20_tokens(&self, chain_id: u64) -> Vec<ERC20Token> {
      let currencies = self.get_currencies(chain_id);
      let mut tokens = Vec::new();
      for currency in currencies.iter() {
         if let Some(token) = currency.erc20() {
            tokens.push(token.clone());
         }
      }
      tokens
   }

   /// Get an ERC20Token for the given chain and address
   pub fn get_erc20_token(&self, chain_id: u64, address: Address) -> Option<ERC20Token> {
      let tokens = self.get_erc20_tokens(chain_id);
      tokens.iter().find(|t| t.address == address).cloned()
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
         if currencies.iter().any(|c| c == &currency) {
            return;
         }
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
      // Ethereum
      let eth = NativeCurrency::from(ETH);
      self.insert_currency(ETH, Currency::from(eth));

      // Binance Smart Chain
      let bnb_native = NativeCurrency::from(BSC);
      self.insert_currency(BSC, Currency::from(bnb_native));

      let wbnb = ERC20Token::wrapped_native_token(BSC);
      self.insert_currency(BSC, Currency::from(wbnb));

      // Optimism
      let eth_op = NativeCurrency::from(OPTIMISM);
      self.insert_currency(OPTIMISM, Currency::from(eth_op));

      // Base Network
      let eth_base = NativeCurrency::from(BASE);
      self.insert_currency(BASE, Currency::from(eth_base));

      // Arbitrum
      let eth_arb = NativeCurrency::from(ARBITRUM);
      self.insert_currency(ARBITRUM, Currency::from(eth_arb));

      // Load the default token list
      let (default_tokens, _bytes_read): (Vec<TokenData>, usize) =
         decode_from_slice(TOKEN_DATA, standard())?;

      let weth = ERC20Token::weth();
      for token in default_tokens {
         let address = Address::from_str(&token.address)?;
         if address == weth.address {
            continue;
         }

         let erc20 = ERC20Token {
            address,
            chain_id: token.chain_id,
            symbol: token.symbol,
            name: token.name,
            decimals: token.decimals,
            total_supply: U256::ZERO,
         };

         self.insert_currency(token.chain_id, erc20.into());
      }

      // Fix for WETH on mainnet cause it has WETH as name
      self.insert_currency(ETH, weth.into());

      Ok(())
   }
}
