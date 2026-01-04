use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::core::{context::data_dir, serde_hashmap};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   types::{ARBITRUM, BASE, BSC, ETH, OPTIMISM},
};

use bincode::{Decode, Encode, config::standard, decode_from_slice};

const FILE_NAME: &str = "currencies.json";
pub const TOKENS: &[u8] = include_bytes!("../../../token_data.data");

#[derive(Clone, Encode, Decode)]
pub struct TokenData {
   pub chain_id: u64,
   pub address: String,
   pub name: String,
   pub symbol: String,
   pub decimals: u8,
   pub icon_data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyDB {
   #[serde(with = "serde_hashmap")]
   pub currencies: HashMap<u64, Arc<Vec<Currency>>>,
   #[serde(with = "serde_hashmap")]
   pub tokens: HashMap<(u64, Address), ERC20Token>,
}

impl Default for CurrencyDB {
   fn default() -> Self {
      let mut currency_db = CurrencyDB::new();
      currency_db.load_default_currencies().unwrap_or_default();
      currency_db.build_map();
      currency_db
   }
}

impl CurrencyDB {
   fn new() -> Self {
      Self {
         currencies: HashMap::new(),
         tokens: HashMap::new(),
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

   pub fn build_map(&mut self) {
      for currencies_arc in self.currencies.values() {
         for currency in currencies_arc.iter() {
            if let Some(token) = currency.erc20_opt() {
               self.tokens.insert((token.chain_id, token.address), token.clone());
            }
         }
      }
      self.tokens.shrink_to_fit();
   }

   pub fn get_currencies(&self, chain_id: u64) -> Arc<Vec<Currency>> {
      self.currencies.get(&chain_id).cloned().unwrap_or_default()
   }

   /// Get an ERC20Token for the given chain and address
   pub fn get_erc20_token(&self, chain_id: u64, address: Address) -> Option<ERC20Token> {
      self.tokens.get(&(chain_id, address)).cloned()
   }

   /// Remove any duplicate currencies
   pub fn dedup(&mut self) {
      for (_, currencies) in self.currencies.iter_mut() {
         let currencies_mut = Arc::make_mut(currencies);
         currencies_mut.dedup();
         currencies_mut.shrink_to_fit();
      }
   }

   pub fn insert_currency(&mut self, chain_id: u64, currency: Currency) {
      if let Some(currencies_arc) = self.currencies.get_mut(&chain_id) {
         let currencies = Arc::make_mut(currencies_arc);
         if currencies.contains(&currency) {
            tracing::info!("Currency already in DB: {:?}", currency.address());
            return;
         }
         currencies.push(currency.clone());
      } else {
         self.currencies.insert(chain_id, Arc::new(vec![currency.clone()]));
      }
      if currency.is_erc20() {
         let erc20 = currency.to_erc20().into_owned();
         self.tokens.insert((chain_id, erc20.address), erc20);
      }
   }

   pub fn remove_currency(&mut self, chain_id: u64, currency: &Currency) {
      if let Some(currencies_arc) = self.currencies.get_mut(&chain_id) {
         let currencies = Arc::make_mut(currencies_arc);
         currencies.retain(|c| c != currency);
      }
      if currency.is_erc20() {
         self.tokens.remove(&(chain_id, currency.address()));
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
         decode_from_slice(TOKENS, standard())?;

      let weth = ERC20Token::weth();
      let dai = ERC20Token::dai();
      for token in default_tokens {
         let address = Address::from_str(&token.address)?;
         if address == weth.address {
            continue;
         }

         if address == dai.address {
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

      // Fix for WETH on mainnet cause it has WETH as name instead of Wrapped Ether
      self.insert_currency(ETH, weth.into());

      // Fix for DAI on mainnet cause it has DAI as name instead of Dai Stablecoin
      self.insert_currency(ETH, dai.into());

      Ok(())
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_save() {
      let currency_db = CurrencyDB::default();
      currency_db.save().unwrap();
   }
}
