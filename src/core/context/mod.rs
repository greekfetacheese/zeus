use super::utils::{data_dir, pool_data_dir};
use crate::core::{Profile, Wallet};
use anyhow::anyhow;
use providers::{Rpc, RpcProviders};
use std::sync::{Arc, RwLock};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::amm::{
   pool_manager::PoolStateManagerHandle,
   uniswap::{
      v2::pool::UniswapV2Pool,
      v3::pool::{FEE_TIERS, UniswapV3Pool},
   },
};
use zeus_eth::{
   currency::{Currency, erc20::ERC20Token},
   types::ChainId,
   utils::NumericValue,
   utils::client::{HttpClient, get_http_client},
};

const CONTACTS_FILE: &str = "contacts.json";

pub mod db;
pub mod providers;

pub use db::{BalanceDB, CurrencyDB, PortfolioDB, Portfolio};

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

   pub fn save_pool_data(&self) -> Result<(), anyhow::Error> {
      let data = self.read(|ctx| ctx.pool_manager.to_string().ok());
      if let Some(data) = data {
         let dir = pool_data_dir()?;
         std::fs::write(dir, data)?;
      }
      Ok(())
   }

   pub fn profile_exists(&self) -> bool {
      self.read(|ctx| ctx.profile_exists)
   }

   pub fn logged_in(&self) -> bool {
      self.read(|ctx| ctx.logged_in)
   }

   pub fn profile(&self) -> Profile {
      self.read(|ctx| ctx.profile.clone())
   }

   pub fn rpc(&self) -> Rpc {
      self.read(|ctx| ctx.rpc.clone())
   }

   pub fn get_client(&self) -> Result<HttpClient, anyhow::Error> {
      self.read(|ctx| ctx.get_client())
   }

   pub fn get_client_with_id(&self, id: u64) -> Result<HttpClient, anyhow::Error> {
      self.read(|ctx| ctx.get_client_with_id(id))
   }

   pub fn wallet(&self) -> Wallet {
      self.read(|ctx| ctx.wallet())
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

   pub fn save_all(&self) -> Result<(), anyhow::Error> {
      self.save_balance_db()?;
      self.save_currency_db()?;
      self.save_portfolio_db()?;
      self.save_contact_db()?;
      Ok(())
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> NumericValue {
      self.read(|ctx| {
         ctx.balance_db
            .get_token_balance(chain, owner, token)
            .cloned()
            .unwrap_or_default()
      })
   }

   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> NumericValue {
      self.read(|ctx| {
         ctx.balance_db
            .get_eth_balance(chain, owner)
            .cloned()
            .unwrap_or_default()
      })
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

   /// Calculate and update the portfolio value
   pub fn update_portfolio_value(&self, chain: u64, owner: Address) {
      let mut portfolio = Portfolio::from(self.get_portfolio(chain, owner));
      let currencies = portfolio.currencies();
      let mut value = 0.0;

      for currency in currencies {
         let price = self.get_currency_price(currency).float();
         let balance = self.get_currency_balance(chain, owner, currency).float();
         value += NumericValue::currency_value(balance, price).float()
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

   pub fn get_token_price(&self, token: &ERC20Token) -> NumericValue {
      self.read(|ctx| ctx.pool_manager.get_token_price(token).unwrap_or_default())
   }

   pub fn get_currency_price(&self, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         let wrapped_token = ERC20Token::native_wrapped_token(currency.chain_id());
         self.get_token_price(&wrapped_token)
      } else {
         let token = currency.erc20().unwrap();
         self.get_token_price(&token)
      }
   }

   /// Get the currency's value in USD for the given chain and owner
   pub fn get_currency_value(&self, chain: u64, owner: Address, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         let token = ERC20Token::native_wrapped_token(chain);
         let price = self.get_token_price(&token);
         let balance = self.get_eth_balance(chain, owner);
         return NumericValue::currency_value(balance.float(), price.float());
      } else {
         let token = currency.erc20().unwrap();
         let price = self.get_token_price(token);
         let balance = self.get_token_balance(chain, owner, token.address);
         return NumericValue::currency_value(balance.float(), price.float());
      }
   }

   pub fn get_currency_balance(&self, chain: u64, owner: Address, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         self.get_eth_balance(chain, owner)
      } else {
         let token = currency.erc20().unwrap();
         self.get_token_balance(chain, owner, token.address)
      }
   }

   /// Get the v2 pool for the given tokens, token order does not matter
   pub fn get_v2_pool(&self, chain: u64, token0: Address, token1: Address) -> Option<UniswapV2Pool> {
      self.read(|ctx| ctx.pool_manager.get_v2_pool(chain, token0, token1))
   }

   /// Get the v3 pool for the given tokens, token order does not matter
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
   pub fn get_v3_pools(&self, token: ERC20Token) -> Vec<UniswapV3Pool> {
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

   /// Get all v2 pools for the given pair
   pub fn get_v2_pools(&self, token: ERC20Token) -> Vec<UniswapV2Pool> {
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

pub struct ZeusContext {
   pub providers: RpcProviders,

   /// The current selected rpc provider from the GUI
   pub rpc: Rpc,

   /// The current selected chain from the GUI
   pub chain: ChainId,

   /// Loaded profile
   pub profile: Profile,

   pub profile_exists: bool,

   pub logged_in: bool,

   pub balance_db: BalanceDB,
   pub currency_db: CurrencyDB,
   pub portfolio_db: PortfolioDB,
   pub contact_db: ContactDB,

   pub pool_manager: PoolStateManagerHandle,

   pub pool_data_syncing: bool,
}

impl ZeusContext {
   pub fn new() -> Self {
      let mut providers = RpcProviders::default();
      if let Ok(loaded_providers) = RpcProviders::load_from_file() {
         providers.rpc = loaded_providers.rpc;
      }

      let contact_db = ContactDB::load_from_file().unwrap_or_default();
      let balance_db = BalanceDB::load_from_file().unwrap_or_default();
      let currency_db = CurrencyDB::load_from_file().unwrap_or_default();
      let portfolio_db = PortfolioDB::load_from_file().unwrap_or_default();

      let profile_exists = Profile::exists().expect("Failed to read data directory");
      let rpc = providers.get(1).expect("Failed to find provider");

      let pool_dir = pool_data_dir().unwrap().exists();
      let mut pool_manager = PoolStateManagerHandle::default();
      if pool_dir {
         let dir = pool_data_dir().unwrap();
         let data = std::fs::read(dir).unwrap();
         let manager = PoolStateManagerHandle::from_slice(&data).unwrap();
         pool_manager = manager;
      }

      Self {
         providers,
         rpc,
         chain: ChainId::new(1).unwrap(),
         profile: Profile::default(),
         profile_exists,
         logged_in: false,
         balance_db,
         currency_db,
         portfolio_db,
         contact_db,
         pool_manager,
         pool_data_syncing: false,
      }
   }

   pub fn get_client(&self) -> Result<HttpClient, anyhow::Error> {
      let rpc = self.providers.get(self.chain.id())?;
      let client = get_http_client(&rpc.url)?;
      Ok(client)
   }

   pub fn get_client_with_id(&self, id: u64) -> Result<HttpClient, anyhow::Error> {
      let rpc = self.providers.get(id)?;
      let client = get_http_client(&rpc.url)?;
      Ok(client)
   }

   /// Get the current wallet selected from the GUI
   pub fn wallet(&self) -> Wallet {
      self.profile.current_wallet.clone()
   }
}
