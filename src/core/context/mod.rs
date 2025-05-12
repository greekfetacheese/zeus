use super::{
   Wallet,
   providers::{CLIENT_RPS, COMPUTE_UNITS_PER_SECOND, INITIAL_BACKOFF, MAX_RETRIES},
   utils::{data_dir, pool_data_dir},
};
use crate::core::{Account, WalletInfo};
use anyhow::anyhow;
use std::{
   collections::HashMap,
   sync::{Arc, RwLock},
};
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, erc20::ERC20Token},
   types::{ChainId, SUPPORTED_CHAINS},
   utils::NumericValue,
   utils::client::{RpcClient, get_http_client, retry_layer, throttle_layer},
};
use zeus_eth::{
   amm::{DexKind, pool_manager::PoolManagerHandle, uniswap::AnyUniswapPool},
   utils::client::get_client,
};

const CONTACTS_FILE: &str = "contacts.json";

pub mod db;
pub mod providers;

pub use db::{BalanceDB, CurrencyDB, Portfolio, PortfolioDB, TransactionsDB};
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

   pub fn pool_manager(&self) -> PoolManagerHandle {
      self.read(|ctx| ctx.pool_manager.clone())
   }

   /// If pool_data.json has been deleted, we need to re-sync the pools
   pub fn pools_need_resync(&self) -> bool {
      match pool_data_dir() {
         Ok(dir) => !dir.exists(),
         Err(_) => true,
      }
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

   /// Mutable access to the account
   pub fn write_account<R>(&self, writer: impl FnOnce(&mut Account) -> R) -> R {
      writer(&mut self.0.write().unwrap().account)
   }

   pub fn set_account(&self, new_account: Account) {
      self.0.write().unwrap().account = new_account;
   }

   pub fn get_account(&self) -> Account {
      self.read(|ctx| ctx.account.clone())
   }

   /// Get the wallet with the given address
   ///
   /// Should only used if we need the wallet's private key
   ///
   /// Panics if the wallet is not found
   pub fn get_wallet(&self, address: Address) -> Wallet {
      self.read(|ctx| {
         let wallets = ctx.account.wallets();
         let wallet = wallets
            .iter()
            .find(|w| w.info.address == address)
            .cloned()
            .unwrap();
         wallet
      })
   }

   pub fn current_wallet(&self) -> WalletInfo {
      self.read(|ctx| ctx.account.current_wallet.clone())
   }

   pub fn wallet_exists(&self, address: Address) -> bool {
      self.read(|ctx| ctx.account.wallet_address_exists(address))
   }

   pub fn get_wallet_info(&self, address: Address) -> Option<WalletInfo> {
      let mut info = None;
      self.read(|ctx| {
         for wallet in ctx.account.wallets() {
            if wallet.info.address == address {
               info = Some(wallet.info.clone());
               break;
            }
         }
      });
      info
   }

   pub fn wallets_info(&self) -> Vec<WalletInfo> {
      let mut info = Vec::new();
      self.read(|ctx| {
         for wallet in ctx.account.wallets() {
            info.push(wallet.info.clone());
         }
      });
      info
   }

   /// Get a contact by it's address
   pub fn get_contact_by_address(&self, address: &str) -> Option<Contact> {
      self.read(|ctx| {
         ctx.contact_db
            .contacts
            .iter()
            .find(|c| c.address == address)
            .cloned()
      })
   }

   pub async fn get_client_with_id(&self, id: u64) -> Result<RpcClient, anyhow::Error> {
      let rpcs = self.read(|ctx| ctx.providers.get_all_fastest(id));
      let mut client = None;
      for rpc in &rpcs {
         if !rpc.working || !rpc.enabled {
            continue;
         }

         let (retry, throttle) = if rpc.default {
            (
               retry_layer(
                  MAX_RETRIES,
                  INITIAL_BACKOFF,
                  COMPUTE_UNITS_PER_SECOND,
               ),
               throttle_layer(CLIENT_RPS),
            )
         } else {
            (retry_layer(100, 10, 1000), throttle_layer(1000))
         };

         let c = match get_client(&rpc.url, retry, throttle).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  id,
                  e
               );
               continue;
            }
         };
         client = Some(c);
         break;
      }
      if client.is_none() {
         return Err(anyhow!("No clients found for chain {}", id));
      } else {
         Ok(client.unwrap())
      }
   }

   pub fn get_flashbots_client(&self) -> Result<RpcClient, anyhow::Error> {
      self.read(|ctx| ctx.get_flashbots_client())
   }

   pub fn get_flashbots_fast_client(&self) -> Result<RpcClient, anyhow::Error> {
      self.read(|ctx| ctx.get_flashbots_fast_client())
   }

   pub fn chain(&self) -> ChainId {
      self.read(|ctx| ctx.chain.clone())
   }

   pub fn save_balance_db(&self) {
      self.read(|ctx| match ctx.balance_db.save() {
         Ok(_) => {
            tracing::info!("BalanceDB saved");
         }
         Err(e) => {
            tracing::error!("Error saving DB: {:?}", e);
         }
      })
   }

   pub fn save_currency_db(&self) {
      self.read(|ctx| match ctx.currency_db.save() {
         Ok(_) => {
            tracing::info!("CurrencyDB saved");
         }
         Err(e) => {
            tracing::error!("Error saving DB: {:?}", e);
         }
      })
   }

   pub fn save_portfolio_db(&self) {
      self.read(|ctx| match ctx.portfolio_db.save() {
         Ok(_) => {
            tracing::info!("PortfolioDB saved");
         }
         Err(e) => {
            tracing::error!("Error saving DB: {:?}", e);
         }
      })
   }

   pub fn save_contact_db(&self) {
      self.read(|ctx| match ctx.contact_db.save() {
         Ok(_) => {
            tracing::info!("ContactDB saved");
         }
         Err(e) => {
            tracing::error!("Error saving DB: {:?}", e);
         }
      })
   }

   pub fn save_providers(&self) {
      self.read(|ctx| match ctx.providers.save_to_file() {
         Ok(_) => {
            tracing::info!("Providers saved");
         }
         Err(e) => {
            tracing::error!("Error saving DB: {:?}", e);
         }
      })
   }

   pub fn save_pool_manager(&self) -> Result<(), anyhow::Error> {
      let data = self.read(|ctx| ctx.pool_manager.to_string())?;
      let dir = pool_data_dir()?;
      std::fs::write(dir, data)?;
      Ok(())
   }

   pub fn save_all(&self) {
      self.save_balance_db();
      self.save_currency_db();
      self.save_portfolio_db();
      self.save_contact_db();
      self.save_providers();
      self.save_tx_db();
      match self.save_pool_manager() {
         Ok(_) => {
            tracing::info!("Pool Manager saved");
         }
         Err(e) => {
            tracing::error!("Error saving Pool Manager: {:?}", e);
         }
      }
   }

   pub fn save_tx_db(&self) {
      self.read(|ctx| match ctx.tx_db.save() {
         Ok(_) => {
            tracing::info!("TxDB saved");
         }
         Err(e) => {
            tracing::error!("Error saving TxDB: {:?}", e);
         }
      })
   }

   pub fn get_token_balance(
      &self,
      chain: u64,
      owner: Address,
      token: Address,
   ) -> Option<NumericValue> {
      self.read(|ctx| {
         ctx.balance_db
            .get_token_balance(chain, owner, token)
            .cloned()
      })
   }

   /// Return the chains which the owner has balance in
   pub fn get_owner_chains(&self, owner: Address) -> Vec<u64> {
      let mut chains = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let balance = self.get_eth_balance(chain, owner).unwrap_or_default();
         if !balance.is_zero() {
            chains.push(chain);
         }
      }
      chains
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
      let wallets_info = self.wallets_info();

      for wallet in &wallets_info {
         let portfolio = self.get_portfolio(chain, wallet.address);
         tokens.extend(portfolio.erc20_tokens());
      }
      tokens
   }

   /// Calculate and update the portfolio value
   pub fn calculate_portfolio_value(&self, chain: u64, owner: Address) {
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

   pub fn get_eth_price(&self) -> NumericValue {
      let weth = ERC20Token::weth();
      self.get_token_price(&weth).unwrap_or_default()
   }

   pub fn get_currency_price(&self, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         let wrapped_token = ERC20Token::wrapped_native_token(currency.chain_id());
         self.get_token_price(&wrapped_token).unwrap_or_default()
      } else {
         let token = currency.erc20().unwrap();
         self.get_token_price(&token).unwrap_or_default()
      }
   }

   pub fn get_currency_price_opt(&self, currency: &Currency) -> Option<NumericValue> {
      if currency.is_native() {
         let wrapped_token = ERC20Token::wrapped_native_token(currency.chain_id());
         self.get_token_price(&wrapped_token)
      } else {
         let token = currency.erc20().unwrap();
         self.get_token_price(&token)
      }
   }

   pub fn get_currency_value(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> NumericValue {
      let price = self.get_currency_price(currency);
      let balance = self.get_currency_balance(chain, owner, currency);
      NumericValue::value(balance.f64(), price.f64())
   }

   /// Get the value of the given amount in the given currency
   pub fn get_currency_value2(&self, amount: f64, currency: &Currency) -> NumericValue {
      let price = self.get_currency_price(currency);
      NumericValue::value(amount, price.f64())
   }

   pub fn get_currency_balance(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> NumericValue {
      if currency.is_native() {
         self.get_eth_balance(chain, owner).unwrap_or_default()
      } else {
         let token = currency.erc20().unwrap();
         self
            .get_token_balance(chain, owner, token.address)
            .unwrap_or_default()
      }
   }

   pub fn get_currency_balance_opt(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> Option<NumericValue> {
      if currency.is_native() {
         self.get_eth_balance(chain, owner)
      } else {
         let token = currency.erc20().unwrap();
         self.get_token_balance(chain, owner, token.address)
      }
   }

   pub fn get_pool(
      &self,
      chain: u64,
      fee: u32,
      dex: DexKind,
      currency_a: &Currency,
      currency_b: &Currency,
   ) -> Option<AnyUniswapPool> {
      self.read(|ctx| {
         ctx.pool_manager
            .get_pool(chain, dex, fee, currency_a, currency_b)
      })
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

   pub fn connect_dapp(&self, dapp: String) {
      self.write(|ctx| {
         ctx.connected_dapps.connect_dapp(dapp);
      });
   }

   pub fn disconnect_dapp(&self, dapp: &str) {
      self.write(|ctx| {
         ctx.connected_dapps.disconnect_dapp(dapp);
      });
   }

   pub fn remove_dapp(&self, dapp: String) {
      self.write(|ctx| {
         ctx.connected_dapps.remove_dapp(dapp);
      });
   }

   pub fn is_dapp_connected(&self, dapp: &str) -> bool {
      self.read(|ctx| ctx.connected_dapps.is_connected(dapp))
   }
}

#[derive(Debug, Clone, Default)]
pub struct ConnectedDapps {
   pub dapps: HashMap<String, bool>,
}

impl ConnectedDapps {
   pub fn connect_dapp(&mut self, dapp: String) {
      self.dapps.insert(dapp, true);
   }

   pub fn disconnect_dapp(&mut self, dapp: &str) {
      self.dapps.get_mut(dapp).map(|b| *b = false);
   }

   pub fn remove_dapp(&mut self, dapp: String) {
      self.dapps.remove(&dapp);
   }

   pub fn is_connected(&self, dapp: &str) -> bool {
      self.dapps.get(dapp).map(|b| *b).unwrap_or(false)
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
   const MAX_CHARS: usize = 20;

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

   pub fn contact_mut(&mut self, contact: &Contact) -> Option<&mut Contact> {
      self
         .contacts
         .iter_mut()
         .find(|c| c.address == contact.address)
   }

   pub fn add_contact(&mut self, contact: Contact) -> Result<(), anyhow::Error> {
      if contact.name.is_empty() {
         return Err(anyhow!("Contact name cannot be empty"));
      }

      if contact.name.len() > Self::MAX_CHARS {
         return Err(anyhow!(
            "Contact name cannot be longer than {} characters",
            Self::MAX_CHARS
         ));
      }

      // make sure name and address are unique
      if self.contacts.iter().any(|c| c.name == contact.name) {
         return Err(anyhow!(
            "Contact with name {} already exists",
            contact.name
         ));
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
   account: Account,

   pub account_exists: bool,
   pub logged_in: bool,
   pub balance_db: BalanceDB,
   pub currency_db: CurrencyDB,
   pub portfolio_db: PortfolioDB,
   pub contact_db: ContactDB,
   pub tx_db: TransactionsDB,
   pub pool_manager: PoolManagerHandle,
   /// True if we are syncing important data and need to show a msg
   pub data_syncing: bool,
   pub on_startup_syncing: bool,
   pub base_fee: HashMap<u64, BaseFee>,
   pub priority_fee: PriorityFee,
   pub connected_dapps: ConnectedDapps,
}

impl ZeusContext {
   pub fn new() -> Self {
      let mut providers = RpcProviders::default();
      if let Ok(loaded_providers) = RpcProviders::load_from_file() {
         providers.rpcs = loaded_providers.rpcs;
         providers.reset_latency();
         providers.reset_working();
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

      let mut pool_manager = PoolManagerHandle::default();

      let pool_dir_exists = match pool_data_dir() {
         Ok(dir) => dir.exists(),
         Err(e) => {
            tracing::error!("Failed to read pool data dir, {:?}", e);
            false
         }
      };

      if pool_dir_exists {
         let dir = pool_data_dir().unwrap();
         let manager = match PoolManagerHandle::from_dir(&dir) {
            Ok(manager) => manager,
            Err(e) => {
               tracing::error!("Failed to load pool data, {:?}", e);
               PoolManagerHandle::default()
            }
         };
         pool_manager = manager;
      }

      let priority_fee = PriorityFee::default();
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
         on_startup_syncing: false,
         base_fee: HashMap::new(),
         priority_fee,
         connected_dapps: ConnectedDapps::default(),
      }
   }

   /// This only for ETH mainnet
   pub fn get_flashbots_client(&self) -> Result<RpcClient, anyhow::Error> {
      let url = "https://rpc.flashbots.net";
      get_http_client(
         &url,
         retry_layer(100, 10, 1000),
         throttle_layer(5),
      )
   }

   /// This only for ETH mainnet
   pub fn get_flashbots_fast_client(&self) -> Result<RpcClient, anyhow::Error> {
      let url = "https://rpc.flashbots.net/fast";
      get_http_client(
         &url,
         retry_layer(100, 10, 1000),
         throttle_layer(5),
      )
   }

   pub async fn get_client_with_id(&self, id: u64) -> Result<RpcClient, anyhow::Error> {
      let rpc = self.providers.get_rpc(id)?;
      // for default rpcs we use a throttled client
      let (retry, throttle) = if rpc.default {
         (retry_layer(10, 1000, 100), throttle_layer(5))
      } else {
         (retry_layer(100, 10, 1000), throttle_layer(1000))
      };
      get_client(&rpc.url, retry, throttle).await
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

      let client = ctx.get_client_with_id(1).await.unwrap();
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let base_fee = block.header.base_fee_per_gas.unwrap();
      let fee = format_units(base_fee, "gwei").unwrap();
      println!("Ethereum base fee: {}", fee);

      let client = ctx.get_client_with_id(10).await.unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("Optimism base fee: {}", fee);

      let client = ctx.get_client_with_id(56).await.unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("BSC base fee: {}", fee);

      let client = ctx.get_client_with_id(42161).await.unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("Arbitrum base fee: {}", fee);
   }

   #[tokio::test]
   async fn test_priority_fee_suggestion() {
      let ctx = ZeusContext::new();

      for chain in SUPPORTED_CHAINS {
         let client = ctx.get_client_with_id(chain).await.unwrap();
         let fee = client.get_max_priority_fee_per_gas().await.unwrap();
         let fee = format_units(U256::from(fee), "gwei").unwrap();
         println!("Suggested Fee on {}: {}", chain, fee)
      }
   }
}
