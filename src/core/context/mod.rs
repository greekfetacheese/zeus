use super::{
   providers::{CLIENT_RPS, COMPUTE_UNITS_PER_SECOND, INITIAL_BACKOFF, MAX_RETRIES},
   utils::pool_data_dir,
};
use crate::core::{
   WalletInfo,
   user::{Contact, Vault, Wallet},
   utils::server_port_dir,
};
use crate::server::SERVER_PORT;
use anyhow::anyhow;
use db::V3Position;
use ncrypt_me::Argon2;
use std::{
   collections::HashMap,
   sync::{Arc, RwLock},
   time::{Duration, Instant},
};
use tokio::time::sleep;
use zeus_eth::{
   alloy_primitives::Address,
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   amm::uniswap::{AnyUniswapPool, DexKind},
   currency::{Currency, erc20::ERC20Token},
   types::{ChainId, SUPPORTED_CHAINS},
   utils::{
      NumericValue, address_book,
      client::{RpcClient, get_client, retry_layer, throttle_layer},
   },
};

const CLIENT_TIMEOUT: u64 = 10;

pub mod balance_manager;
pub mod db;
pub mod pool_manager;
// pub mod position_manager;
pub mod providers;

pub use balance_manager::BalanceManagerHandle;
pub use db::{CurrencyDB, Portfolio, PortfolioDB, TransactionsDB, V3PositionsDB};
pub use pool_manager::PoolManagerHandle;
pub use providers::{Rpc, RpcProviders};

/// Thread-safe handle to the [ZeusContext]
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

   pub fn balance_manager(&self) -> BalanceManagerHandle {
      self.read(|ctx| ctx.balance_manager.clone())
   }

   /// If pool_data.json has been deleted, we need to re-sync the pools
   pub fn pools_need_resync(&self) -> bool {
      match pool_data_dir() {
         Ok(dir) => !dir.exists(),
         Err(_) => true,
      }
   }

   pub fn vault_exists(&self) -> bool {
      self.read(|ctx| ctx.vault_exists)
   }

   /// Encrypt and save the vault
   /// 
   /// If `new_vault` is None, the current vault will be encrypted
   /// 
   /// If `new_params` is None, the current [Argon2] params will be used
   pub fn encrypt_and_save_vault(
      &self,
      new_vault: Option<Vault>,
      new_params: Option<Argon2>,
   ) -> Result<(), anyhow::Error> {
      if self.save_vault_in_progress() {
         return Err(anyhow!(
            "Saving account in progress, try again later"
         ));
      }

      self.write(|ctx| ctx.save_vault_in_progress = true);

      let vault = if new_vault.is_some() {
         new_vault.unwrap()
      } else {
         self.get_vault()
      };

      let res = vault.encrypt(new_params);

      if res.is_err() {
         self.write(|ctx| ctx.save_vault_in_progress = false);
         return Err(res.err().unwrap());
      }

      let encrypted_data = res.unwrap();
      let res = vault.save(None, encrypted_data);

      if res.is_err() {
         self.write(|ctx| ctx.save_vault_in_progress = false);
         return Err(res.err().unwrap());
      }

      self.write(|ctx| ctx.save_vault_in_progress = false);
      Ok(())
   }

   pub fn set_save_vault_in_progress(&self, save_vault_in_progress: bool) {
      self.write(|ctx| ctx.save_vault_in_progress = save_vault_in_progress);
   }

   pub fn save_vault_in_progress(&self) -> bool {
      self.read(|ctx| ctx.save_vault_in_progress)
   }

   pub fn wallet_discovery_in_progress(&self) -> bool {
      self.read(|ctx| ctx.wallet_discovery_in_progress)
   }

   pub fn logged_in(&self) -> bool {
      self.read(|ctx| ctx.logged_in)
   }

   pub fn rpc_providers(&self) -> RpcProviders {
      self.read(|ctx| ctx.providers.clone())
   }

   /// Mutable access to the vault
   pub fn write_vault<R>(&self, writer: impl FnOnce(&mut Vault) -> R) -> R {
      writer(&mut self.0.write().unwrap().vault)
   }

   pub fn set_vault(&self, new_vault: Vault) {
      self.0.write().unwrap().vault = new_vault;
   }

   pub fn get_vault(&self) -> Vault {
      self.read(|ctx| ctx.vault.clone())
   }

   pub fn get_master_wallet(&self) -> Wallet {
      self.read(|ctx| ctx.vault.get_master_wallet())
   }

   /// Get the wallet with the given address
   pub fn get_wallet(&self, address: Address) -> Option<Wallet> {
      self.read(|ctx| {
         for wallet in ctx.vault_ref().all_wallets() {
            if wallet.address() == address {
               return Some(wallet.clone());
            }
         }
         None
      })
   }

   /// Is this wallet selected as the current wallet
   pub fn is_current_wallet(&self, address: Address) -> bool {
      self.read(|ctx| ctx.current_wallet.address() == address)
   }

   pub fn get_current_wallet(&self) -> Wallet {
      self.read(|ctx| ctx.current_wallet.clone())
   }

   pub fn current_wallet_info(&self) -> WalletInfo {
      self.read(|ctx| ctx.current_wallet.to_wallet_info())
   }

   pub fn current_wallet_address(&self) -> Address {
      self.read(|ctx| ctx.current_wallet.address())
   }

   pub fn current_wallet_name(&self) -> String {
      self.read(|ctx| ctx.current_wallet.name_with_id())
   }

   pub fn wallet_exists(&self, address: Address) -> bool {
      self.read(|ctx| ctx.vault.wallet_address_exists(address))
   }

   pub fn get_wallet_info_by_address(&self, address: Address) -> Option<WalletInfo> {
      let mut info = None;
      self.read(|ctx| {
         for wallet in ctx.vault_ref().all_wallets() {
            if wallet.address() == address {
               info = Some(wallet.to_wallet_info());
               break;
            }
         }
      });
      info
   }

   pub fn get_all_wallets_info(&self) -> Vec<WalletInfo> {
      let mut info = Vec::new();
      self.read(|ctx| {
         for wallet in ctx.vault_ref().all_wallets() {
            info.push(wallet.to_wallet_info());
         }
      });
      info
   }

   pub fn contacts(&self) -> Vec<Contact> {
      self.read(|ctx| ctx.vault.contacts.clone())
   }

   pub fn remove_contact(&self, address: &str) {
      self.write(|ctx| {
         ctx.vault.contacts.retain(|c| c.address != address);
      });
   }

   pub fn contact_name_exists(&self, name: &str) -> bool {
      self.read(|ctx| ctx.vault.contacts.iter().any(|c| c.name == name))
   }

   pub fn add_contact(&self, contact: Contact) -> Result<(), anyhow::Error> {
      if contact.name.is_empty() {
         return Err(anyhow!("Contact name cannot be empty"));
      }

      let contacts = self.contacts();

      // make sure name and address are unique
      if contacts.iter().any(|c| c.name == contact.name) {
         return Err(anyhow!(
            "Contact with name {} already exists",
            contact.name
         ));
      } else if contacts.iter().any(|c| c.address == contact.address) {
         return Err(anyhow!(
            "Contact with address {} already exists",
            contact.address
         ));
      }

      self.write(|ctx| {
         ctx.vault.contacts.push(contact);
      });
      Ok(())
   }

   /// Get a contact by it's address
   pub fn get_contact_by_address(&self, address: &str) -> Option<Contact> {
      self.read(|ctx| ctx.vault.contacts.iter().find(|c| c.address == address).cloned())
   }

   pub fn client_available(&self, chain: u64) -> bool {
      let rpcs = self.read(|ctx| ctx.providers.get_all_fastest(chain));
      rpcs.iter().any(|rpc| rpc.working && rpc.enabled)
   }

   // * Ignore the enabled flag to avoid mistakes
   pub fn client_mev_protect_available(&self, chain: u64) -> bool {
      let rpcs = self.read(|ctx| ctx.providers.get_all_fastest(chain));
      rpcs.iter().any(|rpc| rpc.working && rpc.mev_protect)
   }

   pub fn client_archive_available(&self, chain: u64) -> bool {
      let rpcs = self.read(|ctx| ctx.providers.get_all_fastest(chain));
      rpcs.iter().any(|rpc| rpc.working && rpc.archive && rpc.enabled)
   }

   pub async fn get_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_TIMEOUT);
      let mut client = None;

      while !self.client_available(chain) {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get client for chain {} Timeout exceeded",
               chain
            ));
         }
         sleep(Duration::from_millis(100)).await;
      }

      let rpcs = self.read(|ctx| ctx.providers.get_all_fastest(chain));
      let fully_functional = rpcs
         .clone()
         .into_iter()
         .filter(|rpc| rpc.fully_functional && rpc.enabled)
         .collect::<Vec<_>>();

      let non_fully_functional = rpcs
         .into_iter()
         .filter(|rpc| !rpc.fully_functional && rpc.enabled)
         .collect::<Vec<_>>();

      for rpc in &fully_functional {
         let c = match self.connect_to_rpc(rpc).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  e
               );
               continue;
            }
         };

         client = Some(c);
         break;
      }

      if client.is_none() {
         for rpc in &non_fully_functional {
            let c = match self.connect_to_rpc(rpc).await {
               Ok(client) => client,
               Err(e) => {
                  tracing::error!(
                     "Error connecting to client using {} for chain {}: {:?}",
                     rpc.url,
                     chain,
                     e
                  );
                  continue;
               }
            };

            client = Some(c);
            break;
         }
      }

      if client.is_none() {
         return Err(anyhow!("No clients found for chain {}", chain));
      } else {
         Ok(client.unwrap())
      }
   }

   async fn connect_to_rpc(&self, rpc: &Rpc) -> Result<RpcClient, anyhow::Error> {
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
         (retry_layer(10, 300, 1000), throttle_layer(1000))
      };

      get_client(&rpc.url, retry, throttle).await
   }

   pub async fn get_archive_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let time_passed = Instant::now();
      let timeout = Duration::from_secs(CLIENT_TIMEOUT);
      let mut client = None;

      while !self.client_archive_available(chain) {
         if time_passed.elapsed() > timeout {
            return Err(anyhow!(
               "Failed to get archive client for chain {} Timeout exceeded",
               chain
            ));
         }
         sleep(Duration::from_millis(100)).await;
      }

      let rpcs = self.read(|ctx| ctx.providers.get_all_fastest(chain));

      for rpc in &rpcs {
         if !rpc.working || !rpc.enabled || !rpc.archive {
            continue;
         }

         let c = match self.connect_to_rpc(rpc).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  e
               );
               continue;
            }
         };

         client = Some(c);
         break;
      }
      if client.is_none() {
         return Err(anyhow!(
            "No archive clients found for chain {}",
            chain
         ));
      } else {
         Ok(client.unwrap())
      }
   }

   pub async fn get_mev_protect_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let mut client = None;

      if !self.client_mev_protect_available(chain) {
         return Err(anyhow!(
            "Failed to get MEV protect client for chain {}",
            chain
         ));
      }

      let rpcs = self.read(|ctx| ctx.providers.get_all_fastest(chain));

      for rpc in &rpcs {
         if !rpc.mev_protect || !rpc.working {
            continue;
         }

         tracing::info!("Using MEV protect RPC: {}", rpc.url);

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
            (retry_layer(10, 300, 1000), throttle_layer(1000))
         };

         let c = match get_client(&rpc.url, retry, throttle).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error connecting to client using {} for chain {}: {:?}",
                  rpc.url,
                  chain,
                  e
               );
               continue;
            }
         };
         client = Some(c);
         break;
      }
      if client.is_none() {
         return Err(anyhow!(
            "No MEV protect clients found for chain {}",
            chain
         ));
      } else {
         Ok(client.unwrap())
      }
   }

   pub fn chain(&self) -> ChainId {
      self.read(|ctx| ctx.chain)
   }

   pub fn save_balance_manager(&self) {
      self.read(|ctx| match ctx.balance_manager.save() {
         Ok(_) => {
            tracing::info!("Balance Manager saved");
         }
         Err(e) => {
            tracing::error!("Error saving Balance Manager: {:?}", e);
         }
      })
   }

   pub fn save_v3_positions_db(&self) {
      self.read(|ctx| match ctx.v3_positions_db.save() {
         Ok(_) => {
            tracing::info!("V3PositionsDB saved");
         }
         Err(e) => {
            tracing::error!("Error saving V3 Positions DB: {:?}", e);
         }
      })
   }

   pub fn save_currency_db(&self) {
      self.read(|ctx| match ctx.currency_db.save() {
         Ok(_) => {
            tracing::info!("CurrencyDB saved");
         }
         Err(e) => {
            tracing::error!("Error saving CurrencyDB: {:?}", e);
         }
      })
   }

   pub fn save_portfolio_db(&self) {
      self.read(|ctx| match ctx.portfolio_db.save() {
         Ok(_) => {
            tracing::info!("PortfolioDB saved");
         }
         Err(e) => {
            tracing::error!("Error saving PortfolioDB: {:?}", e);
         }
      })
   }

   pub fn save_providers(&self) {
      self.read(|ctx| match ctx.providers.save_to_file() {
         Ok(_) => {
            tracing::info!("Providers saved");
         }
         Err(e) => {
            tracing::error!("Error saving Providers: {:?}", e);
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
      self.save_balance_manager();
      self.save_currency_db();
      self.save_portfolio_db();
      self.save_providers();
      self.save_tx_db();
      self.save_v3_positions_db();
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

   /// Return the chains which the owner has balance in
   pub fn get_chains_that_have_balance(&self, owner: Address) -> Vec<u64> {
      let mut chains = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let balance = self.get_eth_balance(chain, owner);
         if !balance.is_zero() {
            chains.push(chain);
         }
      }
      chains
   }

   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> NumericValue {
      self.read(|ctx| ctx.balance_manager.get_eth_balance(chain, owner))
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> NumericValue {
      self.read(|ctx| ctx.balance_manager.get_token_balance(chain, owner, token))
   }

   pub fn get_currencies(&self, chain: u64) -> Arc<Vec<Currency>> {
      self.read(|ctx| ctx.currency_db.get_currencies(chain))
   }

   pub fn get_portfolio(&self, chain: u64, owner: Address) -> Portfolio {
      self.read(|ctx| ctx.portfolio_db.get(chain, owner))
   }

   pub fn has_portfolio(&self, chain: u64, owner: Address) -> bool {
      self.read(|ctx| ctx.portfolio_db.portfolios.contains_key(&(chain, owner)))
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

   /// Get all tokens in all portfolios
   pub fn get_all_tokens_from_portfolios(&self, chain: u64) -> Vec<ERC20Token> {
      let mut tokens = Vec::new();
      let portfolios = self.read(|ctx| ctx.portfolio_db.get_all(chain));

      for portfolio in portfolios {
         let erc_tokens =
            portfolio.tokens.iter().map(|c| c.to_erc20().into_owned()).collect::<Vec<_>>();
         tokens.extend(erc_tokens);
      }
      tokens
   }

   /// Calculate and update the portfolio value
   pub fn calculate_portfolio_value(&self, chain: u64, owner: Address) {
      let mut portfolio = self.get_portfolio(chain, owner);
      let mut value = 0.0;

      for currency in &portfolio.tokens {
         if currency.is_native() {
            continue;
         }

         let price = self.get_currency_price(currency).f64();
         let balance = self.get_currency_balance(chain, owner, currency).f64();
         value += NumericValue::value(balance, price).f64()
      }

      let eth_balance = self.get_eth_balance(chain, owner);
      let eth_price = self.get_currency_price(&Currency::from(ERC20Token::wrapped_native_token(
         chain,
      )));

      let eth_value = NumericValue::value(eth_balance.f64(), eth_price.f64());
      value += eth_value.f64();

      let new_value = NumericValue::from_f64(value);
      portfolio.value = new_value;

      self.write(|ctx| {
         ctx.portfolio_db.insert_portfolio(chain, owner, portfolio);
      });
   }

   pub fn get_v3_positions(&self, chain: u64, owner: Address) -> Vec<V3Position> {
      self.read(|ctx| ctx.v3_positions_db.get(chain, owner))
   }

   pub fn get_token_price(&self, token: &ERC20Token) -> NumericValue {
      self.read(|ctx| ctx.pool_manager.get_token_price(token))
   }

   pub fn get_currency_price(&self, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         let wrapped_token = ERC20Token::wrapped_native_token(currency.chain_id());
         self.get_token_price(&wrapped_token)
      } else {
         let token = currency.erc20().unwrap();
         self.get_token_price(token)
      }
   }

   pub fn get_currency_value_for_owner(
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
   pub fn get_currency_value_for_amount(&self, amount: f64, currency: &Currency) -> NumericValue {
      let price = self.get_currency_price(currency);
      NumericValue::value(amount, price.f64())
   }

   pub fn get_token_value_for_amount(&self, amount: f64, token: &ERC20Token) -> NumericValue {
      let price = self.get_token_price(token);
      NumericValue::value(amount, price.f64())
   }

   pub fn get_currency_balance(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> NumericValue {
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
      self.read(|ctx| ctx.pool_manager.get_pool(chain, dex, fee, currency_a, currency_b))
   }

   pub fn get_base_fee(&self, chain: u64) -> Option<BaseFee> {
      self.read(|ctx| ctx.base_fee.get(&chain).cloned())
   }

   pub fn get_priority_fee(&self, chain: u64) -> Option<NumericValue> {
      self.read(|ctx| ctx.priority_fee.get(chain).cloned())
   }

   pub fn update_base_fee(&self, chain: u64, base_fee: u64, next_base_fee: u64) {
      self.write(|ctx| {
         ctx.base_fee.insert(chain, BaseFee::new(base_fee, next_base_fee));
      });
   }

   pub fn update_priority_fee(&self, chain: u64, fee: NumericValue) {
      self.write(|ctx| {
         ctx.priority_fee.fee.insert(chain, fee);
      });
   }

   /// Return the name of this address if its known
   pub fn get_address_name(&self, chain: u64, address: Address) -> Option<String> {
      let wallet = self.get_wallet_info_by_address(address);
      if wallet.is_some() {
         return Some(wallet.unwrap().name());
      }

      let contact = self.get_contact_by_address(&address.to_string());
      if contact.is_some() {
         return Some(contact.unwrap().name);
      }

      let token = self.read(|ctx| ctx.currency_db.get_erc20_token(chain, address));
      if token.is_some() {
         return Some(token.unwrap().name);
      }

      let permit2 = address_book::permit2_contract(chain).unwrap();
      if permit2 == address {
         return Some("Uniswap Protocol: Permit2".to_string());
      }

      let v4_pool_manager = address_book::uniswap_v4_pool_manager(chain).unwrap();
      if v4_pool_manager == address {
         return Some("Uniswap V4: Pool Manager".to_string());
      }

      let ur_router_v2 = address_book::universal_router_v2(chain).unwrap();
      if ur_router_v2 == address {
         return Some("Uniswap V4: Universal Router V2".to_string());
      }

      let nft_position_manager = address_book::uniswap_nft_position_manager(chain).unwrap();
      if nft_position_manager == address {
         return Some("Uniswap V3: NFT Position Manager".to_string());
      }

      let spoke_pool_address = address_book::across_spoke_pool_v2(chain);
      if spoke_pool_address.is_ok() {
         if spoke_pool_address.unwrap() == address {
            return Some("Across Protocol: Spoke Pool V2".to_string());
         }
      }

      None
   }

   pub fn get_connected_dapps(&self) -> Vec<String> {
      self.read(|ctx| ctx.connected_dapps.connected_dapps())
   }

   pub fn connect_dapp(&self, dapp: String) {
      tracing::info!("Connected to dapp: {}", dapp);

      self.write(|ctx| {
         ctx.connected_dapps.connect_dapp(dapp);
      });
   }

   pub fn disconnect_dapp(&self, dapp: &str) {
      self.write(|ctx| {
         ctx.connected_dapps.disconnect_dapp(dapp);
      });
      tracing::info!("Disconnected from dapp: {}", dapp);
   }

   pub fn disconnect_all_dapps(&self) {
      self.write(|ctx| {
         ctx.connected_dapps.disconnect_all();
      });
      tracing::info!("Disconnected from all dapps");
   }

   pub fn is_dapp_connected(&self, dapp: &str) -> bool {
      self.read(|ctx| ctx.connected_dapps.is_connected(dapp))
   }

   pub async fn get_latest_block(&self) -> Result<Option<Block>, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let epoch = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap_or_default()
         .as_millis();

      let block = self.read(|ctx| ctx.latest_block.get(&chain.id()).cloned());
      if let Some(block) = block {
         // time check
         let elapsed = if epoch > block.timestamp as u128 {
            epoch - block.timestamp as u128
         } else {
            u128::MAX
         };

         if elapsed < block_time as u128 {
            return Ok(Some(block));
         }
      }

      let client = self.get_client(chain.id()).await?;
      let block = client.get_block(BlockId::latest()).await?;

      if let Some(block) = block {
         let block = Block::new(block.header.number, block.header.timestamp);
         self.write(|ctx| {
            ctx.latest_block.insert(chain.id(), block.clone());
         });
         return Ok(Some(block));
      }

      Ok(None)
   }

   pub fn server_port(&self) -> u16 {
      self.read(|ctx| ctx.server_port)
   }

   pub fn save_server_port(&self) -> Result<(), anyhow::Error> {
      let port = self.server_port();
      let dir = server_port_dir()?;
      let string = serde_json::to_string(&port)?;
      std::fs::write(dir, string)?;
      Ok(())
   }
}

#[derive(Debug, Clone, Default)]
pub struct ConnectedDapps {
   pub dapps: Vec<String>,
}

impl ConnectedDapps {
   pub fn connected_dapps(&self) -> Vec<String> {
      self.dapps.clone()
   }

   pub fn connect_dapp(&mut self, dapp: String) {
      self.dapps.push(dapp);
   }

   pub fn disconnect_dapp(&mut self, dapp: &str) {
      self.dapps.retain(|d| d != dapp);
   }

   pub fn disconnect_all(&mut self) {
      self.dapps.clear();
   }

   pub fn is_connected(&self, dapp: &str) -> bool {
      self.dapps.contains(&dapp.to_string())
   }
}

#[derive(Debug, Default, Clone)]
pub struct Block {
   pub number: u64,
   pub timestamp: u64,
}

impl Block {
   pub fn new(number: u64, timestamp: u64) -> Self {
      Self { number, timestamp }
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

   /// The current selected wallet from the GUI
   pub current_wallet: Wallet,

   /// Loaded Vault
   vault: Vault,
   pub save_vault_in_progress: bool,
   pub wallet_discovery_in_progress: bool,

   pub vault_exists: bool,
   pub logged_in: bool,
   pub currency_db: CurrencyDB,
   pub portfolio_db: PortfolioDB,
   pub tx_db: TransactionsDB,
   pub v3_positions_db: V3PositionsDB,
   pub pool_manager: PoolManagerHandle,
   pub balance_manager: BalanceManagerHandle,
   pub data_syncing: bool,
   pub on_startup_syncing: bool,
   pub base_fee: HashMap<u64, BaseFee>,
   pub latest_block: HashMap<u64, Block>,
   pub priority_fee: PriorityFee,
   pub connected_dapps: ConnectedDapps,
   pub server_port: u16,
}

impl ZeusContext {
   pub fn new() -> Self {
      let mut providers = RpcProviders::default();
      if let Ok(loaded_providers) = RpcProviders::load_from_file() {
         providers.rpcs = loaded_providers.rpcs;
         providers.reset_latency();
         providers.reset_working();
      }

      let balance_manager = match BalanceManagerHandle::load_from_file() {
         Ok(db) => db,
         Err(e) => {
            tracing::error!("Failed to load balances, {:?}", e);
            BalanceManagerHandle::default()
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

      let v3_positions_db = match V3PositionsDB::load_from_file() {
         Ok(db) => db,
         Err(e) => {
            tracing::error!("Failed to load v3 positions, {:?}", e);
            V3PositionsDB::default()
         }
      };

      let vault_exists = Vault::exists().is_ok_and(|p| p);

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
      } else {
         tracing::info!("No pool data found, using default");
      }

      let priority_fee = PriorityFee::default();
      Self {
         providers,
         chain: ChainId::new(1).unwrap(),
         current_wallet: Wallet::new_rng("I should not be here".to_string()),
         vault: Vault::default(),
         save_vault_in_progress: false,
         wallet_discovery_in_progress: false,
         vault_exists,
         logged_in: false,
         currency_db,
         portfolio_db,
         tx_db,
         v3_positions_db,
         pool_manager,
         balance_manager,
         data_syncing: false,
         on_startup_syncing: false,
         base_fee: HashMap::new(),
         latest_block: HashMap::new(),
         priority_fee,
         connected_dapps: ConnectedDapps::default(),
         server_port: SERVER_PORT,
      }
   }

   pub fn vault_ref(&self) -> &Vault {
      &self.vault
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::{
      alloy_primitives::{U256, utils::format_units},
      alloy_provider::Provider,
      alloy_rpc_types::BlockId,
      types::SUPPORTED_CHAINS,
   };

   #[tokio::test]
   #[should_panic]
   async fn test_must_panic_if_no_mev_protect_client() {
      let ctx = ZeusCtx::new();
      let _r = ctx.get_mev_protect_client(1).await.unwrap();
   }

   #[tokio::test]
   async fn test_base_fee() {
      let ctx = ZeusCtx::new();

      ctx.write(|ctx| {
         ctx.providers.all_working();
      });

      let client = ctx.get_client(1).await.unwrap();
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let base_fee = block.header.base_fee_per_gas.unwrap();
      let fee = format_units(base_fee, "gwei").unwrap();
      println!("Ethereum base fee: {}", fee);

      let client = ctx.get_client(10).await.unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("Optimism base fee: {}", fee);

      let client = ctx.get_client(56).await.unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("BSC base fee: {}", fee);

      let client = ctx.get_client(42161).await.unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("Arbitrum base fee: {}", fee);
   }

   #[tokio::test]
   async fn test_priority_fee_suggestion() {
      let ctx = ZeusCtx::new();

      for chain in SUPPORTED_CHAINS {
         let client = ctx.get_client(chain).await.unwrap();
         let fee = client.get_max_priority_fee_per_gas().await.unwrap();
         let fee = format_units(U256::from(fee), "gwei").unwrap();
         println!("Suggested Fee on {}: {}", chain, fee)
      }
   }
}
