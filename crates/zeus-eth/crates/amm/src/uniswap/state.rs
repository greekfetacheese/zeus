use super::{AnyUniswapPool, UniswapPool};
use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, U256, aliases::I24};
use alloy_rpc_types::BlockId;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use utils::batch::{self, V2PoolReserves, V3Pool, V3PoolData};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum State {
   V2(PoolReserves),
   V3(V3PoolState),
   // Same as V3, just add it here to avoid any confusions
   V4(V3PoolState),
   None,
}

impl Default for State {
   fn default() -> Self {
      Self::None
   }
}

impl State {
   pub fn none() -> Self {
      Self::None
   }

   pub fn v2(reserves: PoolReserves) -> Self {
      Self::V2(reserves)
   }

   pub fn v3(state: V3PoolState) -> Self {
      Self::V3(state)
   }

   pub fn v4(state: V3PoolState) -> Self {
      Self::V4(state)
   }

   pub fn is_none(&self) -> bool {
      matches!(self, Self::None)
   }

   pub fn is_v2(&self) -> bool {
      matches!(self, Self::V2(_))
   }

   pub fn is_v3(&self) -> bool {
      matches!(self, Self::V3(_))
   }

   pub fn is_v4(&self) -> bool {
      matches!(self, Self::V4(_) | Self::V3(_))
   }

   pub fn v2_reserves(&self) -> Option<&PoolReserves> {
      match self {
         Self::V2(reserves) => Some(reserves),
         _ => None,
      }
   }

   pub fn v3_state(&self) -> Option<&V3PoolState> {
      match self {
         Self::V3(state) => Some(state),
         _ => None,
      }
   }

   pub fn v3_or_v4_state(&self) -> Option<&V3PoolState> {
      match self {
         Self::V3(state) => Some(state),
         Self::V4(state) => Some(state),
         _ => None,
      }
   }
}

/// Represents the state of a Uniswap V2 Pool
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolReserves {
   pub reserve0: U256,
   pub reserve1: U256,
   pub block: u64,
}

impl From<V2PoolReserves> for PoolReserves {
   fn from(value: V2PoolReserves) -> Self {
      let reserve0 = U256::from(value.reserve0);
      let reserve1 = U256::from(value.reserve1);
      Self {
         reserve0,
         reserve1,
         block: value.blockTimestampLast as u64,
      }
   }
}

impl PoolReserves {
   pub fn new(reserve0: U256, reserve1: U256, block: u64) -> Self {
      Self {
         reserve0,
         reserve1,
         block,
      }
   }
}

/// The state of a Uniswap V3/V4 Pool
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct V3PoolState {
   pub liquidity: u128,
   pub sqrt_price: U256,
   /// Current Tick
   pub tick: i32,
   pub tick_spacing: i32,
   pub tick_bitmap: HashMap<i16, U256>,
   pub ticks: HashMap<i32, TickInfo>,
   pub pool_tick: PoolTick,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickInfo {
   pub liquidity_gross: u128,
   pub liquidity_net: i128,
   pub initialized: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PoolTick {
   pub tick: i32,
   pub liquidity_net: i128,
   pub block: u64,
}

impl V3PoolState {
   pub fn new(pool_data: V3PoolData, tick_spacing: I24, block: Option<BlockId>) -> Result<Self, anyhow::Error> {
      let mut tick_bitmap_map = HashMap::new();
      tick_bitmap_map.insert(pool_data.wordPos, pool_data.tickBitmap);

      let ticks_info = TickInfo {
         liquidity_gross: pool_data.liquidityGross,
         liquidity_net: pool_data.liquidityNet,
         initialized: pool_data.initialized,
      };

      let block = if let Some(b) = block {
         b.as_u64().unwrap_or(0)
      } else {
         0
      };
      let tick: i32 = pool_data.tick.to_string().parse()?;

      let pool_tick = PoolTick {
         tick,
         liquidity_net: pool_data.liquidityNet,
         block,
      };

      let mut ticks_map = HashMap::new();
      ticks_map.insert(tick, ticks_info);

      let tick_spacing: i32 = tick_spacing.to_string().parse()?;

      Ok(Self {
         liquidity: pool_data.liquidity,
         sqrt_price: U256::from(pool_data.sqrtPriceX96),
         tick,
         tick_spacing,
         tick_bitmap: tick_bitmap_map,
         ticks: ticks_map,
         pool_tick,
      })
   }

   pub fn v4(pool: &impl UniswapPool, data: batch::V4PoolData, block: Option<BlockId>) -> Result<Self, anyhow::Error> {
      let mut tick_bitmap_map = HashMap::new();
      tick_bitmap_map.insert(data.wordPos, data.tickBitmap);

      let ticks_info = TickInfo {
         liquidity_gross: data.liquidityGross,
         liquidity_net: data.liquidityNet,
         initialized: true,
      };

      let block = if let Some(b) = block {
         b.as_u64().unwrap_or(0)
      } else {
         0
      };
      let tick: i32 = data.tick.to_string().parse()?;

      let pool_tick = PoolTick {
         tick,
         liquidity_net: data.liquidityNet,
         block,
      };

      let mut ticks_map = HashMap::new();
      ticks_map.insert(tick, ticks_info);

      let tick_spacing = pool.fee().tick_spacing_i32();

      Ok(Self {
         liquidity: data.liquidity,
         sqrt_price: U256::from(data.sqrtPriceX96),
         tick,
         tick_spacing,
         tick_bitmap: tick_bitmap_map,
         ticks: ticks_map,
         pool_tick,
      })
   }
}



pub async fn get_v2_pool_state<P, N>(
   client: P,
   pool: &impl UniswapPool,
   block: Option<BlockId>,
) -> Result<State, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if !pool.dex_kind().is_v2() {
      return Err(anyhow::anyhow!("Pool is not v2"));
   }
   let reserves = abi::uniswap::v2::pool::get_reserves(pool.address(), client, block).await?;
   let reserve0 = U256::from(reserves.0);
   let reserve1 = U256::from(reserves.1);
   let reserves = PoolReserves::new(reserve0, reserve1, reserves.2 as u64);

   Ok(State::v2(reserves))
}

pub async fn get_v3_pool_state<P, N>(
   client: P,
   pool: &impl UniswapPool,
   block: Option<BlockId>,
) -> Result<(State, V3PoolData), anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if !pool.dex_kind().is_v3() {
      return Err(anyhow::anyhow!("Pool is not v3"));
   }

   let address = pool.address();
   let tick_spacing = pool.fee().tick_spacing();
   let token0 = pool.currency0().to_erc20().address;
   let token1 = pool.currency1().to_erc20().address;
   let pool2 = utils::batch::V3Pool {
      pool: address,
      token0,
      token1,
      tickSpacing: tick_spacing,
   };

   let pool_data = batch::get_v3_state(client, block, vec![pool2]).await?;
   let data = pool_data
      .get(0)
      .cloned()
      .ok_or_else(|| anyhow!("Pool data not found"))?;

   let v3_pool_state = V3PoolState::new(data.clone(), tick_spacing, block)?;
   Ok((State::v3(v3_pool_state), data))
}

pub async fn get_v4_pool_state<P, N>(
   client: P,
   pool: &mut impl UniswapPool,
   block: Option<BlockId>,
) -> Result<State, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if !pool.dex_kind().is_v4() {
      return Err(anyhow::anyhow!("Pool is not v4"));
   }

   let state_view = utils::address::uniswap_v4_stateview(pool.chain_id())?;
   let pool_data = batch::V4Pool {
      pool: pool.pool_id(),
      tickSpacing: pool.fee().tick_spacing(),
   };

   let state = batch::get_v4_pool_state(client.clone(), vec![pool_data], state_view, block).await?;
   let state = state
      .get(0)
      .cloned()
      .ok_or_else(|| anyhow!("Pool data not found"))?;

   let pool_state = V3PoolState::v4(pool, state, block)?;

   Ok(State::v4(pool_state))
}

/// Update the state of all the pools for the given chain
///
/// Supports V2, V3 & V4 pools
///
/// Returns the pools with updated state
pub async fn batch_update_state<P, N>(
   client: P,
   chain_id: u64,
   concurrency: u8,
   mut pools: Vec<AnyUniswapPool>,
) -> Result<Vec<AnyUniswapPool>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   const BATCH_SIZE: usize = 20;
   const BATCH_SIZE_2: usize = 10;

   let v2_addresses: Vec<Address> = pools
      .iter()
      .filter(|p| p.dex_kind().is_v2() && p.chain_id() == chain_id)
      .map(|p| p.address())
      .collect();

   tracing::info!(target: "zeus_eth::amm::uniswap::state", "Batch request for {} V2 pools ChainId {}", v2_addresses.len(), chain_id);
   let v2_reserves = Arc::new(Mutex::new(Vec::new()));
   let mut v2_tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let semaphore = Arc::new(Semaphore::new(concurrency as usize));

   for chunk in v2_addresses.chunks(BATCH_SIZE) {
      let client = client.clone();
      let chunk_clone = chunk.to_vec();
      let semaphore = semaphore.clone();
      let v2_reserves = v2_reserves.clone();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire_owned().await.unwrap();
         match batch::get_v2_pool_reserves(client.clone(), None, chunk_clone.clone()).await {
            Ok(data) => {
               v2_reserves.lock().await.extend(data);
            }
            Err(e) => {
               tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v2 pool reserves: {:?}", e);
            }
         }
         Ok(())
      });
      v2_tasks.push(task);
   }

   let v3_data = Arc::new(Mutex::new(Vec::new()));
   let mut v3_tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let mut v3_pool_info = Vec::new();

   for pool in &pools {
      if pool.dex_kind().is_v3() && pool.chain_id() == chain_id {
         v3_pool_info.push(V3Pool {
            pool: pool.address(),
            token0: pool.currency0().address(),
            token1: pool.currency1().address(),
            tickSpacing: pool.fee().tick_spacing(),
         });
      }
   }

   tracing::info!(target: "zeus_eth::amm::uniswap::state", "Batch request for {} V3 pools ChainId {}", v3_pool_info.len(), chain_id);
   for pool in v3_pool_info.chunks(BATCH_SIZE_2) {
      let client = client.clone();
      let semaphore = semaphore.clone();
      let v3_data = v3_data.clone();
      let pool_chunk = pool.to_vec();

      let addr = pool_chunk.iter().map(|p| p.pool).collect::<Vec<_>>();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire_owned().await.unwrap();
         match batch::get_v3_state(client.clone(), None, pool_chunk).await {
            Ok(data) => {
               tracing::debug!(target: "zeus_eth::amm::uniswap::state", "Got V3 pool data for pools: {:?}", addr);
               v3_data.lock().await.extend(data);
            }
            Err(e) => {
               tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v3 pool data (ChainId {}): {:?}", chain_id, e);
            }
         }
         Ok(())
      });
      v3_tasks.push(task);
   }

   let v4_data = Arc::new(Mutex::new(Vec::new()));
   let mut v4_tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let mut v4_pool_info = Vec::new();

   for pool in &pools {
      if pool.dex_kind().is_v4() && pool.chain_id() == chain_id {
         v4_pool_info.push(batch::V4Pool {
            pool: pool.pool_id(),
            tickSpacing: pool.fee().tick_spacing(),
         });
      }
   }

   tracing::info!(target: "zeus_eth::amm::uniswap::state", "Batch request for {} V4 pools ChainId {}", v4_pool_info.len(), chain_id);
   let state_view = utils::address::uniswap_v4_stateview(chain_id)?;
   for pool in v4_pool_info.chunks(BATCH_SIZE_2) {
      let client = client.clone();
      let semaphore = semaphore.clone();
      let v4_data = v4_data.clone();
      let pool_chunk = pool.to_vec();

      let task = tokio::spawn(async move {
         let _permit = semaphore.acquire_owned().await.unwrap();
         match batch::get_v4_pool_state(client.clone(), pool_chunk, state_view, None).await {
            Ok(data) => {
               v4_data.lock().await.extend(data);
            }
            Err(e) => {
               tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v4 pool data: {:?}", e);
            }
         }
         Ok(())
      });
      v4_tasks.push(task);
   }

   for task in v2_tasks {
      if let Err(e) = task.await? {
         tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v2 pool reserves: {:?}", e);
      }
   }

   for task in v3_tasks {
      if let Err(e) = task.await? {
         tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v3 pool data: {:?}", e);
      }
   }

   for task in v4_tasks {
      if let Err(e) = task.await? {
         tracing::error!(target: "zeus_eth::amm::uniswap::state","Error fetching v4 pool data: {:?}", e);
      }
   }

   let v2_reserves = Arc::try_unwrap(v2_reserves).unwrap().into_inner();
   let v3_reserves = Arc::try_unwrap(v3_data).unwrap().into_inner();
   let v4_reserves = Arc::try_unwrap(v4_data).unwrap().into_inner();

   // update the state of the pools
   for pool in pools.iter_mut() {
      if pool.dex_kind().is_v2() && pool.chain_id() == chain_id {
         for data in &v2_reserves {
            if data.pool == pool.address() {
               pool.set_state(State::v2(data.clone().into()));
            }
         }
      }

      if pool.dex_kind().is_v3() && pool.chain_id() == chain_id {
         for data in &v3_reserves {
            if data.pool == pool.address() {
               let state = V3PoolState::new(data.clone(), pool.fee().tick_spacing(), None)?;
               pool.set_state(State::v3(state));
               pool.v3_mut(|pool| { 
                   pool.liquidity_amount0 = data.token0Balance;
                   pool.liquidity_amount1 = data.token1Balance;
               });
            }
         }
      }

      if pool.dex_kind().is_v4() && pool.chain_id() == chain_id {
         for data in &v4_reserves {
            if data.pool == pool.pool_id() {
               let state = V3PoolState::v4(pool, data.clone(), None)?;
               pool.set_state(State::v4(state));
               match pool.calculate_liquidity2() {
                  Ok(_) => {},
                  Err(e) => {
                     tracing::error!(target: "zeus_eth::amm::uniswap::state","Error calculating liquidity for pool: {:?}", e);
                  }
               }
            }
         }
      }
   }

   Ok(pools)
}
