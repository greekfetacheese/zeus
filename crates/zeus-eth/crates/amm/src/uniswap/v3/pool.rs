use alloy_primitives::{ Address, U256, utils::{ parse_units, format_units } };
use alloy_rpc_types::BlockId;

use alloy_contract::private::Network;
use alloy_provider::Provider;
use alloy_transport::Transport;

use crate::DexKind;
use abi::uniswap::v3;
use currency::erc20::ERC20Token;
use utils::{ batch_request, is_base_token, price_feed::get_base_token_price };

use std::collections::HashMap;

use serde::{ Deserialize, Serialize };
use anyhow::anyhow;

pub const FEE_TIERS: [u32; 4] = [100, 500, 3000, 10000];

/// Represents a Uniswap V3 Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV3Pool {
    pub chain_id: u64,
    pub address: Address,
    pub fee: u32,
    pub token0: ERC20Token,
    pub token1: ERC20Token,
    pub dex: DexKind,
    pub state: Option<V3PoolState>,
}

/// The state of a Uniswap V3 Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3PoolState {
    pub liquidity: u128,
    pub sqrt_price: U256,
    pub tick: i32,
    pub tick_spacing: i32,
    pub tick_bitmap: HashMap<i16, U256>,
    pub ticks: HashMap<i32, TickInfo>,
    pub pool_tick: PoolTick,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickInfo {
    pub liquidity_gross: u128,
    pub liquidity_net: i128,
    pub initialized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolTick {
    pub tick: i32,
    pub liquidity_net: i128,
    pub block: u64,
}

impl V3PoolState {
    pub fn new(
        pool_data: batch_request::V3PoolData,
        block: Option<BlockId>
    ) -> Result<Self, anyhow::Error> {
        let mut tick_bitmap_map = HashMap::new();
        tick_bitmap_map.insert(pool_data.wordPos, pool_data.tickBitmap);

        let ticks_info = TickInfo {
            liquidity_gross: pool_data.liquidityGross,
            liquidity_net: pool_data.liquidityNet,
            initialized: pool_data.initialized,
        };

        let block = if let Some(b) = block { b.as_u64().unwrap_or(0) } else { 0 };
        let tick: i32 = pool_data.tick.to_string().parse()?;

        let pool_tick = PoolTick {
            tick,
            liquidity_net: pool_data.liquidityNet,
            block,
        };

        let mut ticks_map = HashMap::new();
        ticks_map.insert(tick, ticks_info);

        let tick_spacing: i32 = pool_data.tickSpacing.to_string().parse()?;

        Ok(Self {
            liquidity: pool_data.liquidity,
            sqrt_price: U256::from(pool_data.sqrtPrice),
            tick,
            tick_spacing,
            tick_bitmap: tick_bitmap_map,
            ticks: ticks_map,
            pool_tick,
        })
    }
}

impl UniswapV3Pool {
    /// Create a new Uniswap V3 Pool
    ///
    /// Tokens are ordered by address
    pub fn new(
        chain_id: u64,
        address: Address,
        fee: u32,
        token0: ERC20Token,
        token1: ERC20Token,
        dex: DexKind
    ) -> Self {
        let (token0, token1) = if token0.address < token1.address {
            (token0, token1)
        } else {
            (token1, token0)
        };

        Self {
            chain_id,
            address,
            fee,
            token0,
            token1,
            dex,
            state: None,
        }
    }

    pub async fn from<T, P, N>(
        client: P,
        chain_id: u64,
        fee: u32,
        token0: ERC20Token,
        token1: ERC20Token,
        dex: DexKind
    )
        -> Result<Self, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let factory = dex.factory(chain_id)?;
        let address = v3::factory::get_pool(
            client,
            factory,
            token0.address,
            token1.address,
            fee
        ).await?;
        if address.is_zero() {
            anyhow::bail!("Pair not found");
        }
        Ok(Self::new(chain_id, address, fee, token0, token1, dex))
    }

    /// Switch the tokens in the pool
    pub fn toggle(&mut self) {
        std::mem::swap(&mut self.token0, &mut self.token1);
    }

    /// Restore the original order of the tokens
    pub fn reorder(&mut self) {
        if self.token0.address > self.token1.address {
            std::mem::swap(&mut self.token0, &mut self.token1);
        }
    }

    /// Return a reference to the state of this pool
    pub fn state(&self) -> Option<&V3PoolState> {
        self.state.as_ref()
    }

    /// Update the state for this pool
    pub fn update_state(&mut self, state: V3PoolState) {
        self.state = Some(state);
    }

    pub fn is_token0(&self, token: Address) -> bool {
        self.token0.address == token
    }

    pub fn is_token1(&self, token: Address) -> bool {
        self.token1.address == token
    }

    /// Fetch the state of the pool at a given block
    /// If block is None, the latest block is used
    pub async fn fetch_state<T, P, N>(
        client: P,
        pool: Address,
        block: Option<BlockId>
    )
        -> Result<V3PoolState, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let pool_data = batch_request::get_v3_state(client.clone(), block, vec![pool]).await?;
        let data = pool_data
            .get(0)
            .cloned()
            .ok_or_else(|| anyhow!("Pool data not found"))?;

        let state = V3PoolState::new(data, block)?;
        Ok(state)
    }

    pub fn simulate_swap(&self, token_in: Address, amount_in: U256) -> Result<U256, anyhow::Error> {
        let (amount_out, _) = super::calculate_swap(&self, token_in, amount_in)?;
        Ok(amount_out)
    }

    pub fn simulate_swap_mut(
        &mut self,
        token_in: Address,
        amount_in: U256
    ) -> Result<U256, anyhow::Error> {
        let (amount_out, current_state) = super::calculate_swap(&self, token_in, amount_in)?;

        // update the state of the pool
        let mut state = self.state().ok_or(anyhow!("State not initialized"))?.clone();
        state.liquidity = current_state.liquidity;
        state.sqrt_price = current_state.sqrt_price_x_96;
        state.tick = current_state.tick;

        self.update_state(state);

        Ok(amount_out)
    }

    /// Calculate the price of token in terms of quote token
    pub fn calculate_price(&self, base_token: Address) -> Result<f64, anyhow::Error> {
        let price = super::calculate_price(&self, base_token)?;
        Ok(price)
    }

    /// Token0 USD price but we need to know the usd price of token1
    pub fn token0_price(&self, token1_price: f64) -> Result<f64, anyhow::Error> {
        if token1_price == 0.0 {
            return Ok(0.0);
        }
        let unit = parse_units("1", self.token1.decimals)?.get_absolute();
        let amount_out = self.simulate_swap(self.token1.address, unit)?;
        let amount_out = format_units(amount_out, self.token1.decimals)?.parse::<f64>()?;
        Ok(token1_price / amount_out)
    }

    /// Token1 USD price but we need to know the usd price of token0
    pub fn token1_price(&self, token0_price: f64) -> Result<f64, anyhow::Error> {
        if token0_price == 0.0 {
            return Ok(0.0);
        }
        let unit = parse_units("1", self.token0.decimals)?.get_absolute();
        let amount_out = self.simulate_swap(self.token0.address, unit)?;
        let amount_out = format_units(amount_out, self.token0.decimals)?.parse::<f64>()?;
        Ok(token0_price / amount_out)
    }

    /// Get the usd values of token0 and token1 at a given block
    /// If block is None, the latest block is used
    pub async fn tokens_usd<T, P, N>(
        &self,
        client: P,
        block: Option<BlockId>
    )
        -> Result<(f64, f64), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let chain = self.chain_id;
        if is_base_token(chain, self.token0.address) {
            let price0 = get_base_token_price(
                client.clone(),
                chain,
                self.token0.address,
                block
            ).await?;
            let price1 = self.token1_price(price0)?;
            Ok((price0, price1))
        } else if is_base_token(chain, self.token1.address) {
            let price1 = get_base_token_price(
                client.clone(),
                chain,
                self.token1.address,
                block
            ).await?;
            let price0 = self.token0_price(price1)?;
            Ok((price0, price1))
        } else {
            anyhow::bail!("Could not determine a common paired token");
        }
    }
}
