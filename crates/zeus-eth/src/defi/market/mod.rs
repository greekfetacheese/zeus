use crate::prelude::{
    ERC20Token,
    SUPPORTED_CHAINS,
    batch_request::{ self, V2PoolReserves, V3PoolData },
    is_base_token,
    base_tokens,
    UniswapV2Pool,
    V2State,
    UniswapV3Pool,
    V3State,
};
use crate::defi::utils::chain_link::get_token_price;
use alloy_primitives::{ Address, U256 };
use std::collections::HashMap;
use std::sync::{ Arc, RwLock };

use alloy_contract::private::Network;
use alloy_provider::Provider;
use alloy_transport::Transport;

/// Thread-Safe Handle to the [MarketPriceWatcher]
#[derive(Clone)]
pub struct MarketPriceWatcherHandle(Arc<RwLock<MarketPriceWatcher>>);

impl MarketPriceWatcherHandle {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(MarketPriceWatcher::default())))
    }

    pub fn from(watcher: MarketPriceWatcher) -> Self {
        Self(Arc::new(RwLock::new(watcher)))
    }

    /// Deserialize the [MarketPriceWatcher] from a JSON string
    pub fn from_string(json: &str) -> Result<Self, serde_json::Error> {
        let watcher = MarketPriceWatcher::from_string(json)?;
        Ok(Self(Arc::new(RwLock::new(watcher))))
    }

    /// Serialize the [MarketPriceWatcher] to a JSON string
    pub fn to_string(&self) -> Result<String, serde_json::Error> {
        self.read(|watcher| watcher.to_string())
    }

    /// Shared access to the market price watcher
    pub fn read<R>(&self, reader: impl FnOnce(&MarketPriceWatcher) -> R) -> R {
        reader(&self.0.read().unwrap())
    }

    /// Exclusive mutable access to the market price watcher
    pub fn write<R>(&self, writer: impl FnOnce(&mut MarketPriceWatcher) -> R) -> R {
        writer(&mut self.0.write().unwrap())
    }

    pub fn interval(&self) -> u32 {
        self.read(|watcher| watcher.update_interval)
    }

    pub fn v2_pools(&self) -> V2Pools {
        self.read(|watcher| watcher.v2_pools.clone())
    }

    pub fn v3_pools(&self) -> V3Pools {
        self.read(|watcher| watcher.v3_pools.clone())
    }

    /// Update the interval at which the pool state is updated
    pub fn update_interval(&self, interval: u32) {
        self.write(|watcher| watcher.update_interval(interval));
    }

    /// Get the V2 pool for the given chain and token pair
    pub fn get_v2_pool(&self, chain_id: u64, token0: Address, token1: Address) -> Option<UniswapV2Pool> {
        self.read(|watcher| watcher.get_v2_pool(chain_id, token0, token1))
    }

    /// Get the V3 pool for the given chain, fee and token pair
    pub fn get_v3_pool(&self, chain_id: u64, fee: u32, token0: Address, token1: Address) -> Option<UniswapV3Pool> {
        self.read(|watcher| watcher.get_v3_pool(chain_id, fee, token0, token1))
    }

    /// Add V2 type pools to the watcher
    pub fn add_v2_pools(&self, pools: Vec<UniswapV2Pool>) {
        self.write(|watcher| watcher.add_v2_pools(pools));
    }

    /// Add V3 type pools to the watcher
    pub fn add_v3_pools(&self, pools: Vec<UniswapV3Pool>) {
        self.write(|watcher| watcher.add_v3_pools(pools));
    }

    /// Remove V2 type pools from the watcher
    pub fn remove_v2_pool(&self, chain: u64, token0: Address, token1: Address) {
        self.write(|watcher| watcher.remove_v2_pool(chain, token0, token1));
    }

    /// Remove V3 type pool from the watcher
    pub fn remove_v3_pool(&self, chain: u64, fee: u32, token0: Address, token1: Address) {
        self.write(|watcher| watcher.remove_v3_pool(chain, fee, token0, token1));
    }

    /// Update the state of all the pools for the given chain
    pub async fn update_state<T, P, N>(&self, client: P, chain: u64) -> Result<(), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let v2_pool_map = self.read(|watcher| watcher.v2_pools.clone());
        let v3_pool_map = self.read(|watcher| watcher.v3_pools.clone());
        let v2_pools = v2_pool_map.into_values().collect::<Vec<_>>();
        let v3_pools = v3_pool_map.into_values().collect::<Vec<_>>();
        let (v2_reserves, v3_state) = MarketPriceWatcher::fetch_state(client, chain, v2_pools, v3_pools).await?;
        self.write(|watcher| watcher.update_state(v2_reserves, v3_state))
    }

    /// Update the state for the given pools for the given chain
    pub async fn update_state_for_pools<T, P, N>(
        &self,
        client: P,
        chain: u64,
        v2_pools: Vec<UniswapV2Pool>,
        v3_pools: Vec<UniswapV3Pool>
    )
        -> Result<(), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let (v2_reserves, v3_state) = MarketPriceWatcher::fetch_state(client, chain, v2_pools, v3_pools).await?;
        self.write(|watcher| watcher.update_state(v2_reserves, v3_state))
    }

    /// Update the prices of base tokens for the given chain
    pub async fn update_base_token_prices<T, P, N>(&self, client: P, chain: u64) -> Result<(), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let base_tokens = self.read(|watcher| watcher.base_tokens.get(&chain).cloned().unwrap_or_default());
        let prices = MarketPriceWatcher::fetch_base_token_prices(client, chain, base_tokens).await?;
        self.write(|watcher| watcher.update_base_token_prices(prices));
        Ok(())
    }

    /// Calculate the prices of all tokens in the pools
    pub fn calculate_prices(&self) -> Result<(), anyhow::Error> {
        self.write(|watcher| watcher.calculate_prices())
    }

    /// Get the price of a token
    pub fn get_token_price(&self, token: &ERC20Token) -> f64 {
        self.read(|watcher| watcher.get_token_price(token))
    }

    /// Get the price of a base token
    pub fn get_base_token_price(&self, token: &ERC20Token) -> f64 {
        self.read(|watcher| watcher.get_base_token_price(token))
    }
}

impl Default for MarketPriceWatcherHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl serde::Serialize for MarketPriceWatcherHandle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde_price_watcher::serialize;
        serialize(self, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for MarketPriceWatcherHandle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde_price_watcher::deserialize;
        deserialize(deserializer)
    }
}

/// Uniswap V2 Pools
/// 
/// Key: (chain_id, tokenA, tokenB) -> Value: Pool
pub type V2Pools = HashMap<(u64, Address, Address), UniswapV2Pool>;

/// Uniswap V3 Pools
/// 
/// Key: (chain_id, fee, tokenA, tokenB) -> Value: Pool
pub type V3Pools = HashMap<(u64, u32, Address, Address), UniswapV3Pool>;

/// Multi-chain market price watcher for Uniswap V2 and V3 type pools
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct MarketPriceWatcher {
    /// V2 type pools
    #[serde(with = "serde_hashmap")]
    pub v2_pools: V2Pools,

    /// V3 type pools
    #[serde(with = "serde_hashmap")]
    pub v3_pools: V3Pools,

    /// What we consider as base tokens
    ///
    /// eg. `token/WETH` or `token/USDC` pairs, in this case both WETH and USDC are base tokens
    #[serde(with = "serde_hashmap")]
    pub base_tokens: HashMap<u64, Vec<ERC20Token>>,

    /// Cached usd prices for base tokens
    #[serde(with = "serde_hashmap")]
    pub base_token_prices: HashMap<(u64, Address), f64>,

    /// Cached token prices
    #[serde(with = "serde_hashmap")]
    pub token_prices: HashMap<(u64, Address), f64>,

    /// How often to update the pool state in seconds
    pub update_interval: u32,
}

impl MarketPriceWatcher {
    pub fn new(
        v2_pools: V2Pools,
        v3_pools: V3Pools,
        base_tokens: HashMap<u64, Vec<ERC20Token>>,
        base_token_prices: HashMap<(u64, Address), f64>,
        token_prices: HashMap<(u64, Address), f64>,
        update_interval: u32
    ) -> Self {
        Self {
            v2_pools,
            v3_pools,
            base_tokens,
            base_token_prices,
            token_prices,
            update_interval,
        }
    }

    pub fn default() -> Self {
        let mut base_tokens_map = HashMap::new();
        for chain in SUPPORTED_CHAINS.iter() {
            base_tokens_map.insert(*chain, base_tokens(*chain));
        }

        Self {
            v2_pools: HashMap::new(),
            v3_pools: HashMap::new(),
            base_tokens: base_tokens_map,
            base_token_prices: HashMap::new(),
            token_prices: HashMap::new(),
            update_interval: 60,
        }
    }

    pub fn from_string(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn to_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn get_v2_pool(&self, chain_id: u64, token0: Address, token1: Address) -> Option<UniswapV2Pool> {
        self.v2_pools.get(&(chain_id, token0, token1)).cloned()
    }

    pub fn get_v3_pool(&self, chain_id: u64, fee: u32, token0: Address, token1: Address) -> Option<UniswapV3Pool> {
        self.v3_pools.get(&(chain_id, fee, token0, token1)).cloned()
    }

    pub fn add_v2_pools(&mut self, pools: Vec<UniswapV2Pool>) {
        for pool in pools {
            self.v2_pools.insert((pool.chain_id, pool.token0.address, pool.token1.address), pool);
        }
    }

    pub fn add_v3_pools(&mut self, pools: Vec<UniswapV3Pool>) {
        for pool in pools {
            self.v3_pools.insert((pool.chain_id, pool.fee, pool.token0.address, pool.token1.address), pool);
        }
    }

    pub fn remove_v2_pool(&mut self, chain_id: u64, token0: Address, token1: Address) {
        self.v2_pools.remove(&(chain_id, token0, token1));
    }

    pub fn remove_v3_pool(&mut self, chain_id: u64, fee: u32, token0: Address, token1: Address) {
        self.v3_pools.remove(&(chain_id, fee, token0, token1));
    }

    pub fn update_interval(&mut self, interval: u32) {
        self.update_interval = interval;
    }

    pub fn get_token_price(&self, token: &ERC20Token) -> f64 {
        self.token_prices.get(&(token.chain_id, token.address)).cloned().unwrap_or(0.0)
    }

    pub fn get_base_token_price(&self, token: &ERC20Token) -> f64 {
        self.base_token_prices.get(&(token.chain_id, token.address)).cloned().unwrap_or(0.0)
    }

    pub async fn fetch_base_token_prices<T, P, N>(
        client: P,
        chain_id: u64,
        base_tokens: Vec<ERC20Token>
    )
        -> Result<HashMap<Address, f64>, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let mut prices = HashMap::new();
        for token in base_tokens {
            let price = get_token_price(client.clone(), None, chain_id, token.address).await?;
            prices.insert(token.address, price);
        }

        Ok(prices)
    }

    pub fn update_base_token_prices(&mut self, prices: HashMap<Address, f64>) {
        for (chain_id, tokens) in &self.base_tokens {
            for token in tokens {
                let price = prices.get(&token.address).cloned().unwrap_or(0.0);
                self.base_token_prices.insert((*chain_id, token.address), price);
            }
        }
    }

    pub async fn fetch_state<T, P, N>(
        client: P,
        chain_id: u64,
        v2_pools: Vec<UniswapV2Pool>,
        v3_pools: Vec<UniswapV3Pool>
    )
        -> Result<(Vec<V2PoolReserves>, Vec<V3PoolData>), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        const BATCH_SIZE: usize = 100;

        // Process V2 pools in batches
        let v2_addresses: Vec<Address> = v2_pools
            .into_iter()
            .filter(|p| p.chain_id == chain_id)
            .map(|p| p.address)
            .collect();

        let v2_reserves = futures::future::try_join_all(
            v2_addresses
                .chunks(BATCH_SIZE)
                .map(|chunk| batch_request::get_v2_pool_reserves(client.clone(), None, chunk.to_vec()))
        ).await?;

        // Process V3 pools in batches
        let v3_addresses: Vec<Address> = v3_pools
            .into_iter()
            .filter(|p| p.chain_id == chain_id)
            .map(|p| p.address)
            .collect();

        let v3_state = futures::future::try_join_all(
            v3_addresses
                .chunks(BATCH_SIZE)
                .map(|chunk| batch_request::get_v3_state(client.clone(), None, chunk.to_vec()))
        ).await?;

        Ok((v2_reserves.into_iter().flatten().collect(), v3_state.into_iter().flatten().collect()))
    }

    pub fn update_state(
        &mut self,
        v2_state: Vec<V2PoolReserves>,
        v3_state: Vec<V3PoolData>
    ) -> Result<(), anyhow::Error> {
        // make sure no matter the order of the pools, we can match them
        let v2_state_map: HashMap<Address, _> = v2_state
            .into_iter()
            .map(|s| (s.pool, s))
            .collect();

        let v3_state_map: HashMap<Address, _> = v3_state
            .into_iter()
            .map(|d| (d.pool, d))
            .collect();

        let pool_addr = self.v2_pools.iter().map(|(_, pool)| pool.clone()).collect::<Vec<_>>();

        for pool in pool_addr {
            if let Some(data) = v2_state_map.get(&pool.address) {
                let key = &(pool.chain_id, pool.token0.address, pool.token1.address);
                let pool = self.v2_pools.get_mut(key).map_or_else(|| Err(anyhow::anyhow!("V2 Pool not found")), Ok)?;
                let reserve0 = U256::from(data.reserve0);
                let reserve1 = U256::from(data.reserve1);
                let v2_state = V2State::new(reserve0, reserve1, data.blockTimestampLast as u64);
                pool.update_state(v2_state);
            }
        }

        let pool_addr = self.v3_pools.iter().map(|(_, pool)| pool.clone()).collect::<Vec<_>>();

        for pool in pool_addr {
            if let Some(data) = v3_state_map.get(&pool.address) {
                let key = &(pool.chain_id, pool.fee, pool.token0.address, pool.token1.address);
                let pool = self.v3_pools.get_mut(key).map_or_else(|| Err(anyhow::anyhow!("V3 Pool not found")), Ok)?;
                let v3_state = V3State::new(data.clone())?;
                pool.update_state(v3_state);
            }
        }

        Ok(())
    }

    pub fn calculate_prices(&mut self) -> Result<(), anyhow::Error> {
        for pool in self.v2_pools.values() {
            if is_base_token(&pool.token0) {
                let token0_usd = self.base_token_prices.get(&(pool.chain_id, pool.token0.address)).unwrap_or(&0.0);
                let token1_price = pool.token1_price(*token0_usd)?;
                self.token_prices.insert((pool.chain_id, pool.token1.address), token1_price);
            } else if is_base_token(&pool.token1) {
                let token1_usd = self.base_token_prices.get(&(pool.chain_id, pool.token1.address)).unwrap_or(&0.0);
                let token0_price = pool.token0_price(*token1_usd)?;
                self.token_prices.insert((pool.chain_id, pool.token0.address), token0_price);
            }
        }

        for pool in self.v3_pools.values() {
            if is_base_token(&pool.token0) {
                let token0_usd = self.base_token_prices.get(&(pool.chain_id, pool.token0.address)).unwrap_or(&0.0);
                let token1_price = pool.token1_price(*token0_usd)?;
                self.token_prices.insert((pool.chain_id, pool.token1.address), token1_price);
            } else if is_base_token(&pool.token1) {
                let token1_usd = self.base_token_prices.get(&(pool.chain_id, pool.token1.address)).unwrap_or(&0.0);
                let token0_price = pool.token0_price(*token1_usd)?;
                self.token_prices.insert((pool.chain_id, pool.token0.address), token0_price);
            }
        }

        Ok(())
    }
}

mod serde_price_watcher {
    use super::MarketPriceWatcherHandle;
    use serde::{Deserialize, Deserializer, Serializer };

    pub fn serialize<S>(price_watcher: &MarketPriceWatcherHandle, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        let string = price_watcher.to_string().map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&string)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MarketPriceWatcherHandle, D::Error> where D: Deserializer<'de> {
        let string = String::deserialize(deserializer)?;
        MarketPriceWatcherHandle::from_string(&string).map_err(serde::de::Error::custom)
    }
}

mod serde_hashmap {
    use serde::{ de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer };
    use std::collections::HashMap;

    pub fn serialize<S, K, V>(map: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer, K: Serialize, V: Serialize
    {
        let stringified_map: HashMap<String, &V> = map
            .iter()
            .map(|(k, v)| (serde_json::to_string(k).unwrap(), v))
            .collect();
        stringified_map.serialize(serializer)
    }

    pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
        where D: Deserializer<'de>, K: DeserializeOwned + std::cmp::Eq + std::hash::Hash, V: DeserializeOwned
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
    use crate::prelude::{ ERC20Token, DexKind, UniswapV2Pool, weth };
    use alloy_primitives::address;
    use alloy_provider::ProviderBuilder;
    use alloy_transport_http::reqwest::Url;

    use super::*;

    #[tokio::test]
    async fn test_price_watcher() {
        let url = Url::parse("https://eth.merkle.io").unwrap();
        let client = ProviderBuilder::new().on_http(url);

        let weth = ERC20Token::new(client.clone(), weth(1).unwrap(), 1).await.unwrap();
        let uni_addr = address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984");
        let uni = ERC20Token::new(client.clone(), uni_addr, 1).await.unwrap();

        let pool_address = address!("d3d2E2692501A5c9Ca623199D38826e513033a17");
        let pool = UniswapV2Pool::new(1, pool_address, weth.clone(), uni.clone(), DexKind::UniswapV2);

        let price_watcher = MarketPriceWatcherHandle::new();
        price_watcher.add_v2_pools(vec![pool]);

        price_watcher.update_base_token_prices(client.clone(), 1).await.unwrap();
        price_watcher.update_state(client, 1).await.unwrap();
        price_watcher.calculate_prices().unwrap();

        let weth_price = price_watcher.get_base_token_price(&weth);
        let uni_price = price_watcher.get_token_price(&uni);

        println!("WETH Price: ${}", weth_price);
        println!("UNI Price: ${}", uni_price);
    }

    #[test]
    fn watcher_serde_test() {
        let price_watcher = MarketPriceWatcherHandle::new();
        let json = price_watcher.to_string().expect("Failed to serialize price watcher");
        let price_watcher = MarketPriceWatcherHandle::from_string(&json).expect("Failed to deserialize price watcher");

        #[derive(serde::Serialize, serde::Deserialize)]
        struct Temp {
            price_watcher: MarketPriceWatcherHandle,
        }

        let temp = Temp { price_watcher };

        let json = serde_json::to_string(&temp).expect("Struct Serialization failed");
        let _temp: Temp = serde_json::from_str(&json).expect("Struct Deserialization failed");
    }
}