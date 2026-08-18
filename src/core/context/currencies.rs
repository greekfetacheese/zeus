use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::core::{WalletStateKey, context::data_dir, serde_hashmap};
use crate::embedded::TOKEN_DATA;
use crate::utils::write_private_atomic;

use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token, NativeCurrency},
   types::{BSC, ETH, ETH_SEPOLIA},
};

use bincode_next::{Decode, Encode, config::standard, decode_from_slice};

const FILE_NAME: &str = "tokens.data";

/// Bound ciphertext to this logical slot (AAD).
const CURRENCY_DB_AAD: &[u8] = b"zeus-currency-db-v1";

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
   pub fn load_from_file(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let dir = Self::dir()?;
      let sealed = std::fs::read(&dir)?;
      let mut db: CurrencyDB = key.open_json(&sealed, CURRENCY_DB_AAD)?;

      match db.load_default_tokens() {
         Ok(_) => {}
         Err(e) => {
            tracing::error!("Failed to load default tokens: {:?}", e);
            return Ok(db);
         }
      };

      Ok(db)
   }

   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let sealed = key.seal_json(self, CURRENCY_DB_AAD)?;
      let dir = Self::dir()?;
      write_private_atomic(&dir, &sealed)?;
      Ok(())
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      Ok(data_dir()?.join(FILE_NAME))
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
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

   pub fn get_token_name(&self, chain_id: u64, address: Address) -> Option<Arc<str>> {
      if let Some(tokens) = self.tokens.get(&chain_id) {
         tokens.get(&address).map(|token| token.name.clone())
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
      decode_from_slice(TOKEN_DATA, standard())?;

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
   fn test_seal_open_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let db = CurrencyDB::default();
      let sealed = key.seal_json(&db, CURRENCY_DB_AAD).unwrap();
      let loaded: CurrencyDB = key.open_json(&sealed, CURRENCY_DB_AAD).unwrap();
      assert!(!loaded.tokens.is_empty());
      assert!(key.open_json::<CurrencyDB>(&sealed, b"wrong-aad").is_err());
   }
}
