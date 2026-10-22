use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

use crate::persisted::{PersistedFile, ensure_data_dir, file_path};
use crate::platform::fs::write_private_atomic;
use crate::{WalletStateKey, serde_hashmap};

use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token, NativeCurrency},
   types::ChainId,
};

/// Bound ciphertext to this logical slot (AAD).
const CURRENCY_DB_AAD: &[u8] = b"zeus-currency-db-v1";

type TokenMap = HashMap<Address, ERC20Token>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyDB {
   #[serde(with = "serde_hashmap")]
   pub tokens: HashMap<u64, TokenMap>,
}

impl Default for CurrencyDB {
   fn default() -> Self {
      let mut currency_db = CurrencyDB::new();
      currency_db.insert_base_tokens();
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
      db.insert_base_tokens();
      Ok(db)
   }

   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      ensure_data_dir()?;
      let sealed = key.seal_json(self, CURRENCY_DB_AAD)?;
      let dir = Self::dir()?;
      write_private_atomic(&dir, &sealed)?;
      Ok(())
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      file_path(PersistedFile::Tokens)
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
         for token in tokens.values() {
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

   /// Insert the base token list for every supported chain.
   ///
   /// The default list is [`ERC20Token::base_tokens`] — the canonical wrapped-native and stablecoin
   /// tokens of each chain — not a snapshot of a third-party token repository. Run on a fresh DB
   /// *and* after loading an existing file, so the base tokens are always present with their
   /// canonical metadata; everything else in `tokens.data` is what the user has added.
   fn insert_base_tokens(&mut self) {
      for chain in ChainId::supported_chains() {
         for token in ERC20Token::base_tokens(chain.id()) {
            self.insert_token(token.chain_id, token);
         }
      }
   }
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

   /// The default list is the base tokens and nothing else — the whole point of the refactor.
   #[test]
   fn default_db_holds_only_the_base_tokens() {
      let db = CurrencyDB::default();

      for chain in ChainId::supported_chains() {
         let base = ERC20Token::base_tokens(chain.id());

         // The native currency plus that chain's base tokens.
         assert_eq!(
            db.get_currencies(chain.id()).len(),
            base.len() + 1
         );

         for token in base {
            assert!(db.get_erc20_token(chain.id(), token.address).is_some());
         }
      }
   }
}
