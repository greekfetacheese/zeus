use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr};

use crate::core::{context::data_dir, serde_hashmap};

use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token, NativeCurrency},
   types::{BSC, ETH, ETH_SEPOLIA},
};

use bincode_next::{Decode, Encode, config::standard, decode_from_slice};

const FILE_NAME: &str = "tokens.json";
pub const TOKENS: &[u8] = include_bytes!("../../../token_data.data");

#[derive(Clone, Encode, Decode)]
pub struct TokenData {
   pub chain_id: u64,
   pub address: String,
   pub name: String,
   pub symbol: String,
   pub decimals: u8,
   pub icon_data_x32: Vec<u8>,
   pub icon_data_x24: Vec<u8>,
}

type TokenMap = HashMap<Address, ERC20Token>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyDB {
   #[serde(with = "serde_hashmap")]
   pub tokens: HashMap<u64, TokenMap>,
}

impl Default for CurrencyDB {
   fn default() -> Self {
      let mut currency_db = CurrencyDB::new();
      currency_db.load_default_tokens().unwrap_or_default();
      currency_db
   }
}

impl CurrencyDB {
   fn new() -> Self {
      Self {
         tokens: HashMap::new(),
      }
   }
   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(FILE_NAME);
      let data = std::fs::read(dir)?;
      let mut db: CurrencyDB = serde_json::from_slice(&data)?;

      match db.load_default_tokens() {
         Ok(_) => {}
         Err(e) => {
            tracing::error!("Failed to load default tokens: {:?}", e);
            return Ok(db);
         }
      };

      Ok(db)
   }

   pub fn save(&self) -> Result<(), anyhow::Error> {
      let db = serde_json::to_string(&self)?;
      let dir = data_dir()?.join(FILE_NAME);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn get_currencies(&self, chain_id: u64) -> Vec<Currency> {
      let mut currencies = Vec::new();

      let native = NativeCurrency::from(chain_id);
      currencies.push(Currency::from(native));

      let tokens = self.tokens.get(&chain_id);

      if let Some(tokens) = tokens {
         for (_, token) in tokens {
            currencies.push(Currency::from(token.clone()));
         }
      }
      currencies
   }

   /// Get an ERC20Token for the given chain and address
   pub fn get_erc20_token(&self, chain_id: u64, address: Address) -> Option<ERC20Token> {
      if let Some(tokens) = self.tokens.get(&chain_id) {
         tokens.get(&address).cloned()
      } else {
         None
      }
   }

   pub fn insert_currency(&mut self, chain_id: u64, currency: Currency) {
      if currency.is_erc20() {
         self.insert_token(chain_id, currency.to_erc20().into_owned());
      }
   }

   pub fn insert_token(&mut self, chain_id: u64, token: ERC20Token) {
      if let Some(tokens) = self.tokens.get_mut(&chain_id) {
         tokens.insert(token.address, token);
      } else {
         let mut tokens = HashMap::new();
         tokens.insert(token.address, token);
         self.tokens.insert(chain_id, tokens);
      }
   }

   pub fn remove_token(&mut self, chain_id: u64, address: Address) {
      if let Some(tokens) = self.tokens.get_mut(&chain_id) {
         tokens.remove(&address);
      }
   }

   pub fn load_default_tokens(&mut self) -> Result<(), anyhow::Error> {
      let default_tokens = load_default_tokens()?;

      let weth = ERC20Token::weth();
      let dai = ERC20Token::dai();
      let wbnb = ERC20Token::wbnb();

      for token in default_tokens {
         if token.address == weth.address {
            continue;
         }

         if token.address == dai.address {
            continue;
         }

         self.insert_token(token.chain_id, token);
      }

      self.insert_token(BSC, wbnb);

      // Fix for WETH on mainnet cause it has WETH as name instead of Wrapped Ether
      self.insert_token(ETH, weth);

      // Fix for DAI on mainnet cause it has DAI as name instead of Dai Stablecoin
      self.insert_token(ETH, dai);

      // Sepolia Testnet
      let sepolia_weth = ERC20Token::weth_sepolia();
      let sepolia_dai = ERC20Token::dai_sepolia();
      let sepolia_usdc = ERC20Token::usdc_sepolia();

      self.insert_token(ETH_SEPOLIA, sepolia_weth);
      self.insert_token(ETH_SEPOLIA, sepolia_dai);
      self.insert_token(ETH_SEPOLIA, sepolia_usdc);

      Ok(())
   }
}

fn load_default_tokens() -> Result<Vec<ERC20Token>, anyhow::Error> {
   let (default_tokens, _bytes_read): (Vec<TokenData>, usize) =
      decode_from_slice(TOKENS, standard())?;

   let mut tokens = Vec::new();

   for token in default_tokens {
      let address = Address::from_str(&token.address)?;
      let erc20 = ERC20Token {
         chain_id: token.chain_id,
         address,
         name: token.name.clone().into(),
         symbol: token.symbol.clone().into(),
         decimals: token.decimals,
         total_supply: Default::default(),
      };
      tokens.push(erc20);
   }

   Ok(tokens)
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
