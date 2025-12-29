use super::{
   BalanceManagerHandle, CurrencyDB, PoolManagerHandle, ZeusClient, misc::*,
   price_manager::PriceManagerHandle,
};

use crate::core::{Vault, WalletInfo, client::Rpc, serde_hashmap};
use crate::server::SERVER_PORT;
use crate::utils::{RT, state::test_and_measure_rpcs};
use anyhow::anyhow;
use ncrypt_me::Argon2;
use std::{
   collections::HashMap,
   path::PathBuf,
   sync::{Arc, RwLock},
};
use zeus_theme::ThemeKind;
use zeus_wallet::Wallet;

use secure_types::Zeroize;
use zeus_eth::{
   alloy_primitives::{Address, Bytes, FixedBytes, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, Transaction, TransactionReceipt, TransactionRequest},
   amm::uniswap::{
      AnyUniswapPool, DexKind, FeeAmount, State, UniswapPool, UniswapV2Pool, UniswapV3Pool,
      UniswapV4Pool,
   },
   currency::{Currency, NativeCurrency, erc20::ERC20Token},
   types::{ChainId, SUPPORTED_CHAINS},
   utils::{NumericValue, address_book, client::RpcClient},
};

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const SERVER_PORT_FILE: &str = "server_port.json";
const THEME_FILE: &str = "theme.json";
const POOL_DATA_FULL: &str = "pool_data_full.json";
const POOL_DATA_FILE: &str = "pool_data.json";
const DELEGATED_WALLETS_FILE: &str = "delegated_wallets.json";

/// This is the minimum USD value in a base currency that a pool needs to have in order to be considered sufficiently liquid
pub const DEFAULT_POOL_MINIMUM_LIQUIDITY: f64 = 10_000.0;

const DELEGATE_WALLET_CHECK_TIMEOUT: u64 = 600;

/// Zeus data directory
pub fn data_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = std::env::current_dir()?.join("data");

   if !dir.exists() {
      std::fs::create_dir_all(dir.clone())?;
   }

   Ok(dir)
}

pub fn theme_kind_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(THEME_FILE);
   Ok(dir)
}

pub fn server_port_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(SERVER_PORT_FILE);
   Ok(dir)
}

pub fn delegated_wallets_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(DELEGATED_WALLETS_FILE);
   Ok(dir)
}

pub fn load_server_port() -> Result<u16, anyhow::Error> {
   let dir = server_port_dir()?;
   let port_str = std::fs::read_to_string(dir)?;
   let port = serde_json::from_str(&port_str)?;
   Ok(port)
}

pub fn load_theme_kind() -> Result<ThemeKind, anyhow::Error> {
   let dir = theme_kind_dir()?;
   let theme_kind_str = std::fs::read_to_string(dir)?;
   let theme_kind = serde_json::from_str(&theme_kind_str)?;
   Ok(theme_kind)
}

/// Pool data directory
pub fn pool_data_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(POOL_DATA_FILE);
   Ok(dir)
}

pub fn pool_data_full_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(POOL_DATA_FULL);
   Ok(dir)
}

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

   pub fn qr_image_data(&self) -> Arc<[u8]> {
      self.read(|ctx| ctx.qr_image_data.clone())
   }

   pub fn set_qr_image_data(&self, data: Vec<u8>) {
      self.erase_qr_image_data();
      self.write(|ctx| {
         ctx.qr_image_data = data.into();
      });
   }

   pub fn erase_qr_image_data(&self) {
      self.write(|ctx| {
         if let Some(data) = Arc::get_mut(&mut ctx.qr_image_data) {
            data.zeroize();
            tracing::info!("QR Image data zeroized");
         } else {
            tracing::warn!("QR Image data zeroize failed");
         }
      });
   }

   pub fn pool_manager(&self) -> PoolManagerHandle {
      self.read(|ctx| ctx.pool_manager.clone())
   }

   pub fn price_manager(&self) -> PriceManagerHandle {
      self.read(|ctx| ctx.price_manager.clone())
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

   pub fn vault_unlocked(&self) -> bool {
      self.read(|ctx| ctx.vault_unlocked)
   }

   pub fn server_running(&self) -> bool {
      self.read(|ctx| ctx.server_running)
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

   pub fn tx_confirm_window_open(&self) -> bool {
      self.read(|ctx| ctx.tx_confirm_window_open)
   }

   pub fn set_tx_confirm_window_open(&self, open: bool) {
      self.write(|ctx| {
         ctx.tx_confirm_window_open = open;
      });
   }

   pub fn sign_msg_window_open(&self) -> bool {
      self.read(|ctx| ctx.sign_msg_window_open)
   }

   pub fn set_sign_msg_window_open(&self, open: bool) {
      self.write(|ctx| {
         ctx.sign_msg_window_open = open;
      });
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

   pub fn master_wallet_address(&self) -> Address {
      self.read(|ctx| ctx.vault.master_wallet_address())
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
      let wallet = self.read(|ctx| WalletInfo::from_wallet(&ctx.current_wallet));
      wallet
   }

   pub fn wallet_exists(&self, address: Address) -> bool {
      self.read(|ctx| ctx.vault.wallet_address_exists(address))
   }

   pub fn get_wallet_info_by_address(&self, address: Address) -> Option<WalletInfo> {
      let mut info = None;
      self.read(|ctx| {
         for wallet in ctx.vault_ref().all_wallets() {
            if wallet.address() == address {
               info = Some(WalletInfo::from_wallet(&wallet));
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
            info.push(WalletInfo::from_wallet(&wallet));
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
      let address = address.to_lowercase();
      self.read(|ctx| {
         ctx.vault.contacts.iter().find(|c| c.address.to_lowercase() == address).cloned()
      })
   }

   pub fn client_available(&self, chain: u64) -> bool {
      let z_client = self.get_zeus_client();
      z_client.rpc_available(chain)
   }

   pub fn client_mev_protect_available(&self, chain: u64) -> bool {
      let z_client = self.get_zeus_client();
      z_client.mev_protect_available(chain)
   }

   pub fn client_archive_available(&self, chain: u64) -> bool {
      let z_client = self.get_zeus_client();
      z_client.rpc_archive_available(chain)
   }

   pub fn get_zeus_client(&self) -> ZeusClient {
      self.read(|ctx| ctx.client.clone())
   }

   pub async fn get_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_zeus_client();
      z_client.get_client(chain).await
   }

   pub async fn connect_to_rpc(&self, rpc: &Rpc) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_zeus_client();
      z_client.connect_to(rpc).await
   }

   /// Get an archive client for the given chain.
   ///
   /// If `http` is true, it will use an http endpoint.
   pub async fn get_archive_client(
      &self,
      chain: u64,
      http: bool,
   ) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_zeus_client();
      z_client.get_archive_client(chain, http).await
   }

   pub async fn get_mev_protect_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_zeus_client();
      z_client.get_mev_protect_client(chain).await
   }

   pub fn chain(&self) -> ChainId {
      self.read(|ctx| ctx.chain)
   }

   pub fn save_balance_manager(&self) {
      let manager = self.balance_manager();
      match manager.save() {
         Ok(_) => {
            tracing::trace!("Balance Manager saved");
         }
         Err(e) => tracing::error!("Error saving Balance Manager: {:?}", e),
      }
   }

   pub fn save_v3_positions_db(&self) {
      let db = self.read(|ctx| ctx.v3_positions_db.clone());
      match db.save() {
         Ok(_) => tracing::trace!("V3PositionsDB saved"),
         Err(e) => tracing::error!("Error saving V3 Positions DB: {:?}", e),
      }
   }

   pub fn save_currency_db(&self) {
      let db = self.read(|ctx| ctx.currency_db.clone());
      match db.save() {
         Ok(_) => tracing::trace!("CurrencyDB saved"),
         Err(e) => tracing::error!("Error saving CurrencyDB: {:?}", e),
      }
   }

   pub fn save_portfolio_db(&self) {
      let db = self.read(|ctx| ctx.portfolio_db.clone());
      match db.save() {
         Ok(_) => tracing::trace!("PortfolioDB saved"),
         Err(e) => tracing::error!("Error saving PortfolioDB: {:?}", e),
      }
   }

   pub fn save_zeus_client(&self) {
      let client = self.get_zeus_client();
      match client.save_to_file() {
         Ok(_) => tracing::trace!("ZeusClient saved"),
         Err(e) => tracing::error!("Error saving ZeusClient: {:?}", e),
      }
   }

   pub fn save_pool_manager(&self) {
      let manager = self.pool_manager();
      match manager.save_to_file() {
         Ok(_) => {}
         Err(e) => tracing::error!("Error saving Pool Manager: {:?}", e),
      }
   }

   pub fn save_price_manager(&self) {
      let manager = self.price_manager();
      match manager.save_to_file() {
         Ok(_) => {
            tracing::trace!("Price Manager saved");
         }
         Err(e) => tracing::error!("Error saving Price Manager: {:?}", e),
      }
   }

   pub fn save_tx_db(&self) {
      let db = self.read(|ctx| ctx.tx_db.clone());
      match db.save() {
         Ok(_) => tracing::trace!("TxDB saved"),
         Err(e) => tracing::error!("Error saving TxDB: {:?}", e),
      }
   }

   pub fn save_delegated_wallets(&self) {
      let wallets = self.read(|ctx| ctx.delegated_wallets.clone());
      match wallets.save_to_file() {
         Ok(_) => tracing::trace!("Smart Accounts saved"),
         Err(e) => tracing::error!("Error saving delegated wallets: {:?}", e),
      }
   }

   pub fn save_all(&self) {
      self.save_balance_manager();
      self.save_currency_db();
      self.save_portfolio_db();
      self.save_zeus_client();
      self.save_tx_db();
      self.save_v3_positions_db();
      self.save_pool_manager();
      self.save_price_manager();
      self.save_delegated_wallets();
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
         let erc_tokens = portfolio.tokens.iter().map(|token| token.clone()).collect::<Vec<_>>();
         tokens.extend(erc_tokens);
      }
      tokens
   }

   pub fn portfolio_has_token(&self, chain: u64, owner: Address, token: Address) -> bool {
      let portfolio = self.read(|ctx| ctx.portfolio_db.get(chain, owner));
      portfolio.tokens.iter().any(|t| t.address == token)
   }

   /// Calculate and update the portfolio value
   pub fn calculate_portfolio_value(&self, chain: u64, owner: Address) {
      let mut portfolio = self.get_portfolio(chain, owner);
      let mut value = 0.0;

      for token in &portfolio.tokens {
         let price = self.get_token_price(token).f64();
         let balance = self.get_token_balance(chain, owner, token.address).f64();
         value += NumericValue::value(balance, price).f64()
      }

      let eth_balance = self.get_eth_balance(chain, owner);
      let eth_price = self.get_currency_price(&Currency::wrapped_native(chain));

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
      self.read(|ctx| ctx.price_manager.get_token_price(token)).unwrap_or_default()
   }

   pub fn get_currency_price(&self, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         let wrapped_token = ERC20Token::wrapped_native_token(currency.chain_id());
         self.get_token_price(&wrapped_token)
      } else {
         let token = currency.erc20_opt().unwrap();
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

   pub fn get_token_value_for_owner(
      &self,
      chain: u64,
      owner: Address,
      token: &ERC20Token,
   ) -> NumericValue {
      let price = self.get_token_price(token);
      let balance = self.get_token_balance(chain, owner, token.address);
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
         let token = currency.erc20_opt().unwrap();
         self.get_token_balance(chain, owner, token.address)
      }
   }

   pub fn pool_has_sufficient_liquidity(&self, pool: &AnyUniswapPool) -> Option<bool> {
      if pool.state().is_none() {
         return None;
      }

      let base_balance = pool.base_balance();
      let base_price = self.get_token_price(&pool.base_currency().to_erc20());
      let base_value = NumericValue::value(base_balance.f64(), base_price.f64());

      Some(base_value.f64() >= DEFAULT_POOL_MINIMUM_LIQUIDITY)
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
         return Some("Uniswap: Permit2".to_string());
      }

      let v4_pool_manager = address_book::uniswap_v4_pool_manager(chain).unwrap();
      if v4_pool_manager == address {
         return Some("Uniswap V4: Pool Manager".to_string());
      }

      let ur_router_v2 = address_book::universal_router_v2(chain).unwrap();
      if ur_router_v2 == address {
         return Some("Uniswap: Universal Router V2".to_string());
      }

      let zeus_router = address_book::zeus_router(chain);
      if let Ok(addr) = zeus_router {
         if addr == address {
            return Some("Zeus Router".to_string());
         }
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

      #[cfg(feature = "dev")]
      if address == address_book::vitalik() {
         return Some("Vitalik".to_string());
      }

      None
   }

   pub async fn get_v2_pool(
      &self,
      chain: u64,
      address: Address,
   ) -> Result<AnyUniswapPool, anyhow::Error> {
      let z_client = self.get_zeus_client();
      let cached = self.read(|ctx| ctx.pool_manager.get_v2_pool_from_address(chain, address));

      if let Some(pool) = cached {
         return Ok(pool);
      } else {
         let pool = z_client
            .request(chain, |client| async move {
               UniswapV2Pool::from_address(client, chain, address).await
            })
            .await?;
         let pool = AnyUniswapPool::from_pool(pool);
         self.write(|ctx| ctx.pool_manager.add_pool(pool.clone()));
         self.write(|ctx| ctx.currency_db.insert_currency(chain, pool.currency0().clone()));
         self.write(|ctx| ctx.currency_db.insert_currency(chain, pool.currency1().clone()));

         let ctx = self.clone();
         RT.spawn_blocking(move || {
            ctx.save_currency_db();
            ctx.save_pool_manager();
         });

         return Ok(pool);
      };
   }

   pub async fn get_v3_pool(
      &self,
      chain: u64,
      address: Address,
   ) -> Result<AnyUniswapPool, anyhow::Error> {
      let z_client = self.get_zeus_client();
      let cached = self.read(|ctx| ctx.pool_manager.get_v3_pool_from_address(chain, address));

      if let Some(pool) = cached {
         return Ok(pool);
      } else {
         let pool = z_client
            .request(chain, |client| async move {
               UniswapV3Pool::from_address(client, chain, address).await
            })
            .await?;
         let pool = AnyUniswapPool::from_pool(pool);
         self.write(|ctx| ctx.pool_manager.add_pool(pool.clone()));
         self.write(|ctx| ctx.currency_db.insert_currency(chain, pool.currency0().clone()));
         self.write(|ctx| ctx.currency_db.insert_currency(chain, pool.currency1().clone()));

         let ctx = self.clone();
         RT.spawn_blocking(move || {
            ctx.save_currency_db();
            ctx.save_pool_manager();
         });

         return Ok(pool);
      };
   }

   pub async fn get_v4_pool(
      &self,
      chain: u64,
      fee: u32,
      expected_id: FixedBytes<32>,
   ) -> Result<AnyUniswapPool, anyhow::Error> {
      let cached = self.read(|ctx| ctx.pool_manager.get_v4_pool_from_id(chain, expected_id));

      let pool = if let Some(pool) = cached {
         return Ok(pool);
      } else {
         // Best effort pool finding from all known tokens
         let pool_fee = FeeAmount::CUSTOM(fee);

         let mut base_tokens = ERC20Token::base_tokens(chain);

         // remove WETH since in V4 is not used
         let weth = ERC20Token::wrapped_native_token(chain);
         base_tokens.retain(|t| t.address != weth.address);

         let mut base_currencies: Vec<Currency> =
            base_tokens.iter().map(|t| Currency::from(t.clone())).collect();

         // Add ETH native
         let currency = Currency::from(NativeCurrency::from(chain));
         base_currencies.push(currency);

         let quote_currencies = self.get_currencies(chain);

         let mut found_pool: Option<AnyUniswapPool> = None;
         let mut break_outer = false;

         for quote_currency in quote_currencies.iter() {
            for base_currency in &base_currencies {
               let pool = UniswapV4Pool::new(
                  chain,
                  pool_fee,
                  DexKind::UniswapV4,
                  base_currency.clone(),
                  quote_currency.clone(),
                  State::none(),
                  Address::ZERO,
               );

               if pool.id() == expected_id {
                  let pool_manager = self.pool_manager();
                  pool_manager.add_pool(pool.clone());

                  let ctx = self.clone();
                  RT.spawn_blocking(move || {
                     ctx.save_pool_manager();
                  });

                  found_pool = Some(pool.into());
                  break_outer = true;
                  break;
               }
            }

            match break_outer {
               true => break,
               false => continue,
            }
         }

         found_pool
      };

      match pool {
         Some(pool) => return Ok(pool),
         None => return Err(anyhow!("V4 Pool not found")),
      };
   }

   pub async fn get_token(
      &self,
      chain: u64,
      address: Address,
   ) -> Result<ERC20Token, anyhow::Error> {
      let cached = self.read(|ctx| ctx.currency_db.get_erc20_token(chain, address));

      if let Some(token) = cached {
         return Ok(token);
      } else {
         let z_client = self.get_zeus_client();

         let token = z_client
            .request(chain, |client| async move {
               ERC20Token::new(client, address, chain).await
            })
            .await?;
         self.write(|ctx| ctx.currency_db.insert_currency(chain, Currency::from(token.clone())));

         let ctx = self.clone();
         RT.spawn_blocking(move || {
            ctx.save_currency_db();
         });

         return Ok(token);
      };
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

   pub fn should_check_delegated_wallet_status(&self, chain: u64, account: Address) -> bool {
      self.read(|ctx| ctx.delegated_wallets.should_check(chain, account))
   }

   pub fn get_delegated_address(&self, chain: u64, account: Address) -> Option<Address> {
      self.read(|ctx| ctx.delegated_wallets.get(chain, account))
   }

   pub async fn check_delegated_wallet_status(
      &self,
      chain: u64,
      account: Address,
   ) -> Result<(), anyhow::Error> {
      let client = self.get_zeus_client();
      let code = client
         .request(chain, |client| async move {
            client.get_code_at(account).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if code.is_empty() {
         self.write(|ctx| {
            ctx.delegated_wallets.remove(chain, account);
         });
         return Ok(());
      }

      let addr_slice = &code[3..];
      let delegated_address = Address::from_slice(&addr_slice);

      self.write(|ctx| {
         ctx.delegated_wallets.add(chain, account, delegated_address);
      });

      Ok(())
   }

   pub async fn get_receipt_by_hash(
      &self,
      hash: FixedBytes<32>,
   ) -> Result<Option<TransactionReceipt>, anyhow::Error> {
      let chain = self.chain().id();

      let receipt = self.read(|ctx| ctx.receipts.get(&(chain, hash)).cloned());

      if let Some(receipt) = receipt {
         return Ok(Some(receipt));
      }

      let client = self.get_zeus_client();
      let receipt = client
         .request(chain, |client| async move {
            client.get_transaction_receipt(hash).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(receipt) = &receipt {
         self.write(|ctx| {
            ctx.receipts.insert((chain, hash), receipt.clone());
            let len = ctx.receipts.len();
            if len >= 20 {
               let oldest = ctx.receipts.iter().next().unwrap().0.clone();
               ctx.receipts.remove(&oldest);
            }
         });
      }

      Ok(receipt)
   }

   pub async fn get_tx_by_hash(
      &self,
      hash: FixedBytes<32>,
   ) -> Result<Option<Transaction>, anyhow::Error> {
      let chain = self.chain().id();

      let transaction = self.read(|ctx| ctx.transactions.get(&(chain, hash)).cloned());

      if let Some(transaction) = transaction {
         return Ok(Some(transaction));
      }

      let client = self.get_zeus_client();
      let transaction = client
         .request(chain, |client| async move {
            client.get_transaction_by_hash(hash).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(transaction) = &transaction {
         self.write(|ctx| {
            ctx.transactions.insert((chain, hash), transaction.clone());
            let len = ctx.transactions.len();
            if len >= 20 {
               let oldest = ctx.transactions.iter().next().unwrap().0.clone();
               ctx.transactions.remove(&oldest);
            }
         });
      }

      Ok(transaction)
   }

   pub async fn get_storage(
      &self,
      block_id: BlockId,
      address: Address,
      slot: U256,
   ) -> Result<U256, anyhow::Error> {
      let chain = self.chain().id();

      let block = if let Some(block) = block_id.as_u64() {
         block
      } else {
         self.get_latest_block().await?.number
      };

      let storage = self.read(|ctx| ctx.storage.get(&(chain, block, address, slot)).cloned());

      if let Some(storage) = storage {
         return Ok(storage);
      }

      let client = self.get_zeus_client();
      let storage = client
         .request(chain, |client| async move {
            client
               .get_storage_at(address, slot)
               .block_id(block_id)
               .await
               .map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      self.write(|ctx| {
         ctx.storage.insert((chain, block, address, slot), storage.clone());
         let len = ctx.storage.len();
         if len >= 20 {
            let oldest = ctx.storage.iter().next().unwrap().0.clone();
            ctx.storage.remove(&oldest);
         }
      });

      Ok(storage)
   }

   pub async fn get_code(
      &self,
      block_id: BlockId,
      address: Address,
   ) -> Result<Bytes, anyhow::Error> {
      let chain = self.chain().id();

      let block = if let Some(block) = block_id.as_u64() {
         block
      } else {
         self.get_latest_block().await?.number
      };

      let code = self.read(|ctx| ctx.codes.get(&(chain, block, address)).cloned());

      if let Some(code) = code {
         return Ok(code);
      }

      let client = self.get_zeus_client();
      let code = client
         .request(chain, |client| async move {
            client
               .get_code_at(address)
               .block_id(block_id)
               .await
               .map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      self.write(|ctx| {
         ctx.codes.insert((chain, block, address), code.clone());
         let len = ctx.codes.len();
         if len >= 20 {
            let oldest = ctx.codes.iter().next().unwrap().0.clone();
            ctx.codes.remove(&oldest);
         }
      });

      Ok(code)
   }

   pub async fn estimate_gas(&self, tx: TransactionRequest) -> Result<u64, anyhow::Error> {
      let chain = self.chain();
      let res = self.read(|ctx| ctx.estimate_gas.get(&(chain.id(), tx.clone())).cloned());
      let block_time = chain.block_time_millis();
      let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;

      if let Some(res) = res {
         // time check
         let old = res.timestamp;
         let elapsed = if now > old {
            now - old
         } else {
            tracing::warn!("System time is behind block timestamp");
            u64::MAX
         };

         if elapsed < block_time {
            return Ok(res.gas);
         }
      }

      let client = self.get_zeus_client();
      let gas = client
         .request(chain.id(), |client| {
            let tx = tx.clone();
            async move { client.estimate_gas(tx).await.map_err(|e| anyhow!("{:?}", e)) }
         })
         .await?;

      let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;

      self.write(|ctx| {
         ctx.estimate_gas.insert(
            (chain.id(), tx),
            EstimateGas {
               timestamp: now,
               gas,
            },
         );
         let len = ctx.estimate_gas.len();
         if len >= 20 {
            let oldest = ctx.estimate_gas.iter().next().unwrap().0.clone();
            ctx.estimate_gas.remove(&oldest);
         }
      });

      Ok(gas)
   }

   pub async fn get_eth_call(&self, tx: TransactionRequest) -> Result<EthCall, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
      let eth_call = self.read(|ctx| ctx.eth_calls.get(&(chain.id(), tx.clone())).cloned());

      if let Some(eth_call) = eth_call {
         // time check
         let old = eth_call.timestamp;
         let elapsed = if now > old {
            now - old
         } else {
            tracing::warn!("System time is behind block timestamp");
            u64::MAX
         };

         if elapsed < block_time {
            return Ok(eth_call);
         }
      }

      let z_client = self.get_zeus_client();
      let result = z_client
         .request(chain.id(), |client| {
            let tx = tx.clone();
            async move { client.call(tx).await.map_err(|e| anyhow!("{:?}", e)) }
         })
         .await?;

      let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;

      let eth_call = EthCall {
         timestamp: now,
         result,
      };

      self.write(|ctx| {
         ctx.eth_calls.insert((chain.id(), tx), eth_call.clone());
         let len = ctx.eth_calls.len();
         if len >= 20 {
            let oldest = ctx.eth_calls.iter().next().unwrap().0.clone();
            ctx.eth_calls.remove(&oldest);
         }
      });

      Ok(eth_call)
   }

   pub async fn get_latest_block(&self) -> Result<Block, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
      let block = self.read(|ctx| ctx.latest_block.get(&chain.id()).cloned());

      if let Some(block) = block {
         // time check
         let block_timestamp_ms = block.timestamp * 1000u64;
         let elapsed = if now > block_timestamp_ms {
            now - block_timestamp_ms
         } else {
            tracing::warn!("System time is behind block timestamp");
            u64::MAX
         };

         if elapsed < block_time {
            return Ok(block);
         }
      }

      let z_client = self.get_zeus_client();
      let block = z_client
         .request(chain.id(), |client| async move {
            client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(block) = block {
         let block = Block::new(block.header.number, block.header.timestamp);
         self.write(|ctx| {
            ctx.latest_block.insert(chain.id(), block.clone());
         });
         return Ok(block);
      }

      Err(anyhow!("No block found"))
   }

   pub async fn test_and_measure_rpcs(&self) {
      test_and_measure_rpcs(self.clone()).await
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

/// Holds addresses that are delegated to a smart contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegatedWallets {
   #[serde(with = "serde_hashmap")]
   /// Map of (chain, account) to delegated address
   pub map: HashMap<(u64, Address), Address>,
   /// Last time we checked the smart account status
   /// Time is in UNIX timestamp
   pub last_check: HashMap<(u64, Address), u64>,
}

impl DelegatedWallets {
   pub fn new() -> Self {
      Self {
         map: HashMap::new(),
         last_check: HashMap::new(),
      }
   }

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = delegated_wallets_dir()?;
      let data = std::fs::read(dir)?;
      let smart_accounts = serde_json::from_slice(&data)?;
      Ok(smart_accounts)
   }

   pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
      let data = serde_json::to_string(self)?;
      let dir = delegated_wallets_dir()?;
      std::fs::write(dir, data)?;
      Ok(())
   }

   pub fn add(&mut self, chain: u64, account: Address, delegated_address: Address) {
      self.map.insert((chain, account), delegated_address);
   }

   pub fn remove(&mut self, chain: u64, account: Address) {
      self.map.remove(&(chain, account));
   }

   pub fn should_check(&self, chain: u64, account: Address) -> bool {
      let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
      let last_check = self.last_check.get(&(chain, account)).cloned();
      if last_check.is_none() {
         return true;
      }

      let last_check = last_check.unwrap();
      let time_passed = now.saturating_sub(last_check);
      time_passed > DELEGATE_WALLET_CHECK_TIMEOUT
   }

   pub fn get(&self, chain: u64, account: Address) -> Option<Address> {
      self.map.get(&(chain, account)).cloned()
   }
}

#[derive(Clone)]
pub struct Block {
   pub number: u64,
   pub timestamp: u64,
}

impl Block {
   pub fn new(number: u64, timestamp: u64) -> Self {
      Self { number, timestamp }
   }
}

#[derive(Clone)]
pub struct EthCall {
   pub timestamp: u64,
   pub result: Bytes,
}

#[derive(Clone)]
pub struct EstimateGas {
   pub timestamp: u64,
   pub gas: u64,
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

/// Saved contact by the user
#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Contact {
   pub name: String,
   pub address: String,
}

impl Contact {
   pub fn new(name: String, address: String) -> Self {
      Self { name, address }
   }

   pub fn address_short(&self, start: usize, end: usize) -> String {
      let address_str = self.address.as_str();

      if address_str.len() < start + end {
         return address_str.to_string();
      }

      let start_part = &address_str[..start];
      let end_part = &address_str[address_str.len() - end..];

      format!("{}...{}", start_part, end_part)
   }
}

pub struct ZeusContext {
   pub client: ZeusClient,

   /// The current selected chain from the GUI
   pub chain: ChainId,

   /// The current selected wallet from the GUI
   pub current_wallet: Wallet,

   /// Loaded Vault
   vault: Vault,
   pub save_vault_in_progress: bool,
   pub vault_exists: bool,
   pub vault_unlocked: bool,
   pub currency_db: CurrencyDB,
   pub portfolio_db: PortfolioDB,
   pub tx_db: TransactionsDB,
   pub v3_positions_db: V3PositionsDB,
   pub pool_manager: PoolManagerHandle,
   pub price_manager: PriceManagerHandle,
   pub balance_manager: BalanceManagerHandle,
   pub data_syncing: bool,
   pub dex_syncing: bool,
   pub on_startup_syncing: bool,
   pub base_fee: HashMap<u64, BaseFee>,
   pub latest_block: HashMap<u64, Block>,
   pub eth_calls: HashMap<(u64, TransactionRequest), EthCall>,
   pub estimate_gas: HashMap<(u64, TransactionRequest), EstimateGas>,
   pub codes: HashMap<(u64, u64, Address), Bytes>,
   pub storage: HashMap<(u64, u64, Address, U256), U256>,
   pub transactions: HashMap<(u64, FixedBytes<32>), Transaction>,
   pub receipts: HashMap<(u64, FixedBytes<32>), TransactionReceipt>,
   pub priority_fee: PriorityFee,
   pub connected_dapps: ConnectedDapps,
   pub delegated_wallets: DelegatedWallets,
   pub server_port: u16,
   pub server_running: bool,
   pub tx_confirm_window_open: bool,
   pub sign_msg_window_open: bool,
   /// Private Key Qr Code
   pub qr_image_data: Arc<[u8]>,
}

impl ZeusContext {
   pub fn new() -> Self {
      let client = match ZeusClient::load_from_file() {
         Ok(client) => client,
         Err(e) => {
            tracing::error!("Error loading client: {:?}", e);
            ZeusClient::default()
         }
      };

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

      let pool_manager = match PoolManagerHandle::load_from_file() {
         Ok(manager) => manager,
         Err(e) => {
            tracing::error!(
               "Failed to load pool manager, falling back to default: {:?}",
               e
            );
            PoolManagerHandle::default()
         }
      };

      let price_manager = match PriceManagerHandle::load_from_file() {
         Ok(manager) => manager,
         Err(e) => {
            tracing::error!(
               "Failed to load price manager, falling back to default: {:?}",
               e
            );
            PriceManagerHandle::new()
         }
      };

      let delegated_wallets = match DelegatedWallets::load_from_file() {
         Ok(accounts) => accounts,
         Err(e) => {
            tracing::error!("Failed to load delegated wallets: {:?}", e);
            DelegatedWallets::new()
         }
      };

      let priority_fee = PriorityFee::default();
      Self {
         client,
         chain: ChainId::new(1).unwrap(),
         current_wallet: Wallet::new_rng("I should not be here".to_string()),
         vault: Vault::default(),
         save_vault_in_progress: false,
         vault_exists,
         vault_unlocked: false,
         currency_db,
         portfolio_db,
         tx_db,
         v3_positions_db,
         pool_manager,
         price_manager,
         balance_manager,
         data_syncing: false,
         dex_syncing: false,
         on_startup_syncing: false,
         base_fee: HashMap::with_capacity(SUPPORTED_CHAINS.len()),
         latest_block: HashMap::with_capacity(SUPPORTED_CHAINS.len()),
         eth_calls: HashMap::with_capacity(20),
         estimate_gas: HashMap::with_capacity(20),
         codes: HashMap::with_capacity(20),
         storage: HashMap::with_capacity(20),
         transactions: HashMap::with_capacity(20),
         receipts: HashMap::with_capacity(20),
         priority_fee,
         connected_dapps: ConnectedDapps::default(),
         delegated_wallets,
         server_port: SERVER_PORT,
         server_running: false,
         tx_confirm_window_open: false,
         sign_msg_window_open: false,
         qr_image_data: Arc::new([0u8; 0]),
      }
   }

   pub fn vault_ref(&self) -> &Vault {
      &self.vault
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::str::FromStr;
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
   async fn test_extract_delegated_address() {
      let ctx = ZeusCtx::new();

      let chain = 1;
      let account = Address::from_str("0x67d3FA6a5CF45D85F697A497b3270A06415E5BfE").unwrap();
      let delegated_address =
         Address::from_str("0x63c0c19a282a1B52b07dD5a65b58948A07DAE32B").unwrap();

      let client = ctx.get_client(chain).await.unwrap();
      let code = client.get_code_at(account).await.unwrap();
      eprintln!("Code {}", code);
      eprintln!("Code length: {}", code.len());

      let addr_slice = &code[3..];
      let address = Address::from_slice(&addr_slice);
      eprintln!("Address: {}", address);

      assert_eq!(address, delegated_address);
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
