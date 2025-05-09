use alloy_contract::private::{Network, Provider};
use alloy_primitives::Address;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::{
   DexKind,
   uniswap::{AnyUniswapPool, FEE_TIERS, UniswapPool, UniswapV2Pool, UniswapV3Pool, state::*, v2::pool::FEE as V2_FEE},
};
use currency::{Currency, ERC20Token};
use serde::{Deserialize, Serialize};
use tracing::info;
use utils::{NumericValue, batch, price_feed::get_base_token_price};

/// Thread-safe handle to the [PoolManager]
#[derive(Clone)]
pub struct PoolManagerHandle(Arc<RwLock<PoolManager>>);

impl Default for PoolManagerHandle {
   fn default() -> Self {
      Self(Arc::new(RwLock::new(PoolManager::default())))
   }
}

impl PoolManagerHandle {
   pub fn new(pool_manager: PoolManager) -> Self {
      Self(Arc::new(RwLock::new(pool_manager)))
   }

   /// Shared access to the pool manager
   pub fn read<R>(&self, reader: impl FnOnce(&PoolManager) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   /// Exclusive mutable access to the pool manager
   pub fn write<R>(&self, writer: impl FnOnce(&mut PoolManager) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   /// Deserialize the [PoolManager] from a JSON string
   pub fn from_string(json: &str) -> Result<Self, serde_json::Error> {
      let manager = serde_json::from_str(json)?;
      Ok(Self(Arc::new(RwLock::new(manager))))
   }

   /// Serialize the [PoolManager] to a JSON string
   pub fn to_string(&self) -> Result<String, serde_json::Error> {
      self.read(|manager| serde_json::to_string(manager))
   }

   /// Deserialize the [PoolManager] from a JSON file
   pub fn from_dir(dir: &std::path::PathBuf) -> Result<Self, anyhow::Error> {
      let data = std::fs::read(dir)?;
      let manager = serde_json::from_slice(&data)?;
      Ok(Self(Arc::new(RwLock::new(manager))))
   }

   /// Serialize the [PoolManager] to a JSON file
   pub fn save_to_dir(&self, dir: &std::path::PathBuf) -> Result<(), anyhow::Error> {
      let data = self.read(|manager| serde_json::to_string(manager))?;
      std::fs::write(dir, data)?;
      Ok(())
   }

   pub fn reset_token_prices(&self) {
      self.write(|manager| manager.token_prices.clear());
   }

   /// Get all pools that include the given currency
   pub fn get_pools_from_currency(&self, currency: &Currency) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_pools_from_currency(currency))
   }

   pub fn get_pools_from_pair(&self, currency_a: &Currency, currency_b: &Currency) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_pools_from_pair(currency_a.chain_id(), currency_a, currency_b))
   }

   pub fn get_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self.read(|manager| manager.get_pools_for_chain(chain_id))
   }

   pub fn get_pool(
      &self,
      chain_id: u64,
      dex: DexKind,
      fee: u32,
      currency0: &Currency,
      currency1: &Currency,
   ) -> Option<AnyUniswapPool> {
      self.read(|manager| {
         manager
            .get_pool(chain_id, dex, fee, currency0, currency1)
            .cloned()
      })
   }

   pub fn get_pool_from_address(&self, chain_id: u64, address: Address) -> Option<AnyUniswapPool> {
      self.read(|manager| manager.get_pool_from_address(chain_id, address).cloned())
   }

   pub fn get_v2_pool_from_address(&self, chain_id: u64, address: Address) -> Option<AnyUniswapPool> {
      self.read(|manager| manager.get_v2_pool_from_address(chain_id, address).cloned())
   }

   pub fn get_v3_pool_from_address(&self, chain_id: u64, address: Address) -> Option<AnyUniswapPool> {
      self.read(|manager| manager.get_v3_pool_from_address(chain_id, address).cloned())
   }

   pub fn add_pool(&self, pool: impl UniswapPool) {
      self.write(|manager| manager.add_pool(pool));
   }

   pub fn remove_pool(&self, chain_id: u64, dex: DexKind, fee: u32, currency0: Currency, currency1: Currency) {
      self.write(|manager| manager.remove_pool(chain_id, dex, fee, currency0, currency1));
   }

   pub fn get_token_price(&self, token: &ERC20Token) -> Option<NumericValue> {
      self.read(|manager| manager.get_token_price(token))
   }

   /// Update the state of the manager for the given chain
   pub async fn update<P, N>(&self, client: P, chain: u64) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      self.update_pool_state(client.clone(), chain).await?;
      self.update_base_token_prices(client.clone(), chain).await?;
      // self.cleanup_pools();
      self.calculate_prices();
      Ok(())
   }

   async fn update_pool_state<P, N>(&self, client: P, chain_id: u64) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let pools = self.read(|manager| manager.pools.clone().into_values().collect::<Vec<_>>());
      let concurrency = self.read(|manager| manager.concurrency);
      let pools = batch_update_state(client, chain_id, concurrency, pools).await?;
      self.write(|manager| {
         for pool in pools {
            manager.add_pool(pool);
         }
      });
      Ok(())
   }

   /// Update the state for the given pools
   pub async fn update_state_for_pools<P, N>(
      &self,
      client: P,
      chain: u64,
      pools: Vec<impl UniswapPool>,
   ) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let pools = pools
         .into_iter()
         .map(|p| AnyUniswapPool::from_pool(p))
         .collect::<Vec<_>>();
      let concurrency = self.read(|manager| manager.concurrency);
      let pools = batch_update_state(client, chain, concurrency, pools).await?;
      self.write(|manager| {
         for pool in pools {
            manager.add_pool(pool);
         }
      });
      self.calculate_prices();
      Ok(())
   }

   /// Update the base token prices for the given tokens
   pub async fn update_base_token_prices<P, N>(&self, client: P, chain: u64) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let prices = PoolManager::fetch_base_token_prices(client, chain).await?;
      self.write(|manager| manager.set_token_prices(prices));
      Ok(())
   }

   /// Cleanup pools that do not have sufficient liquidity
   pub fn cleanup_pools(&self) {
      self.write(|manager| manager.cleanup_pools())
   }

   pub fn calculate_prices(&self) {
      self.write(|manager| manager.calculate_prices())
   }

   /// Sync V2 & V3 pools for the given tokens based on:
   ///
   /// - The token's chain id
   /// - All the possible [DexKind] for the chain
   /// - Base Tokens [ERC20Token::base_tokens()]
   pub async fn sync_pools_for_tokens<P, N>(
      &self,
      client: P,
      tokens: Vec<ERC20Token>,
      dex_kinds: Vec<DexKind>,
   ) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      for token in tokens {
         self
            .sync_v2_pools_for_token(client.clone(), token.clone(), dex_kinds.clone())
            .await?;

         self
            .sync_v3_pools_for_token(client.clone(), token.clone(), dex_kinds.clone())
            .await?;
      }

      Ok(())
   }

   // TODO: Do batch requests
   /// Sync all the possible v2 pools for the given token based on:
   ///
   /// - The token's chain id
   /// - All the possible [DexKind] for the chain
   /// - Base Tokens [ERC20Token::base_tokens()]
   pub async fn sync_v2_pools_for_token<P, N>(
      &self,
      client: P,
      token: ERC20Token,
      dex_kinds: Vec<DexKind>,
   ) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let chain = token.chain_id;
      let base_tokens = ERC20Token::base_tokens(chain);
      let mut pools = Vec::new();

      for base_token in base_tokens {
         if base_token.address == token.address {
            continue;
         }

         let currency_a = base_token.clone().into();
         let currency_b = token.clone().into();

         for dex in &dex_kinds {
            if dex.is_v3() {
               continue;
            }

            // if pool already exist, skip
            let cached_pool = self.get_pool(chain, *dex, V2_FEE, &currency_a, &currency_b);
            if cached_pool.is_some() {
               continue;
            }

            info!(
               target: "zeus_eth::amm::pool_manager", "Pool Not in cache: {} {}-{} for Chain Id: {}",
               dex.to_str(),
               token.symbol,
               base_token.symbol,
               token.chain_id
            );

            let pool_res = UniswapV2Pool::from(
               client.clone(),
               token.chain_id,
               token.clone(),
               base_token.clone(),
               *dex,
            )
            .await;

            if let Ok(pool) = pool_res {
               info!(
                  target: "zeus_eth::amm::pool_manager", "Got {} pool {} for {}-{} for Chain Id: {}",
                  dex.to_str(),
                  pool.address(),
                  pool.token0().symbol,
                  pool.token1().symbol,
                  token.chain_id
               );
               pools.push(pool);
            }
         }
      }

      for pool in pools {
         self.add_pool(pool.clone());
      }

      Ok(())
   }

   /// Sync all the possible v3 pools for the given token based on:
   ///
   /// - The token's chain id
   /// - All the possible [DexKind] for the chain
   /// - Base Tokens [ERC20Token::base_tokens()]
   pub async fn sync_v3_pools_for_token<P, N>(
      &self,
      client: P,
      token: ERC20Token,
      dex_kinds: Vec<DexKind>,
   ) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let chain = token.chain_id;
      let base_tokens = ERC20Token::base_tokens(chain);

      for base_token in &base_tokens {
         if base_token.address == token.address {
            continue;
         }

         for dex in &dex_kinds {
            if dex.is_v2() {
               continue;
            }

            let currency_a = base_token.clone().into();
            let currency_b = token.clone().into();

            // check if pool already exists
            // TODO: Add a timeout or something because some fee tiers may not exist at all
            let mut pools_exists = [false; FEE_TIERS.len()];
            for (i, fee) in FEE_TIERS.iter().enumerate() {
               let pool = self.get_pool(chain, *dex, *fee, &currency_a, &currency_b);
               if pool.is_some() {
                  pools_exists[i] = true;
               }
            }

            // if all true then skip
            if pools_exists.iter().all(|b| *b == true) {
               continue;
            }

            let factory = dex.factory(token.chain_id)?;

            info!(
               target: "zeus_eth::amm::pool_manager", "Getting {} pools for: {}-{} Chain Id: {}",
               dex.to_str(),
               token.symbol,
               base_token.symbol,
               token.chain_id
            );

            let pools = batch::get_v3_pools(client.clone(), token.address, base_token.address, factory).await?;

            for pool in &pools {
               if !pool.addr.is_zero() {
                  let fee: u32 = pool.fee.to_string().parse()?;
                  let v3_pool = UniswapV3Pool::new(
                     token.chain_id,
                     pool.addr,
                     fee,
                     token.clone(),
                     base_token.clone(),
                     *dex,
                  );

                  info!(
                     target: "zeus_eth::amm::pool_manager", "Got {} pool {} for {}/{} - Fee: {}",
                     dex.to_str(),
                     v3_pool.address,
                     v3_pool.token0().symbol,
                     v3_pool.token1().symbol,
                     v3_pool.fee.fee()
                  );

                  self.add_pool(v3_pool);
               }
            }
         }
      }

      Ok(())
   }
}

/// Key: (chain_id, dex_kind, fee, tokenA, tokenB) -> Value: Pool
pub type Pools = HashMap<(u64, DexKind, u32, Currency, Currency), AnyUniswapPool>;

/// Token Prices
///
/// Key: (chain_id, token) -> Value: Price
pub type TokenPrices = HashMap<(u64, Address), NumericValue>;

#[derive(Clone, Serialize, Deserialize)]
pub struct PoolManager {
   #[serde(with = "serde_hashmap")]
   pub pools: Pools,

   #[serde(with = "serde_hashmap")]
   pub token_prices: TokenPrices,

   /// Set to 1 for no concurrency
   pub concurrency: u8,
}

impl Default for PoolManager {
   fn default() -> Self {
      Self {
         pools: HashMap::new(),
         token_prices: HashMap::new(),
         concurrency: 1,
      }
   }
}

impl PoolManager {
   pub fn new(pools: Pools, token_prices: TokenPrices, concurrency: u8) -> Self {
      Self {
         pools,
         token_prices,
         concurrency,
      }
   }

   pub fn get_token_price(&self, token: &ERC20Token) -> Option<NumericValue> {
      self
         .token_prices
         .get(&(token.chain_id, token.address))
         .cloned()
   }

   pub fn set_token_prices(&mut self, prices: TokenPrices) {
      for (key, price) in prices {
         self.token_prices.insert(key, price);
      }
   }

   // ! skip that part for now
   pub fn cleanup_pools(&mut self) {
      // self.pools.retain(|_, pool| pool.enough_liquidity());
   }

   pub fn add_pool(&mut self, pool: impl UniswapPool) {
      let any_pool = AnyUniswapPool::from_pool(pool);
      let key = (
         any_pool.chain_id(),
         any_pool.dex_kind(),
         any_pool.fee().fee(),
         any_pool.currency0().clone(),
         any_pool.currency1().clone(),
      );

      self.pools.insert(key, any_pool);
   }

   pub fn remove_pool(&mut self, chain_id: u64, dex: DexKind, fee: u32, currency0: Currency, currency1: Currency) {
      self
         .pools
         .remove(&(chain_id, dex, fee, currency0, currency1));
   }

   /// Get any pools that includes the given currency
   pub fn get_pools_from_currency(&self, currency: &Currency) -> Vec<AnyUniswapPool> {
      let mut pools = Vec::new();
      for (_, pool) in &self.pools {
         if pool.chain_id() != currency.chain_id() {
            continue;
         }
         if pool.is_currency0(currency) || pool.is_currency1(currency) {
            pools.push(pool.clone());
         }
      }
      pools
   }

   /// Get all pools for this currency pair
   pub fn get_pools_from_pair(
      &self,
      chain_id: u64,
      currency_a: &Currency,
      currency_b: &Currency,
   ) -> Vec<AnyUniswapPool> {
      let mut pools = Vec::new();
      for (_, pool) in &self.pools {
         if pool.chain_id() != chain_id {
            continue;
         }
         if pool.is_currency0(currency_a) && pool.is_currency1(currency_b) {
            pools.push(pool.clone());
         } else if pool.is_currency0(currency_b) && pool.is_currency1(currency_a) {
            pools.push(pool.clone());
         }
      }
      pools
   }

   /// Get all pools for the given chain
   pub fn get_pools_for_chain(&self, chain_id: u64) -> Vec<AnyUniswapPool> {
      self
         .pools
         .values()
         .filter(|p| p.chain_id() == chain_id)
         .cloned()
         .collect()
   }

   pub fn get_pool(
      &self,
      chain_id: u64,
      dex: DexKind,
      fee: u32,
      currency_a: &Currency,
      currency_b: &Currency,
   ) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self
         .pools
         .get(&(chain_id, dex, fee, currency_a.clone(), currency_b.clone()))
      {
         return Some(pool);
      } else if let Some(pool) = self
         .pools
         .get(&(chain_id, dex, fee, currency_b.clone(), currency_a.clone()))
      {
         return Some(pool);
      } else {
         return None;
      }
   }

   pub fn get_v2_pool_from_address(&self, chain_id: u64, address: Address) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self
         .pools
         .iter()
         .find(|(_, p)| p.address() == address && p.chain_id() == chain_id && p.dex_kind().is_v2())
      {
         Some(pool.1)
      } else {
         None
      }
   }

   pub fn get_v3_pool_from_address(&self, chain_id: u64, address: Address) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self
         .pools
         .iter()
         .find(|(_, p)| p.address() == address && p.chain_id() == chain_id && p.dex_kind().is_v3())
      {
         Some(pool.1)
      } else {
         None
      }
   }

   pub fn get_pool_from_address(&self, chain_id: u64, address: Address) -> Option<&AnyUniswapPool> {
      if let Some(pool) = self
         .pools
         .iter()
         .find(|(_, p)| p.address() == address && p.chain_id() == chain_id)
      {
         Some(pool.1)
      } else {
         None
      }
   }

   pub fn calculate_prices(&mut self) {
      for (_, pool) in &self.pools {
         if !pool.enough_liquidity() {
            continue;
         }

         let chain = pool.chain_id();
         let base_tokens = ERC20Token::base_tokens(chain);
         let quote_token = pool.quote_currency().to_erc20();
         let base_token = pool.base_currency().to_erc20();

         // if both tokens are base tokens, skip
         if base_tokens.contains(&quote_token) && base_tokens.contains(&base_token) {
            continue;
         }

         let base_price = self.get_token_price(&base_token).unwrap_or_default();
         let quote_price = pool.quote_price(base_price.f64()).unwrap_or_default();
         if quote_price == 0.0 {
            continue;
         }

         let key = (chain, quote_token.address);
         let quote_price = NumericValue::currency_price(quote_price);
         self.token_prices.insert(key, quote_price);
      }
   }

   async fn fetch_base_token_prices<P, N>(client: P, chain_id: u64) -> Result<TokenPrices, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let mut prices = HashMap::new();
      let tokens = ERC20Token::base_tokens(chain_id);
      for token in tokens {
         let price = get_base_token_price(client.clone(), token.chain_id, token.address, None).await?;
         prices.insert(
            (token.chain_id, token.address),
            NumericValue::currency_price(price),
         );
      }
      Ok(prices)
   }
}

mod serde_hashmap {
   use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
   use std::collections::HashMap;

   pub fn serialize<S, K, V>(map: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: Serializer,
      K: Serialize,
      V: Serialize,
   {
      let stringified_map: HashMap<String, &V> = map
         .iter()
         .map(|(k, v)| (serde_json::to_string(k).unwrap(), v))
         .collect();
      stringified_map.serialize(serializer)
   }

   pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
   where
      D: Deserializer<'de>,
      K: DeserializeOwned + std::cmp::Eq + std::hash::Hash,
      V: DeserializeOwned,
   {
      let stringified_map: HashMap<String, V> = HashMap::deserialize(deserializer)?;
      stringified_map
         .into_iter()
         .map(|(k, v)| {
            let key = serde_json::from_str(&k).map_err(serde::de::Error::custom)?;
            Ok((key, v))
         })
         .collect()
   }
}

#[cfg(test)]
mod tests {
   use alloy_primitives::address;

   use crate::UniswapV2Pool;

   use super::*;

   #[test]
   fn serde_works() {
      let pool = UniswapV2Pool::weth_uni();
      let pool_manager = PoolManager::default();
      let handle = PoolManagerHandle::new(pool_manager);

      handle.add_pool(pool);
      let json = handle.to_string().unwrap();

      let _handle2 = PoolManagerHandle::from_string(&json).unwrap();
   }

   #[test]
   fn sanity_check() {
      let pool_manager = PoolManager::default();
      let handle = PoolManagerHandle::new(pool_manager);
      let dai = ERC20Token::dai();
      let usdt = ERC20Token::usdt();
      let dex = DexKind::UniswapV2;
      let addr = address!("0xB20bd5D04BE54f870D5C0d3cA85d82b34B836405");
      let pool = UniswapV2Pool::new(1, addr, dai, usdt, dex);
      let pool = AnyUniswapPool::from_pool(pool);

      handle.add_pool(pool.clone());
      let pool2 = handle
         .get_pool(
            pool.chain_id(),
            pool.dex_kind(),
            pool.fee().fee(),
            pool.currency0(),
            pool.currency1(),
         )
         .unwrap();
      assert_eq!(pool, pool2);
   }
}
