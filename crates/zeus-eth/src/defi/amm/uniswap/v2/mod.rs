use std::any;

use alloy_primitives::utils::{ format_units, parse_units };
use alloy_primitives::{ Address, U256 };
use alloy_rpc_types::BlockId;

use alloy_contract::private::Network;
use alloy_provider::Provider;
use alloy_transport::Transport;

use serde::{ Deserialize, Serialize };

use crate::abi::uniswap::{ pool::v2, factory };
use crate::defi::currency::erc20::ERC20Token;
use crate::defi::utils::chain_link::get_token_price;

use crate::defi::amm::{ DexKind, consts::* };
use crate::defi::utils::is_base_token;

/// Represents a Uniswap V2 Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV2Pool {
    pub chain_id: u64,
    pub address: Address,
    pub token0: ERC20Token,
    pub token1: ERC20Token,
    pub dex: DexKind,
    pub state: Option<V2State>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V2State {
    pub reserve0: U256,
    pub reserve1: U256,
    pub block: u64,
}

impl V2State {
    pub fn new(reserve0: U256, reserve1: U256, block: u64) -> Self {
        Self {
            reserve0,
            reserve1,
            block,
        }
    }
}

impl UniswapV2Pool {
    pub fn new(chain_id: u64, address: Address, token0: ERC20Token, token1: ERC20Token, dex: DexKind) -> Self {
        // reorder tokens
        let (token0, token1) = if token0.address < token1.address { (token0, token1) } else { (token1, token0) };

        Self {
            chain_id,
            address,
            token0,
            token1,
            dex,
            state: None,
        }
    }

    /// Create a new Uniswap V2 Pool from token0, token1 and the DEX
    pub async fn from<T, P, N>(
        client: P,
        chain_id: u64,
        token0: ERC20Token,
        token1: ERC20Token,
        dex: DexKind
    )
        -> Result<Self, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let factory = dex.factory(chain_id)?;
        let address = factory::v2::get_pair(client, factory, token0.address, token1.address).await?;
        if address.is_zero() {
            anyhow::bail!("Pair not found");
        }
        Ok(Self::new(chain_id, address, token0, token1, dex))
    }

    /// Serialize the pool to a json string
    pub fn to_string(&self) -> Result<String, anyhow::Error> {
        Ok(serde_json::to_string(self)?)
    }

    /// Deserialize the pool from a json string
    pub fn from_string(data: &str) -> Result<Self, anyhow::Error> {
        Ok(serde_json::from_str(data)?)
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
    pub fn state(&self) -> Option<&V2State> {
        self.state.as_ref()
    }

    /// Update the state for this pool
    pub fn update_state(&mut self, state: V2State) {
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
        -> Result<V2State, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let reserves = v2::get_reserves(pool, client, block).await?;
        let reserve0 = U256::from(reserves.0);
        let reserve1 = U256::from(reserves.1);

        Ok(V2State {
            reserve0,
            reserve1,
            block: reserves.2 as u64,
        })
    }

    pub fn simulate_swap(&self, token_in: Address, amount_in: U256) -> Result<U256, anyhow::Error> {
        let state = self.state.as_ref().ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

        if self.token0.address == token_in {
            Ok(self.get_amount_out(amount_in, state.reserve0, state.reserve1))
        } else {
            Ok(self.get_amount_out(amount_in, state.reserve1, state.reserve0))
        }
    }

    pub fn simulate_swap_mut(&mut self, token_in: Address, amount_in: U256) -> Result<U256, anyhow::Error> {
        let mut state = self.state.clone().ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

        if self.token0.address == token_in {
            let amount_out = self.get_amount_out(amount_in, state.reserve0, state.reserve1);

            state.reserve0 += amount_in;
            state.reserve1 -= amount_out;
            self.state = Some(state);

            Ok(amount_out)
        } else {
            let amount_out = self.get_amount_out(amount_in, state.reserve1, state.reserve0);

            state.reserve0 -= amount_out;
            state.reserve1 += amount_in;
            self.state = Some(state);

            Ok(amount_out)
        }
    }

    /// Calculates the amount received for a given `amount_in` `reserve_in` and `reserve_out`.
    pub fn get_amount_out(&self, amount_in: U256, reserve_in: U256, reserve_out: U256) -> U256 {
        if amount_in.is_zero() || reserve_in.is_zero() || reserve_out.is_zero() {
            return U256::ZERO;
        }
        let fee = (10000 - 300 / 10) / 10; //Fee of 300 => (10,000 - 30) / 10  = 997
        let amount_in_with_fee = amount_in * U256::from(fee);
        let numerator = amount_in_with_fee * reserve_out;
        let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;

        numerator / denominator
    }

    /// Calculates the price of the base token in terms of the quote token.
    ///
    /// Returned as a Q64 fixed point number.
    pub fn calculate_price_64_x_64(&self, base_token: Address) -> Result<u128, anyhow::Error> {
        let state = self.state.as_ref().ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

        let decimal_shift = (self.token0.decimals as i8) - (self.token1.decimals as i8);

        let (r_0, r_1) = if decimal_shift < 0 {
            (
                U256::from(state.reserve0) * U256::from((10u128).pow(decimal_shift.unsigned_abs() as u32)),
                U256::from(state.reserve1),
            )
        } else {
            (U256::from(state.reserve0), U256::from(state.reserve1) * U256::from((10u128).pow(decimal_shift as u32)))
        };

        if base_token == self.token0.address {
            if r_0.is_zero() { Ok(U128_0X10000000000000000) } else { div_uu(r_1, r_0) }
        } else if r_1.is_zero() {
            Ok(U128_0X10000000000000000)
        } else {
            div_uu(r_0, r_1)
        }
    }

    /// Token0 USD price but we need to know the usd price of token1
    pub fn token0_price(&self, token1_price: f64) -> Result<f64, anyhow::Error> {
        let unit = parse_units("1", self.token1.decimals)?.get_absolute();
        let amount_out = self.simulate_swap(self.token1.address, unit)?;
        let amount_out = format_units(amount_out, self.token1.decimals)?.parse::<f64>()?;
        Ok(token1_price / amount_out)
    }

    /// Token1 USD price but we need to know the usd price of token0
    pub fn token1_price(&self, token0_price: f64) -> Result<f64, anyhow::Error> {
        let unit = parse_units("1", self.token0.decimals)?.get_absolute();
        let amount_out = self.simulate_swap(self.token0.address, unit)?;
        let amount_out = format_units(amount_out, self.token0.decimals)?.parse::<f64>()?;
        Ok(token0_price / amount_out)
    }

    /// Get the usd value of token0 and token1 at a given block
    /// If block is None, the latest block is used
    pub async fn tokens_usd<T, P, N>(&self, client: P, block: Option<BlockId>) -> Result<(f64, f64), anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        // token0 is known
        if is_base_token(&self.token0) {
            let price0 = get_token_price(client.clone(), block, self.chain_id, self.token0.address).await?;

            // 1 unit of token0
            let unit = parse_units("1", self.token0.decimals)?.get_absolute();

            // amount of token1 received for 1 unit of token0
            let amount_out = self.simulate_swap(self.token0.address, unit)?;
            let amount_out = format_units(amount_out, self.token1.decimals)?;

            // price of token1 in usd
            let price1 = price0 / amount_out.parse::<f64>()?;

            Ok((price0, price1))
        } else if is_base_token(&self.token1) {
            let price1 = get_token_price(client.clone(), block, self.chain_id, self.token1.address).await?;

            // 1 unit of token1
            let unit = parse_units("1", self.token1.decimals)?.get_absolute();

            // amount of token0 received for 1 unit of token1
            let amount_out = self.simulate_swap(self.token1.address, unit)?;
            let amount_out = format_units(amount_out, self.token0.decimals)?;

            // price of token0 in usd
            let price0 = price1 / amount_out.parse::<f64>()?;

            Ok((price0, price1))
        } else {
            anyhow::bail!("Could not determine a common paired token");
        }
    }
}

pub fn div_uu(x: U256, y: U256) -> Result<u128, anyhow::Error> {
    if !y.is_zero() {
        let mut answer;

        if x <= U256_0XFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF {
            answer = (x << U256_64) / y;
        } else {
            let mut msb = U256_192;
            let mut xc = x >> U256_192;

            if xc >= U256_0X100000000 {
                xc >>= U256_32;
                msb += U256_32;
            }

            if xc >= U256_0X10000 {
                xc >>= U256_16;
                msb += U256_16;
            }

            if xc >= U256_0X100 {
                xc >>= U256_8;
                msb += U256_8;
            }

            if xc >= U256_16 {
                xc >>= U256_4;
                msb += U256_4;
            }

            if xc >= U256_4 {
                xc >>= U256_2;
                msb += U256_2;
            }

            if xc >= U256_2 {
                msb += U256_1;
            }

            answer = (x << (U256_255 - msb)) / (((y - U256_1) >> (msb - U256_191)) + U256_1);
        }

        if answer > U256_0XFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF {
            return Ok(0);
        }

        let hi = answer * (y >> U256_128);
        let mut lo = answer * (y & U256_0XFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF);

        let mut xh = x >> U256_192;
        let mut xl = x << U256_64;

        if xl < lo {
            xh -= U256_1;
        }

        xl = xl.overflowing_sub(lo).0;
        lo = hi << U256_128;

        if xl < lo {
            xh -= U256_1;
        }

        xl = xl.overflowing_sub(lo).0;

        if xh != (hi >> U256_128) {
            return Err(anyhow::anyhow!("Rounding Error"));
        }

        answer += xl / y;

        if answer > U256_0XFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF {
            return Ok(0_u128);
        }

        Ok(answer.to::<u128>())
    } else {
        Err(anyhow::anyhow!("Y is zero"))
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::{ ERC20Token, weth };
    use alloy_primitives::{ address, utils::{ parse_units, format_units } };
    use alloy_provider::{ ProviderBuilder, WsConnect };
    use super::*;

    #[tokio::test]
    async fn uniswap_v2_pool_test() {
        let url = "wss://eth.merkle.io";
        let ws_connect = WsConnect::new(url);
        let client = ProviderBuilder::new().on_ws(ws_connect).await.unwrap();

        let weth = ERC20Token::new(client.clone(), weth(1).unwrap(), 1).await.unwrap();
        let uni_addr = address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984");
        let uni = ERC20Token::new(client.clone(), uni_addr, 1).await.unwrap();

        let pool_address = address!("d3d2E2692501A5c9Ca623199D38826e513033a17");
        let mut pool = UniswapV2Pool::new(1, pool_address, weth.clone(), uni.clone(), DexKind::UniswapV2);

        let state = UniswapV2Pool::fetch_state(client.clone(), pool_address, None).await.unwrap();
        pool.update_state(state);

        let amount_in = parse_units("1", weth.decimals).unwrap().get_absolute();
        let amount_out = pool.simulate_swap(weth.address, amount_in).unwrap();

        let amount_in = format_units(amount_in, weth.decimals).unwrap();
        let amount_out = format_units(amount_out, uni.decimals).unwrap();

        println!("=== V2 Swap Test ===");
        println!("Swapped {} {} For {} {}", amount_in, weth.symbol, amount_out, uni.symbol);
        println!("=== Tokens Price Test ===");

        let (token0_usd, token1_usd) = pool.tokens_usd(client.clone(), None).await.unwrap();
        println!("{} Price: ${}", pool.token0.symbol, token0_usd);
        println!("{} Price: ${}", pool.token1.symbol, token1_usd);

        let token0_price = pool.token0_price(token1_usd).unwrap();
        println!("{} Price: ${}", pool.token0.symbol, token0_price);

        assert_eq!(pool.token0.address, uni.address);
        assert_eq!(pool.token1.address, weth.address);

        pool.toggle();
        assert_eq!(pool.token0.address, weth.address);
        assert_eq!(pool.token1.address, uni.address);

        pool.reorder();
        assert_eq!(pool.token0.address, uni.address);
        assert_eq!(pool.token1.address, weth.address);

        let pool_str = pool.to_string().expect("Failed to serialize pool");
        let _pool = UniswapV2Pool::from_string(&pool_str).expect("Failed to deserialize pool");
    }
}
