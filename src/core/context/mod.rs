use super::utils::{data_dir, pool_data_dir};
use crate::core::Account;
use anyhow::anyhow;
use std::{
   collections::HashMap,
   sync::{Arc, RwLock},
};
use zeus_eth::amm::{
   pool_manager::PoolStateManagerHandle,
   uniswap::{
      v2::pool::UniswapV2Pool,
      v3::pool::{FEE_TIERS, UniswapV3Pool},
   },
};
use zeus_eth::{alloy_primitives::Address, utils::client::get_http_client_with_throttle};
use zeus_eth::{
   currency::{Currency, erc20::ERC20Token},
   types::{ChainId, SUPPORTED_CHAINS},
   utils::NumericValue,
   utils::client::{HttpClient, get_http_client},
};

const CONTACTS_FILE: &str = "contacts.json";

pub mod db;
pub mod providers;

pub use db::{BalanceDB, CurrencyDB, TransactionsDB, Portfolio, PortfolioDB};
pub use providers::{Rpc, RpcProviders};

#[derive(Clone)]
pub struct ZeusCtx(Arc<RwLock<ZeusContext>>);

impl ZeusCtx {
   pub fn new() -> Self {
      Self(Arc::new(RwLock::new(ZeusContext::new())))
   }

   /// Shared access to the context
   pub fn read<R>(&self, reader: impl FnOnce(&ZeusContext) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   /// Exclusive mutable access to the context
   pub fn write<R>(&self, writer: impl FnOnce(&mut ZeusContext) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   pub fn pool_manager(&self) -> PoolStateManagerHandle {
      self.read(|ctx| ctx.pool_manager.clone())
   }

   /// If pool_data.json has been deleted, we need to re-sync the pools
   pub fn pools_need_resync(&self) -> bool {
      match pool_data_dir() {
         Ok(dir) => !dir.exists(),
         Err(_) => true,
      }
   }

   pub fn save_pool_data(&self) -> Result<(), anyhow::Error> {
      let data = self.read(|ctx| ctx.pool_manager.to_string().ok());
      if let Some(data) = data {
         let dir = pool_data_dir()?;
         std::fs::write(dir, data)?;
      }
      Ok(())
   }

   pub fn account_exists(&self) -> bool {
      self.read(|ctx| ctx.account_exists)
   }

   pub fn logged_in(&self) -> bool {
      self.read(|ctx| ctx.logged_in)
   }

   pub fn rpc_providers(&self) -> RpcProviders {
      self.read(|ctx| ctx.providers.clone())
   }

   pub fn account(&self) -> Account {
      self.read(|ctx| ctx.account.clone())
   }

   pub fn get_client_with_id(&self, id: u64) -> Result<HttpClient, anyhow::Error> {
      self.read(|ctx| ctx.get_client_with_id(id))
   }

   pub fn chain(&self) -> ChainId {
      self.read(|ctx| ctx.chain.clone())
   }

   pub fn save_balance_db(&self) -> Result<(), anyhow::Error> {
      self.read(|ctx| ctx.balance_db.save())
   }

   pub fn save_currency_db(&self) -> Result<(), anyhow::Error> {
      self.read(|ctx| ctx.currency_db.save())
   }

   pub fn save_portfolio_db(&self) -> Result<(), anyhow::Error> {
      self.read(|ctx| ctx.portfolio_db.save())
   }

   pub fn save_contact_db(&self) -> Result<(), anyhow::Error> {
      self.read(|ctx| ctx.contact_db.save())
   }

   pub fn save_providers(&self) -> Result<(), anyhow::Error> {
      self.read(|ctx| ctx.providers.save_to_file())
   }

   pub fn save_all(&self) -> Result<(), anyhow::Error> {
      self.save_balance_db()?;
      self.save_currency_db()?;
      self.save_portfolio_db()?;
      self.save_contact_db()?;
      self.save_providers()?;
      Ok(())
   }

   pub fn save_tx_db(&self) -> Result<(), anyhow::Error> {
      self.read(|ctx| ctx.tx_db.save())
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> Option<NumericValue> {
      self.read(|ctx| {
         ctx.balance_db
            .get_token_balance(chain, owner, token)
            .cloned()
      })
   }

   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> Option<NumericValue> {
      self.read(|ctx| ctx.balance_db.get_eth_balance(chain, owner).cloned())
   }

   pub fn get_currencies(&self, chain: u64) -> Arc<Vec<Currency>> {
      self.read(|ctx| ctx.currency_db.get_currencies(chain))
   }

   pub fn get_portfolio(&self, chain: u64, owner: Address) -> Arc<Portfolio> {
      let portfolio = self.read(|ctx| ctx.portfolio_db.get_portfolio(chain, owner));
      if let Some(portfolio) = portfolio {
         portfolio
      } else {
         let portfolio = Portfolio::empty(chain, owner);
         self.write(|ctx| {
            ctx.portfolio_db
               .insert_portfolio(chain, owner, portfolio.clone())
         });
         Arc::new(portfolio)
      }
   }

   /// Get the portfolio value across all chains
   pub fn get_portfolio_value_all_chains(&self, owner: Address) -> NumericValue {
      let mut value = 0.0;
      let chains = SUPPORTED_CHAINS.to_vec();

      for chain in chains {
         let portfolio = self.get_portfolio(chain, owner);
         value += portfolio.value.f64();
      }

      NumericValue::from_f64(value)
   }

   /// Get all the erc20 tokens in all portfolios
   pub fn get_all_erc20_tokens(&self, chain: u64) -> Vec<ERC20Token> {
      let mut tokens = Vec::new();
      let wallets = self.account().wallets;

      for wallet in &wallets {
         let owner = wallet.address();
         let portfolio = self.get_portfolio(chain, owner);
         tokens.extend(portfolio.erc20_tokens());
      }
      tokens
   }

   /// Calculate and update the portfolio value
   pub fn update_portfolio_value(&self, chain: u64, owner: Address) {
      let mut portfolio = Portfolio::from(self.get_portfolio(chain, owner));
      let currencies = portfolio.currencies();
      let mut value = 0.0;

      for currency in currencies {
         let price = self.get_currency_price(currency).f64();
         let balance = self.get_currency_balance(chain, owner, currency).f64();
         value += NumericValue::value(balance, price).f64()
      }

      portfolio.update_value(value);
      self.write(|ctx| {
         // override the existing portfolio
         ctx.portfolio_db.insert_portfolio(chain, owner, portfolio);
      });
   }

   pub fn contacts(&self) -> Vec<Contact> {
      self.read(|ctx| ctx.contact_db.contacts.clone())
   }

   pub fn get_token_price(&self, token: &ERC20Token) -> Option<NumericValue> {
      self.read(|ctx| ctx.pool_manager.get_token_price(token))
   }

   pub fn get_currency_price(&self, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         let wrapped_token = ERC20Token::native_wrapped_token(currency.chain_id());
         self.get_token_price(&wrapped_token).unwrap_or_default()
      } else {
         let token = currency.erc20().unwrap();
         self.get_token_price(&token).unwrap_or_default()
      }
   }

   pub fn get_currency_price_opt(&self, currency: &Currency) -> Option<NumericValue> {
      if currency.is_native() {
         let wrapped_token = ERC20Token::native_wrapped_token(currency.chain_id());
         self.get_token_price(&wrapped_token)
      } else {
         let token = currency.erc20().unwrap();
         self.get_token_price(&token)
      }
   }

   pub fn get_currency_value(&self, chain: u64, owner: Address, currency: &Currency) -> NumericValue {
      let price = self.get_currency_price(currency);
      let balance = self.get_currency_balance(chain, owner, currency);
      NumericValue::value(balance.f64(), price.f64())
   }

   pub fn get_currency_balance(&self, chain: u64, owner: Address, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         self.get_eth_balance(chain, owner).unwrap_or_default()
      } else {
         let token = currency.erc20().unwrap();
         self
            .get_token_balance(chain, owner, token.address)
            .unwrap_or_default()
      }
   }

   pub fn get_currency_balance_opt(&self, chain: u64, owner: Address, currency: &Currency) -> Option<NumericValue> {
      if currency.is_native() {
         self.get_eth_balance(chain, owner)
      } else {
         let token = currency.erc20().unwrap();
         self.get_token_balance(chain, owner, token.address)
      }
   }

   /// Get the v2 pool for the pair, token order does not matter
   pub fn get_v2_pool(&self, chain: u64, token0: Address, token1: Address) -> Option<UniswapV2Pool> {
      self.read(|ctx| ctx.pool_manager.get_v2_pool(chain, token0, token1))
   }

   /// Get the v3 pool for the pair, token order does not matter
   pub fn get_v3_pool(&self, chain: u64, fee: u32, token0: Address, token1: Address) -> Option<UniswapV3Pool> {
      self.read(|ctx| ctx.pool_manager.get_v3_pool(chain, fee, token0, token1))
   }

   pub fn add_v2_pools(&self, pools: Vec<UniswapV2Pool>) {
      self.write(|ctx| ctx.pool_manager.add_v2_pools(pools));
   }

   pub fn add_v3_pools(&self, pools: Vec<UniswapV3Pool>) {
      self.write(|ctx| ctx.pool_manager.add_v3_pools(pools));
   }

   /// Get all v3 pools that include the given token and [FEE_TIERS]
   pub fn get_v3_pools(&self, token: &ERC20Token) -> Vec<UniswapV3Pool> {
      let base_tokens = ERC20Token::base_tokens(token.chain_id);
      let mut pools = Vec::new();

      for base_token in base_tokens {
         if base_token.address == token.address {
            continue;
         }

         for fee in FEE_TIERS {
            if let Some(pool) = self.get_v3_pool(token.chain_id, fee, base_token.address, token.address) {
               pools.push(pool);
            }
         }
      }
      pools
   }

   /// Get all v2 pools for the given token
   pub fn get_v2_pools(&self, token: &ERC20Token) -> Vec<UniswapV2Pool> {
      let base_tokens = ERC20Token::base_tokens(token.chain_id);
      let mut pools = Vec::new();

      for base_token in base_tokens {
         if base_token.address == token.address {
            continue;
         }

         if let Some(pool) = self.get_v2_pool(token.chain_id, base_token.address, token.address) {
            pools.push(pool);
         }
      }
      pools
   }

   pub fn get_base_fee(&self, chain: u64) -> Option<BaseFee> {
      self.read(|ctx| ctx.base_fee.get(&chain).cloned())
   }

   pub fn get_priority_fee(&self, chain: u64) -> Option<NumericValue> {
      self.read(|ctx| ctx.priority_fee.get(chain).cloned())
   }

   pub fn update_base_fee(&self, chain: u64, base_fee: u64, next_base_fee: u64) {
      self.write(|ctx| {
         ctx.base_fee
            .insert(chain, BaseFee::new(base_fee, next_base_fee));
      });
   }

   pub fn update_priority_fee(&self, chain: u64, fee: NumericValue) {
      self.write(|ctx| {
         ctx.priority_fee.fee.insert(chain, fee);
      });
   }
}

/// Saved contact by the user
#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Contact {
   pub name: String,
   pub address: String,
   pub notes: String,
}

impl Contact {
   pub fn new(name: String, address: String, notes: String) -> Self {
      Self {
         name,
         address,
         notes,
      }
   }

   pub fn address_short(&self) -> String {
      format!("{}...{}", &self.address[..6], &self.address[36..])
   }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ContactDB {
   pub contacts: Vec<Contact>,
}

impl ContactDB {
   pub fn new() -> Self {
      Self {
         contacts: Vec::new(),
      }
   }
   /// Load from file
   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(CONTACTS_FILE);
      let data = std::fs::read(dir)?;
      let db = serde_json::from_slice(&data)?;
      Ok(db)
   }

   /// Save to file
   pub fn save(&self) -> Result<(), anyhow::Error> {
      let db = serde_json::to_string(&self)?;
      let dir = data_dir()?.join(CONTACTS_FILE);
      std::fs::write(dir, db)?;
      Ok(())
   }

   pub fn contact_address_exists(&self, address: &str) -> bool {
      self.contacts.iter().any(|c| &c.address == address)
   }

   pub fn contact_name_exists(&self, name: &str) -> bool {
      self.contacts.iter().any(|c| &c.name == name)
   }

   pub fn add_contact(&mut self, contact: Contact) -> Result<(), anyhow::Error> {
      // make sure name and address are unique
      if self.contacts.iter().any(|c| c.name == contact.name) {
         return Err(anyhow!("Contact with name {} already exists", contact.name));
      } else if self.contacts.iter().any(|c| c.address == contact.address) {
         return Err(anyhow!(
            "Contact with address {} already exists",
            contact.address
         ));
      }
      self.contacts.push(contact);
      Ok(())
   }

   pub fn remove_contact(&mut self, address: String) {
      self.contacts.retain(|c| c.address != address);
   }
}

#[derive(Debug, Clone)]
pub struct BaseFee {
   pub current: u64,
   pub next: u64,
}

impl Default for BaseFee {
   fn default() -> Self {
      Self {
         current: 1,
         next: 1,
      }
   }
}

impl BaseFee {
   pub fn new(current: u64, next: u64) -> Self {
      Self { current, next }
   }
}

/// Suggested priority fees for each chain
#[derive(Debug, Clone)]
pub struct PriorityFee {
   pub fee: HashMap<u64, NumericValue>,
}

impl PriorityFee {
   pub fn get(&self, chain: u64) -> Option<&NumericValue> {
      self.fee.get(&chain)
   }
}

impl Default for PriorityFee {
   fn default() -> Self {
      let mut map = HashMap::new();
      // Eth
      map.insert(1, NumericValue::parse_to_gwei("1"));

      // Optimism
      map.insert(10, NumericValue::parse_to_gwei("0.002"));

      // BSC (Legacy Tx)
      map.insert(56, NumericValue::parse_to_gwei("0"));

      // Base
      map.insert(8453, NumericValue::parse_to_gwei("0.002"));

      // Arbitrum (Legacy Tx)
      map.insert(42161, NumericValue::parse_to_gwei("0"));

      Self { fee: map }
   }
}

pub struct ZeusContext {
   pub providers: RpcProviders,

   /// The current selected chain from the GUI
   pub chain: ChainId,

   /// Loaded account
   pub account: Account,

   pub account_exists: bool,

   pub logged_in: bool,

   pub balance_db: BalanceDB,
   pub currency_db: CurrencyDB,
   pub portfolio_db: PortfolioDB,
   pub contact_db: ContactDB,
   pub tx_db: TransactionsDB,

   pub pool_manager: PoolStateManagerHandle,

   /// True if we are syncing important data and need to show a msg
   pub data_syncing: bool,

   pub base_fee: HashMap<u64, BaseFee>,
   pub priority_fee: PriorityFee,
}

impl ZeusContext {
   pub fn new() -> Self {
      let mut providers = RpcProviders::default();
      if let Ok(loaded_providers) = RpcProviders::load_from_file() {
         providers.rpcs = loaded_providers.rpcs;
      }

      let contact_db = match ContactDB::load_from_file() {
         Ok(db) => db,
         Err(e) => {
            tracing::error!("Failed to load contacts, {:?}", e);
            ContactDB::default()
         }
      };

      let balance_db = match BalanceDB::load_from_file() {
         Ok(db) => db,
         Err(e) => {
            tracing::error!("Failed to load balances, {:?}", e);
            BalanceDB::default()
         }
      };

      let currency_db = match CurrencyDB::load_from_file() {
         Ok(db) => db,
         Err(e) => {
            tracing::error!("Failed to load currencies, {:?}", e);
            CurrencyDB::default()
         }
      };

      let portfolio_db = match PortfolioDB::load_from_file() {
         Ok(db) => db,
         Err(e) => {
            tracing::error!("Failed to load portfolios, {:?}", e);
            PortfolioDB::default()
         }
      };

      let tx_db = match TransactionsDB::load_from_file() {
         Ok(db) => db,
         Err(e) => {
            tracing::error!("Failed to load transactions, {:?}", e);
            TransactionsDB::default()
         }
      };

      let account_exists = Account::exists().is_ok_and(|p| p);

      let mut pool_manager = PoolStateManagerHandle::default();

      let pool_dir_exists = match pool_data_dir() {
         Ok(dir) => dir.exists(),
         Err(e) => {
            tracing::error!("Failed to read pool data dir, {:?}", e);
            false
         }
      };

      if pool_dir_exists {
         let dir = pool_data_dir().unwrap();
         let manager = match PoolStateManagerHandle::from_dir(&dir) {
            Ok(manager) => manager,
            Err(e) => {
               tracing::error!("Failed to load pool data, {:?}", e);
               PoolStateManagerHandle::default()
            }
         };
         pool_manager = manager;
      }

      Self {
         providers,
         chain: ChainId::new(1).unwrap(),
         account: Account::default(),
         account_exists,
         logged_in: false,
         balance_db,
         currency_db,
         portfolio_db,
         contact_db,
         tx_db,
         pool_manager,
         data_syncing: false,
         base_fee: HashMap::new(),
         priority_fee: PriorityFee::default(),
      }
   }

   pub fn get_client_with_id(&self, id: u64) -> Result<HttpClient, anyhow::Error> {
      let rpc = self.providers.get_rpc(id)?;
      // for default rpcs we use a throttled client
      let client = if rpc.default {
         get_http_client_with_throttle(&rpc.url)?
      } else {
         get_http_client(&rpc.url)?
      };
      Ok(client)
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::{
      abi::alloy_provider::Provider,
      alloy_primitives::{U256, utils::format_units},
      alloy_rpc_types::BlockId,
      types::SUPPORTED_CHAINS,
   };

   #[tokio::test]
   async fn test_base_fee() {
      let ctx = ZeusContext::new();

      let client = ctx.get_client_with_id(1).unwrap();
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let base_fee = block.header.base_fee_per_gas.unwrap();
      let fee = format_units(base_fee, "gwei").unwrap();
      println!("Ethereum base fee: {}", fee);

      let client = ctx.get_client_with_id(10).unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("Optimism base fee: {}", fee);

      let client = ctx.get_client_with_id(56).unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("BSC base fee: {}", fee);

      let client = ctx.get_client_with_id(42161).unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("Arbitrum base fee: {}", fee);
   }

   #[tokio::test]
   async fn test_priority_fee_suggestion() {
      let ctx = ZeusContext::new();

      for chain in SUPPORTED_CHAINS {
         let client = ctx.get_client_with_id(chain).unwrap();
         let fee = client.get_max_priority_fee_per_gas().await.unwrap();
         let fee = format_units(U256::from(fee), "gwei").unwrap();
         println!("Suggested Fee on {}: {}", chain, fee)
      }
   }
}
