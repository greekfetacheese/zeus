use alloy_primitives::{ Address, U256 };

use std::collections::HashMap;
use std::time::Duration;
use std::sync::{ Arc, RwLock };
use tokio::{ sync::{ Mutex, Semaphore }, task::JoinHandle };

use alloy_contract::private::Network;
use alloy_provider::Provider;
use alloy_transport::Transport;

use crate::uniswap::{ v2::pool::{ PoolReserves, UniswapV2Pool }, v3::pool::{ V3PoolState, UniswapV3Pool } };
use crate::DexKind;
use currency::erc20::ERC20Token;
use utils::{
    price_feed::get_base_token_price,
    batch_request::{ V3Pool2, get_v2_pool_reserves, get_v3_pools, get_v3_state, V2PoolReserves, V3PoolData },
};

use serde::{ Deserialize, Serialize };

/// Thread-safe handle to the [PoolStateManager]
#[derive(Clone)]
pub struct PoolStateManagerHandle(Arc<RwLock<PoolStateManager>>);

impl Default for PoolStateManagerHandle {
    fn default() -> Self {
        Self::new(PoolStateManager::default())
    }
}

impl serde::Serialize for PoolStateManagerHandle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
        use serde_pool_manage_handle::serialize;
        serialize(self, serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PoolStateManagerHandle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de> {
        use serde_pool_manage_handle::deserialize;
        deserialize(deserializer)
    }
}

impl PoolStateManagerHandle {
    pub fn new(state: PoolStateManager) -> Self {
        Self(Arc::new(RwLock::new(state)))
    }

    /// Shared access to the market price watcher
    pub fn read<R>(&self, reader: impl FnOnce(&PoolStateManager) -> R) -> R {
        reader(&self.0.read().unwrap())
    }

    /// Exclusive mutable access to the market price watcher
    pub fn write<R>(&self, writer: impl FnOnce(&mut PoolStateManager) -> R) -> R {
        writer(&mut self.0.write().unwrap())
    }

    /// Deserialize the [PoolStateManager] from a JSON string
    pub fn from_string(json: &str) -> Result<Self, serde_json::Error> {
        let manager = serde_json::from_str(json)?;
        Ok(Self(Arc::new(RwLock::new(manager))))
    }

    /// Deserialize the [PoolStateManager] from a JSON string
    pub fn from_slice(data: &[u8]) -> Result<Self, serde_json::Error> {
        let manager = serde_json::from_slice(data)?;
        Ok(Self(Arc::new(RwLock::new(manager))))
    }

    /// Serialize the [PoolStateManager] to a JSON string
    pub fn to_string(&self) -> Result<String, serde_json::Error> {
        self.read(|manager| serde_json::to_string(manager))
    }

    pub fn v2_pools(&self) -> V2Pools {
        self.read(|manager| manager.v2_pools.clone())
    }

    pub fn v3_pools(&self) -> V3Pools {
        self.read(|manager| manager.v3_pools.clone())
    }

    /// Get a specific v2 pools for the given chain and token pair
    pub fn get_v2_pool(&self, chain_id: u64, token0: Address, token1: Address) -> Option<UniswapV2Pool> {
        self.read(|manager| manager.get_v2_pool(chain_id, token0, token1))
    }

    /// Get a specific v3 pools for the given chain, fee and token pair
    pub fn get_v3_pool(&self, chain_id: u64, fee: u32, token0: Address, token1: Address) -> Option<UniswapV3Pool> {
        self.read(|manager| manager.get_v3_pool(chain_id, fee, token0, token1))
    }

    /// Add these v2 pools to the manager
    pub fn add_v2_pools(&self, pools: Vec<UniswapV2Pool>) {
        self.write(|manager| manager.add_v2_pools(pools))
    }

    /// Add these v3 pools to the manager
    pub fn add_v3_pools(&self, pools: Vec<UniswapV3Pool>) {
        self.write(|manager| manager.add_v3_pools(pools))
    }

    /// Remove a v2 pool from the manager
    pub fn remove_v2_pool(&self, chain_id: u64, token0: Address, token1: Address) {
        self.write(|manager| manager.remove_v2_pool(chain_id, token0, token1))
    }

    /// Remove a v3 pool from the manager
    pub fn remove_v3_pool(&self, chain_id: u64, fee: u32, token0: Address, token1: Address) {
        self.write(|manager| manager.remove_v3_pool(chain_id, fee, token0, token1))
    }

    /// Update everything in the manager
    pub async fn update<T, P, N>(&self, client: P, chain: u64, tokens: Vec<ERC20Token>) -> Result<(), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone + 'static, N: Network
    {
        self.update_pool_state(client.clone(), chain).await?;
        self.update_base_token_prices(client.clone(), tokens).await?;
        self.calculate_prices()?;
        Ok(())
    }

    /// Update the state of all the pools for the given chain
    pub async fn update_pool_state<T, P, N>(&self, client: P, chain: u64) -> Result<(), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone + 'static, N: Network
    {
        let v2_pool_map = self.read(|manager| manager.v2_pools.clone());
        let v3_pool_map = self.read(|manager| manager.v3_pools.clone());
        let concurrency = self.read(|manager| manager.concurrency);
        let v2_pools = v2_pool_map.into_values().collect::<Vec<_>>();
        let v3_pools = v3_pool_map.into_values().collect::<Vec<_>>();
        let (v2_reserves, v3_state) = PoolStateManager::fetch_state(
            client,
            chain,
            concurrency,
            v2_pools,
            v3_pools
        ).await?;
        self.write(|manager| manager.update_state(v2_reserves, v3_state))
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
        where T: Transport + Clone, P: Provider<T, N> + Clone + 'static, N: Network
    {
        let concurrency = self.read(|manager| manager.concurrency);
        let (v2_reserves, v3_state) = PoolStateManager::fetch_state(
            client,
            chain,
            concurrency,
            v2_pools,
            v3_pools
        ).await?;
        self.write(|manager| manager.update_state(v2_reserves, v3_state))
    }

    /// Update the base token prices for the given tokens
    pub async fn update_base_token_prices<T, P, N>(
        &self,
        client: P,
        tokens: Vec<ERC20Token>
    )
        -> Result<(), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let prices = PoolStateManager::fetch_base_token_prices(client, tokens).await?;
        self.write(|manager| manager.update_base_token_prices(prices));
        Ok(())
    }

    /// Get all the possible v2 pools for the given token based on:
    ///
    /// - The token's chain id
    /// - All the possible [DexKind] for the chain
    /// - Base Tokens [ERC20Token::base_tokens()]
    pub async fn get_v2_pools_for_token<T, P, N>(&self, client: P, token: ERC20Token) -> Result<(), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let base_tokens = ERC20Token::base_tokens(token.chain_id);
        let dex_kinds = DexKind::all(token.chain_id);

        let mut pools = Vec::new();
        for base_token in base_tokens {
            if base_token.address == token.address {
                continue;
            }

            for dex in &dex_kinds {
                if dex.is_pancakeswap_v3() || dex.is_uniswap_v3() {
                    continue;
                }

                tracing::info!(
                    "Getting v2 pool for: {}-{} on: {} Chain Id: {}",
                    token.symbol,
                    base_token.symbol,
                    dex.to_str(),
                    token.chain_id
                );

                let pool = UniswapV2Pool::from(
                    client.clone(),
                    token.chain_id,
                    token.clone(),
                    base_token.clone(),
                    *dex
                ).await;
                if let Ok(pool) = pool {
                    pools.push(pool);
                }
            }
        }
        self.add_v2_pools(pools);
        Ok(())
    }

    /// Get all the possible v3 pools for the given token based on:
    ///
    /// - The token's chain id
    /// - All the possible [DexKind] for the chain
    /// - Base Tokens [ERC20Token::base_tokens()]
    pub async fn get_v3_pools_for_token<T, P, N>(&self, client: P, token: ERC20Token) -> Result<(), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let base_tokens = ERC20Token::base_tokens(token.chain_id);
        let dex_kinds = DexKind::all(token.chain_id);

        let mut pools = Vec::new();

        for base_token in &base_tokens {
            if base_token.address == token.address {
                continue;
            }

            for dex in &dex_kinds {
                if dex.is_pancakeswap_v2() || dex.is_uniswap_v2() {
                    continue;
                }

                let factory = dex.factory(token.chain_id)?;
                tracing::info!(
                    "Getting v3 pools for: {}-{} on: {} with factory {} Chain Id: {}",
                    token.address,
                    base_token.address,
                    dex.to_str(),
                    factory,
                    token.chain_id
                );
                let v3_pools = get_v3_pools(client.clone(), token.address, base_token.address, factory).await?;
                pools.extend(v3_pools);
            }
        }

        let mut pool_result = Vec::new();
        for base_token in base_tokens {
            if base_token.address == token.address {
                continue;
            }

            for dex in &dex_kinds {
                if dex.is_pancakeswap_v2() || dex.is_uniswap_v2() {
                    continue;
                }
                for pool in &pools {
                    if !pool.addr.is_zero() {
                        let fee: u32 = pool.fee.to_string().parse()?;
                        pool_result.push(
                            UniswapV3Pool::new(token.chain_id, pool.addr, fee, token.clone(), base_token.clone(), *dex)
                        );
                    }
                }
            }
        }

        self.add_v3_pools(pool_result);
        Ok(())
    }

    /// Calculate the prices for all the pools
    pub fn calculate_prices(&self) -> Result<(), anyhow::Error> {
        self.write(|manager| manager.calculate_prices())
    }

    /// Get the price of the given token
    pub fn get_token_price(&self, token: &ERC20Token) -> Option<f64> {
        self.read(|manager| manager.get_token_price(token))
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

/// Token Prices
///
/// Key: (chain_id, token) -> Value: Price
pub type TokenPrices = HashMap<(u64, Address), f64>;

/// Multi-chain pool state manager for Uniswap V2 and V3 type of pools
#[derive(Clone, Serialize, Deserialize)]
pub struct PoolStateManager {
    #[serde(with = "serde_hashmap")]
    pub v2_pools: V2Pools,

    #[serde(with = "serde_hashmap")]
    pub v3_pools: V3Pools,

    #[serde(with = "serde_hashmap")]
    pub token_prices: TokenPrices,

    /// Set to 1 for no concurrency
    pub concurrency: u8,
}

impl Default for PoolStateManager {
    fn default() -> Self {
        Self {
            v2_pools: HashMap::new(),
            v3_pools: HashMap::new(),
            token_prices: HashMap::new(),
            concurrency: 1,
        }
    }
}

impl PoolStateManager {
    pub fn new(v2_pools: V2Pools, v3_pools: V3Pools, token_prices: TokenPrices, concurrency: u8) -> Self {
        Self { v2_pools, v3_pools, token_prices, concurrency }
    }

    pub fn get_v2_pool(&self, chain_id: u64, token0: Address, token1: Address) -> Option<UniswapV2Pool> {
        let mut pool = None;
        if let Some(p) = self.v2_pools.get(&(chain_id, token0, token1)) {
            pool = Some(p.clone());
        } else if let Some(p) = self.v2_pools.get(&(chain_id, token1, token0)) {
            pool = Some(p.clone());
        }
        pool
    }

    pub fn get_v3_pool(&self, chain_id: u64, fee: u32, token0: Address, token1: Address) -> Option<UniswapV3Pool> {
        let mut pool = None;
        if let Some(p) = self.v3_pools.get(&(chain_id, fee, token0, token1)) {
            pool = Some(p.clone());
        } else if let Some(p) = self.v3_pools.get(&(chain_id, fee, token1, token0)) {
            pool = Some(p.clone());
        }
        pool
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

    pub fn get_token_price(&self, token: &ERC20Token) -> Option<f64> {
        self.token_prices.get(&(token.chain_id, token.address)).cloned()
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

        let pools = self.v2_pools
            .iter()
            .map(|(_, pool)| pool.clone())
            .collect::<Vec<_>>();

        for pool in pools {
            if let Some(data) = v2_state_map.get(&pool.address) {
                let key = &(pool.chain_id, pool.token0.address, pool.token1.address);
                let pool = self.v2_pools.get_mut(key).map_or_else(|| Err(anyhow::anyhow!("V2 Pool not found")), Ok)?;
                let reserve0 = U256::from(data.reserve0);
                let reserve1 = U256::from(data.reserve1);
                let v2_state = PoolReserves::new(reserve0, reserve1, data.blockTimestampLast as u64);
                pool.update_state(v2_state);
            }
        }

        let pool_addr = self.v3_pools
            .iter()
            .map(|(_, pool)| pool.clone())
            .collect::<Vec<_>>();

        for pool in pool_addr {
            if let Some(data) = v3_state_map.get(&pool.address) {
                let key = &(pool.chain_id, pool.fee, pool.token0.address, pool.token1.address);
                let pool = self.v3_pools.get_mut(key).map_or_else(|| Err(anyhow::anyhow!("V3 Pool not found")), Ok)?;
                let v3_state = V3PoolState::new(data.clone(), None)?;
                pool.update_state(v3_state);
            }
        }

        Ok(())
    }

    pub fn calculate_prices(&mut self) -> Result<(), anyhow::Error> {
        let mut prices: HashMap<(u64, Address), Vec<f64>> = HashMap::new();

        for (_, pool) in self.v2_pools.iter_mut() {
            if !pool.enough_liquidity() {
                continue;
            }

            let quote = pool.quote_token();
            let base_token = pool.base_token();
            let base_price = self.token_prices.get(&(base_token.chain_id, base_token.address)).cloned().unwrap_or(0.0);
            pool.base_usd = base_price;

            let quote_price = pool.caluclate_quote_price(base_price);
            if quote_price == 0.0 {
                continue;
            }
            prices.entry((pool.chain_id, quote.address)).or_insert_with(Vec::new).push(quote_price);
        }

         
        for (_, pool) in self.v3_pools.iter_mut() {
            if !pool.enough_liquidity() {
                continue;
            }

            let quote = pool.quote_token();
            let base_token = pool.base_token();
            let base_price = self.token_prices.get(&(base_token.chain_id, base_token.address)).cloned().unwrap_or(0.0);
            pool.base_usd = base_price;

            let quote_price = pool.caluclate_quote_price(base_price);
            if quote_price == 0.0 {
                continue;
            }
            prices.entry((pool.chain_id, quote.address)).or_insert_with(Vec::new).push(quote_price);
        }
        

        // calculate the average price
        for ((chain_id, token), prices) in prices {
            let price = prices.iter().sum::<f64>() / prices.len() as f64;
            self.token_prices.insert((chain_id, token), price);
        }

        Ok(())
    }

    pub fn update_base_token_prices(&mut self, prices: TokenPrices) {
        for (key, price) in prices {
            self.token_prices.insert(key, price);
        }
    }
}

// * Fetchers

impl PoolStateManager {
    pub async fn fetch_base_token_prices<T, P, N>(
        client: P,
        tokens: Vec<ERC20Token>
    )
        -> Result<TokenPrices, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let mut prices = HashMap::new();
        for token in tokens {
            let price = get_base_token_price(client.clone(), token.chain_id, token.address, None).await?;
            prices.insert((token.chain_id, token.address), price);
        }
        Ok(prices)
    }

    pub async fn fetch_state<T, P, N>(
        client: P,
        chain_id: u64,
        concurrency: u8,
        v2_pools: Vec<UniswapV2Pool>,
        v3_pools: Vec<UniswapV3Pool>
    )
        -> Result<(Vec<V2PoolReserves>, Vec<V3PoolData>), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone + 'static, N: Network
    {
        const BATCH_SIZE: usize = 100;
        const BATCH_SIZE_2: usize = 10;
        const MAX_RETRY: usize = 5;

        let v2_addresses: Vec<Address> = v2_pools
            .iter()
            .filter(|p| p.chain_id == chain_id)
            .map(|p| p.address)
            .collect();

        let v2_reserves = Arc::new(Mutex::new(Vec::new()));
        let mut v2_tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
        let semaphore = Arc::new(Semaphore::new(concurrency as usize));
        let delay = Arc::new(Mutex::new(0));
        let retry = Arc::new(Mutex::new(0));

        for chunk in v2_addresses.chunks(BATCH_SIZE) {
            let chunk_clone = chunk.to_vec();
            let client = client.clone();
            let semaphore = semaphore.clone();
            let v2_reserves = v2_reserves.clone();
            let delay = delay.clone();
            let retry = retry.clone();

            let task = tokio::spawn({
                async move {
                    let mut got_it = false;
                    while !got_it && *retry.lock().await < MAX_RETRY {
                        let _permit = semaphore.acquire().await?;
                        let data_res = get_v2_pool_reserves(client.clone(), None, chunk_clone.clone()).await;
                        match data_res {
                            Ok(data) => {
                                v2_reserves.lock().await.extend(data);
                                got_it = true;
                            }
                            Err(e) => {
                                tracing::error!("Error fetching v2 pool reserves: {:?}", e);
                                let mut delay = delay.lock().await;
                                *delay += 500;
                                tokio::time::sleep(Duration::from_millis(*delay)).await;
                                *retry.lock().await += 1;
                            }
                        }
                    }
                    Ok(())
                }
            });
            v2_tasks.push(task);
        }

        let pools: Vec<UniswapV3Pool> = v3_pools
            .iter()
            .filter(|p| p.chain_id == chain_id)
            .map(|p| p.clone())
            .collect();

        let v3_data = Arc::new(Mutex::new(Vec::new()));
        let mut v3_tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
        let delay = Arc::new(Mutex::new(0));
        let retry = Arc::new(Mutex::new(0));

        for pool in pools.chunks(BATCH_SIZE_2) {
            let client = client.clone();
            let semaphore = semaphore.clone();
            let v3_data = v3_data.clone();
            let delay = delay.clone();
            let retry = retry.clone();

            let mut pools2 = Vec::new();
            for pool in pool {
                pools2.push(V3Pool2 {
                    pool: pool.address,
                    base_token: pool.base_token().address,
                });
            }

            let task = tokio::spawn({
                async move {
                    let mut got_it = false;
                    while !got_it && *retry.lock().await < MAX_RETRY {
                        let _permit = semaphore.acquire().await?;
                        let data_res = get_v3_state(client.clone(), None, pools2.clone()).await;
                        match data_res {
                            Ok(data) => {
                                v3_data.lock().await.extend(data);
                                got_it = true;
                            }
                            Err(e) => {
                                tracing::error!("Error fetching v3 pool data: {:?}", e);
                                let mut delay = delay.lock().await;
                                *delay += 500;
                                tokio::time::sleep(Duration::from_millis(*delay)).await;
                                *retry.lock().await += 1;
                            }
                        }
                    }
                    Ok(())
                }
            });
            v3_tasks.push(task);
        }

        for task in v2_tasks {
            if let Err(e) = task.await? {
                tracing::error!("Error fetching v2 pool reserves: {:?}", e);
            }
        }

        for task in v3_tasks {
            if let Err(e) = task.await? {
                tracing::error!("Error fetching v3 pool data: {:?}", e);
            }
        }

        let v2_reserves = Arc::try_unwrap(v2_reserves).unwrap().into_inner();
        let v3_data = Arc::try_unwrap(v3_data).unwrap().into_inner();

        Ok((v2_reserves, v3_data))
    }
}

mod serde_pool_manage_handle {
    use super::PoolStateManagerHandle;
    use serde::{ Deserialize, Deserializer, Serializer };

    pub fn serialize<S>(pool_manager: &PoolStateManagerHandle, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        let string = pool_manager.to_string().map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&string)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PoolStateManagerHandle, D::Error> where D: Deserializer<'de> {
        let string = String::deserialize(deserializer)?;
        PoolStateManagerHandle::from_string(&string).map_err(serde::de::Error::custom)
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
