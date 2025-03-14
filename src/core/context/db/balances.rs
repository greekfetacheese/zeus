use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::{serde_helpers, utils::*};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::NumericValue,
};

const FILE_NAME: &str = "balances.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BalanceDB {
   /// Eth Balances (or any native currency for evm compatable chains)
   #[serde(with = "serde_helpers")]
   pub eth_balances: HashMap<(u64, Address), NumericValue>,

   /// Token Balances
   #[serde(with = "serde_helpers")]
   pub token_balances: HashMap<(u64, Address, Address), NumericValue>,
}

impl BalanceDB {
   pub fn new() -> Self {
      Self {
         eth_balances: HashMap::new(),
         token_balances: HashMap::new(),
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

   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> Option<&NumericValue> {
      self.eth_balances.get(&(chain, owner))
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> Option<&NumericValue> {
      self.token_balances.get(&(chain, owner, token))
   }

   pub fn insert_currency_balance(&mut self, owner: Address, balance: NumericValue, currency: &Currency) {
      if currency.is_native() {
         let native = currency.native().unwrap();
         let balance = balance.wei().unwrap_or_default();
         self.insert_eth_balance(native.chain_id, owner, balance, native);
      } else {
         let token = currency.erc20().unwrap();
         let balance = balance.wei().unwrap_or_default();
         self.insert_token_balance(token.chain_id, owner, balance, token);
      }
   }

   pub fn insert_eth_balance(&mut self, chain: u64, owner: Address, balance: U256, currency: &NativeCurrency) {
      let balance = NumericValue::currency_balance(balance, currency.decimals);
      self.eth_balances.insert((chain, owner), balance);
   }

   pub fn insert_token_balance(&mut self, chain: u64, owner: Address, balance: U256, token: &ERC20Token) {
      let balance = NumericValue::currency_balance(balance, token.decimals);
      self
         .token_balances
         .insert((chain, owner, token.address), balance);
   }
}
