use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::core::{
   PoolManagerHandle, ZeusCtx,
   context::{DEFAULT_POOL_MINIMUM_LIQUIDITY, data_dir},
   serde_hashmap,
};
use crate::utils::{RT, write_private};
use zeus_eth::{
   alloy_primitives::{Address, B256},
   amm::uniswap::{AnyUniswapPool, UniswapPool},
   currency::{Currency, ERC20Token},
   utils::{NumericValue, price_feed::get_base_token_price},
};

use tokio::task::JoinHandle;

const PRICE_DATA_FILE: &str = "price_data.json";

/// Time in seconds to wait before updating the base token prices again
pub const TOKEN_PRICE_UPDATE_INTERVAL: u64 = 600;

#[derive(Clone, Serialize, Deserialize)]
pub struct PriceManagerHandle(Arc<RwLock<PriceManager>>);

impl PriceManagerHandle {
   pub fn new() -> Self {
      Self(Arc::new(RwLock::new(PriceManager::new())))
   }

   pub fn read<R>(&self, reader: impl FnOnce(&PriceManager) -> R) -> R {
      reader(&self.0.read().expect("PriceManagerHandle is poisoned"))
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut PriceManager) -> R) -> R {
      writer(&mut self.0.write().expect("PriceManagerHandle is poisoned"))
   }

   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let dir = data_dir()?.join(PRICE_DATA_FILE);
      let data = std::fs::read(dir)?;
      let manager = serde_json::from_slice(&data)?;
      Ok(Self(Arc::new(RwLock::new(manager))))
   }

   pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
      let data = serde_json::to_string(&self.read(|manager| manager.clone()))?;
      let dir = data_dir()?.join(PRICE_DATA_FILE);
      write_private(&dir, data.as_bytes())?;
      Ok(())
   }

   pub fn get_token_price(&self, token: &ERC20Token) -> Option<NumericValue> {
      self.read(|manager| manager.token_prices.get(&(token.chain_id, token.address)).cloned())
   }

   fn get_good_pool_for(
      &self,
      chain: u64,
      pool_manager: PoolManagerHandle,
      token: Address,
   ) -> Option<AnyUniswapPool> {
      let pool_id = self.read(|manager| manager.good_pools.get(&(chain, token)).cloned());

      let id = match pool_id {
         Some(pool_id) => pool_id,
         None => return None,
      };

      match id {
         PoolId::ID(id) => pool_manager.get_v4_pool_from_id(chain, id),
         PoolId::Address(address) => pool_manager.get_pool_from_address(chain, address),
      }
   }

   fn should_update_base_token_prices(&self, chain: u64) -> bool {
      let now = Instant::now();
      let last_updated =
         self.read(|manager| manager.base_token_prices_last_updated.get(&chain).cloned());
      if last_updated.is_none() {
         return true;
      }

      let last_updated = last_updated.unwrap();
      let timeout = Duration::from_secs(TOKEN_PRICE_UPDATE_INTERVAL);
      let time_passed = now.duration_since(last_updated);
      let expired = time_passed > timeout;

      let mut missing_prices = false;

      let base_tokens = ERC20Token::base_tokens(chain);
      for token in &base_tokens {
         if !self.read(|manager| manager.token_prices.contains_key(&(chain, token.address))) {
            missing_prices = true;
            break;
         }
      }

      expired || missing_prices
   }

   pub async fn calculate_prices(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      pool_manager: PoolManagerHandle,
      mut tokens: Vec<ERC20Token>,
   ) -> Result<(), anyhow::Error> {
      if self.should_update_base_token_prices(chain) {
         self.update_base_token_prices(ctx.clone(), chain).await?;
      }

      if tokens.len() == 1 {
         if tokens[0].is_base() {
            return Ok(());
         }
      }

      // Remove base tokens from the list
      tokens.retain(|t| !t.is_base());

      // Find any tokens that do not have a good pool
      let mut tokens_without_pool = Vec::new();

      for token in &tokens {
         if token.is_base() {
            continue;
         }

         let pool_id =
            self.read(|manager| manager.good_pools.get(&(chain, token.address)).cloned());

         if pool_id.is_none() {
            tokens_without_pool.push(token.clone());
         }
      }

      #[cfg(feature = "dev")]
      if tokens_without_pool.len() > 0 {
         tracing::info!(
            "Finding good pools try 1, Tokens without pools {}",
            tokens_without_pool.len()
         );
      }

      self
         .find_good_pool(
            ctx.clone(),
            chain,
            pool_manager.clone(),
            tokens_without_pool,
         )
         .await?;

      // Update the state for any pools if needed
      let mut pools_to_update = Vec::new();

      for token in &tokens {
         let pool_opt = self.get_good_pool_for(chain, pool_manager.clone(), token.address);

         let pool = match pool_opt {
            Some(pool) => pool,
            None => continue,
         };

         if pool.state().is_none() {
            pools_to_update.push(pool);
         }
      }

      if !pools_to_update.is_empty() {
         let _p = pool_manager
            ._update_state_for_pools(ctx.clone(), chain, pools_to_update)
            .await?;
      }

      // Search again for good pools if the current good pool liquidity has dropped below the minimum
      let mut tokens_need_new_pool = Vec::new();

      for token in &tokens {
         let pool_opt = self.get_good_pool_for(chain, pool_manager.clone(), token.address);

         let pool = match pool_opt {
            Some(pool) => pool,
            None => continue,
         };

         let base_balance = pool.base_balance();
         let base_price =
            self.get_token_price(&pool.base_currency().to_erc20()).unwrap_or_default();
         let base_value = NumericValue::value(base_balance.f64(), base_price.f64());

         if base_value.f64() < DEFAULT_POOL_MINIMUM_LIQUIDITY {
            tokens_need_new_pool.push(token.clone());
         }
      }

      #[cfg(feature = "dev")]
      if tokens_need_new_pool.len() > 0 {
         tracing::info!(
            "Finding good pools try 2, Tokens need new pools {}",
            tokens_need_new_pool.len()
         );
      }

      self
         .find_good_pool(
            ctx.clone(),
            chain,
            pool_manager.clone(),
            tokens_need_new_pool,
         )
         .await?;

      // If there is still no good pool for a token, try to discover new pools
      let mut tokens_without_pool = Vec::new();
      for token in &tokens {
         let pool_opt = self.get_good_pool_for(chain, pool_manager.clone(), token.address);

         if pool_opt.is_none() {
            tokens_without_pool.push(token.clone());
         }
      }

      #[cfg(feature = "dev")]
      if tokens_without_pool.len() > 0 {
         tracing::info!(
            "Finding good pools try 3, Tokens without pools {}",
            tokens_without_pool.len()
         );
      }

      match pool_manager
         .discover_pools_for_tokens(ctx.clone(), chain, tokens_without_pool.clone())
         .await
      {
         Ok(_) => {}
         Err(e) => tracing::error!("Error syncing pools: {:?}", e),
      }

      self
         .find_good_pool(
            ctx.clone(),
            chain,
            pool_manager.clone(),
            tokens_without_pool,
         )
         .await?;

      // Calculate the prices
      for token in &tokens {
         let pool_opt = self.get_good_pool_for(chain, pool_manager.clone(), token.address);

         let pool = match pool_opt {
            Some(pool) => pool,
            None => continue,
         };

         let base_token = pool.base_currency().to_erc20();
         let base_price = self.get_token_price(&base_token).unwrap_or_default();
         let quote_price = pool.quote_price(base_price.f64()).unwrap_or_default();

         if quote_price == 0.0 {
            continue;
         }

         let key = (chain, token.address);
         let price = NumericValue::currency_price(quote_price);

         self.write(|manager| {
            manager.token_prices.insert(key, price);
         });
      }

      self.write(|manager| {
         manager.good_pools.shrink_to_fit();
         manager.token_prices.shrink_to_fit();
      });

      Ok(())
   }

   /// Find a good pool for the given tokens
   ///
   /// We choose the pool with the highest $value in base currency liquidity
   ///
   /// For example if we want to find the best pool for UNI and we have only 2 pools lets say WETH/UNI and DAI/UNI
   ///
   /// We will choose the pool with the highest liquidity in WETH or DAI in terms of USD value
   ///
   /// Pool state is collected across all tokens and updated in one batched RPC call.
   async fn find_good_pool(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      pool_manager: PoolManagerHandle,
      tokens: Vec<ERC20Token>,
   ) -> Result<(), anyhow::Error> {
      if tokens.is_empty() {
         return Ok(());
      }

      // Gather candidate pools per token and dedupe pools that need state
      let mut pools_by_token: Vec<(ERC20Token, Vec<AnyUniswapPool>)> =
         Vec::with_capacity(tokens.len());
      let mut pools_to_update = Vec::new();
      let mut seen_update = HashSet::new();

      for token in tokens {
         let currency = Currency::from(token.clone());
         let mut pools = pool_manager.get_pools_that_have_currency(&currency);

         // Avoid irrelevant pools
         pools.retain(|p| p.currency0().is_base() || p.currency1().is_base());

         for pool in &pools {
            if pool.state().is_none() {
               let key = (pool.address(), pool.id());
               if seen_update.insert(key) {
                  pools_to_update.push(pool.clone());
               }
            }
         }

         pools_by_token.push((token, pools));
      }

      // Single batched state fetch for every missing pool
      if !pools_to_update.is_empty() {
         #[cfg(feature = "dev")]
         tracing::info!(
            "find_good_pool: updating state for {} pools across {} tokens",
            pools_to_update.len(),
            pools_by_token.len()
         );

         let _ = pool_manager
            ._update_state_for_pools(ctx.clone(), chain, pools_to_update)
            .await?;

         // Refresh local candidates from the manager (state was written by add_pools)
         for (_, pools) in pools_by_token.iter_mut() {
            for pool in pools.iter_mut() {
               let fresh = if pool.dex_kind().is_v4() {
                  pool_manager.get_v4_pool_from_id(chain, pool.id())
               } else {
                  pool_manager.get_pool_from_address(chain, pool.address())
               };

               if let Some(fresh) = fresh {
                  *pool = fresh;
               }
            }
         }
      }

      // 3) Pick the best pool per token from refreshed state
      for (token, pools) in pools_by_token {
         let mut good_pool = None;
         let mut highest_value = 0.0;

         for pool in &pools {
            let base_token = pool.base_currency().to_erc20();

            // For stables hardcode the price to 1 USD to avoid any mispricing incase of price fluctuations
            // The token price afterwards is calculated based on the actual usd price of the stablecoin
            let base_price = if base_token.is_stablecoin() {
               NumericValue::currency_price(1.0)
            } else {
               self.get_token_price(&base_token).unwrap_or_default()
            };

            let value = NumericValue::value(pool.base_balance().f64(), base_price.f64());

            if value.f64() < DEFAULT_POOL_MINIMUM_LIQUIDITY {
               continue;
            }

            if highest_value < value.f64() {
               highest_value = value.f64();
               good_pool = Some(pool.clone());
            }
         }

         if let Some(pool) = good_pool {
            self.write(|manager| {
               let id = PoolId::new(pool);
               manager.good_pools.insert((chain, token.address), id);
            });
         }
      }

      Ok(())
   }

   pub async fn update_base_token_prices(
      &self,
      ctx: ZeusCtx,
      chain: u64,
   ) -> Result<(), anyhow::Error> {
      let client = ctx.get_zeus_client();
      let new_prices = Arc::new(Mutex::new(HashMap::new()));
      let tokens = ERC20Token::base_tokens(chain);

      let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

      for token in tokens {
         let client = client.clone();
         let new_prices = new_prices.clone();

         let task = RT.spawn(async move {
            let price = client
               .request(chain, |client| async move {
                  get_base_token_price(client, chain, token.address, None).await
               })
               .await?;
            let mut new_prices = new_prices.lock().unwrap();
            new_prices.insert((chain, token.address), price);
            Ok(())
         });
         tasks.push(task);
      }

      for task in tasks {
         let _ = task.await;
      }

      let new_prices = new_prices.lock().unwrap().clone();

      self.write(|manager| {
         for (key, price) in new_prices {
            let p = NumericValue::currency_price(price);
            manager.token_prices.insert(key, p);
         }

         manager.base_token_prices_last_updated.insert(chain, Instant::now());
         manager.base_token_prices_last_updated.shrink_to_fit();
      });

      Ok(())
   }
}

#[derive(Clone, Serialize, Deserialize)]
enum PoolId {
   ID(B256),
   Address(Address),
}

impl PoolId {
   pub fn new(pool: impl UniswapPool) -> Self {
      if pool.dex_kind().is_v4() {
         Self::ID(pool.id())
      } else {
         Self::Address(pool.address())
      }
   }
}

/// Token Prices
///
/// Key: (chain_id, token) -> Value: Price
type TokenPrices = HashMap<(u64, Address), NumericValue>;

/// Pools with sufficient liquidity to calculate prices
///
/// Key: (chain_id, token) -> Value: PoolID
type GoodPools = HashMap<(u64, Address), PoolId>;

/// Last time we updated the base token prices
///
/// Key: chain_id -> Value: Instant
type BaseTokenPriceLastUpdated = HashMap<u64, Instant>;

#[derive(Clone, Serialize, Deserialize)]
pub struct PriceManager {
   #[serde(with = "serde_hashmap")]
   good_pools: GoodPools,

   #[serde(with = "serde_hashmap")]
   pub token_prices: TokenPrices,

   #[serde(skip)]
   base_token_prices_last_updated: BaseTokenPriceLastUpdated,
}

impl PriceManager {
   pub fn new() -> Self {
      Self {
         good_pools: HashMap::new(),
         token_prices: HashMap::new(),
         base_token_prices_last_updated: HashMap::new(),
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[tokio::test]
   async fn test_it_works() {
      let ctx = ZeusCtx::new();
      let chain = 1;

      let price_manager = PriceManagerHandle::new();
      let pool_manager = ctx.pool_manager();

      let link_token = ERC20Token::link();

      price_manager.update_base_token_prices(ctx.clone(), chain).await.unwrap();
      price_manager
         .calculate_prices(
            ctx.clone(),
            chain,
            pool_manager,
            vec![link_token.clone()],
         )
         .await
         .unwrap();

      let price = price_manager.get_token_price(&link_token).unwrap();
      eprintln!("LINK Price: ${}", price.formatted());
   }
}
