use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, B256, U256, address,
   utils::{format_units, parse_units},
};
use alloy_rpc_types::BlockId;
use std::borrow::Cow;

use crate::uniswap::PoolKey;
use crate::{
   DexKind, minimum_liquidity,
   uniswap::{State, UniswapPool, v4::FeeAmount},
};
use abi::uniswap::v3;
use currency::{Currency, ERC20Token};
use utils::{
   batch_request::{self, V3Pool2},
   is_base_token,
   price_feed::get_base_token_price,
};

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const FEE_TIERS: [u32; 4] = [100, 500, 3000, 10000];

/// Represents a Uniswap V3 Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV3Pool {
   pub chain_id: u64,
   pub address: Address,
   pub fee: FeeAmount,
   pub currency0: Currency,
   pub currency1: Currency,
   pub dex: DexKind,
   pub state: State,
}

/// The state of a Uniswap V3 Pool
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct V3PoolState {
   pub base_token_liquidity: U256,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolTick {
   pub tick: i32,
   pub liquidity_net: i128,
   pub block: u64,
}

impl V3PoolState {
   pub fn new(pool_data: batch_request::V3PoolData, block: Option<BlockId>) -> Result<Self, anyhow::Error> {
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

      let tick_spacing: i32 = pool_data.tickSpacing.to_string().parse()?;

      Ok(Self {
         base_token_liquidity: pool_data.base_token_liquidity,
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
   pub fn new(chain_id: u64, address: Address, fee: u32, token0: ERC20Token, token1: ERC20Token, dex: DexKind) -> Self {
      let (token0, token1) = if token0.address < token1.address {
         (token0, token1)
      } else {
         (token1, token0)
      };

      let currency0 = Currency::from(token0);
      let currency1 = Currency::from(token1);

      Self {
         chain_id,
         address,
         fee: FeeAmount::CUSTOM(fee),
         currency0,
         currency1,
         dex,
         state: State::none(),
      }
   }

   pub async fn from_address<P, N>(client: P, chain_id: u64, address: Address) -> Result<Self, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let dex_kind = DexKind::UniswapV3;
      let token0 = v3::pool::token0(address, client.clone()).await?;
      let token1 = v3::pool::token1(address, client.clone()).await?;
      let fee = v3::pool::fee(address, client.clone()).await?;

      let erc_token0 = if let Some(token) = ERC20Token::base_token(chain_id, token0) {
         token
      } else {
         ERC20Token::new(client.clone(), token0, chain_id).await?
      };

      let erc_token1 = if let Some(token) = ERC20Token::base_token(chain_id, token1) {
         token
      } else {
         ERC20Token::new(client.clone(), token1, chain_id).await?
      };

      Ok(Self::new(
         chain_id, address, fee, erc_token0, erc_token1, dex_kind,
      ))
   }

   pub async fn from<P, N>(
      client: P,
      chain_id: u64,
      fee: u32,
      token0: ERC20Token,
      token1: ERC20Token,
      dex: DexKind,
   ) -> Result<Self, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let factory = dex.factory(chain_id)?;
      let address = v3::factory::get_pool(client, factory, token0.address, token1.address, fee).await?;
      if address.is_zero() {
         anyhow::bail!("Pair not found");
      }
      Ok(Self::new(chain_id, address, fee, token0, token1, dex))
   }

   /// Switch the tokens in the pool
   pub fn toggle(&mut self) {
      std::mem::swap(&mut self.currency0, &mut self.currency1);
   }

   /// Restore the original order of the tokens
   pub fn reorder(&mut self) {
      if self.token0().address > self.token1().address {
         std::mem::swap(&mut self.currency0, &mut self.currency1);
      }
   }

   pub fn token0(&self) -> Cow<ERC20Token> {
      self.currency0.to_erc20()
   }

   pub fn token1(&self) -> Cow<ERC20Token> {
      self.currency1.to_erc20()
   }

   pub fn base_token(&self) -> Cow<ERC20Token> {
      self.base_currency().to_erc20()
   }

   pub fn quote_token(&self) -> Cow<ERC20Token> {
      self.quote_currency().to_erc20()
   }

   pub fn calculate_price(&self, token_in: Address) -> Result<f64, anyhow::Error> {
      let zero_for_one = self.zero_for_one_v3(token_in);
      let price = super::calculate_price(self, zero_for_one)?;
      Ok(price)
   }

   /// Test pool
   pub fn usdt_uni() -> Self {
      let usdt = ERC20Token::usdt();
      let uni_addr = address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984");
      let uni = ERC20Token {
         chain_id: 1,
         address: uni_addr,
         decimals: 18,
         symbol: "UNI".to_string(),
         name: "Uniswap Token".to_string(),
         total_supply: U256::ZERO,
      };

      let pool_address = address!("3470447f3CecfFAc709D3e783A307790b0208d60");
      UniswapV3Pool::new(1, pool_address, 3000, usdt, uni, DexKind::UniswapV3)
   }

   pub fn weth_usdc() -> Self {
      let pool_address = address!("0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640");
      UniswapV3Pool::new(
         1,
         pool_address,
         500,
         ERC20Token::weth(),
         ERC20Token::usdc(),
         DexKind::UniswapV3,
      )
   }
}

impl UniswapPool for UniswapV3Pool {
   fn chain_id(&self) -> u64 {
      self.chain_id
   }

   fn address(&self) -> Address {
      self.address
   }

   fn fee(&self) -> FeeAmount {
      self.fee
   }

   fn pool_id(&self) -> B256 {
      B256::ZERO
   }

   fn dex_kind(&self) -> DexKind {
      self.dex
   }

   fn zero_for_one_v3(&self, token_in: Address) -> bool {
      token_in == self.token0().address
   }

   fn zero_for_one_v4(&self, _currency_in: &Currency) -> bool {
      panic!("You should call zero_for_one_v3 instead");
   }

   fn is_token0(&self, token: Address) -> bool {
      self.token0().address == token
   }

   fn is_token1(&self, token: Address) -> bool {
      self.token1().address == token
   }

   fn currency0(&self) -> &Currency {
      &self.currency0
   }

   fn currency1(&self) -> &Currency {
      &self.currency1
   }

   fn is_currency0(&self, currency: &Currency) -> bool {
      &self.currency0 == currency
   }

   fn is_currency1(&self, currency: &Currency) -> bool {
      &self.currency1 == currency
   }

   fn state(&self) -> &State {
      &self.state
   }

   fn set_state(&mut self, state: State) {
      self.state = state;
   }

   fn set_state_res(&mut self, state: State) -> Result<(), anyhow::Error> {
      if state.is_v3() {
         self.state = state;
         Ok(())
      } else {
         Err(anyhow::anyhow!("Pool state is not for v3"))
      }
   }

   fn enough_liquidity(&self) -> bool {
      let threshold = minimum_liquidity(&self.base_token());
      if !self.state.is_v3() {
         return true;
      } else {
         return self.state.v3_state().unwrap().base_token_liquidity >= threshold;
      }
   }

   fn base_token_exists(&self) -> bool {
      if is_base_token(self.chain_id, self.token0().address) {
         true
      } else if is_base_token(self.chain_id, self.token1().address) {
         true
      } else {
         false
      }
   }

   fn base_currency(&self) -> &Currency {
      // If currency0 is native (e.g., ETH) or a base token, use it as the base.
      // Otherwise, use currency1.
      if self.currency0.is_native() || is_base_token(self.chain_id, self.currency0.to_erc20().address) {
         &self.currency0
      } else {
         &self.currency1
      }
   }

   fn quote_currency(&self) -> &Currency {
      // Return the opposite currency of base_currency.
      if self.currency0.is_native() || is_base_token(self.chain_id, self.currency0.to_erc20().address) {
         &self.currency1
      } else {
         &self.currency0
      }
   }

   fn get_pool_key(&self) -> Result<PoolKey, anyhow::Error> {
      bail!("Pool Key method only applies to V4");
   }

   /// Fetch the state of the pool at a given block
   /// If block is None, the latest block is used
   async fn fetch_state<P, N>(client: P, pool: impl UniswapPool, block: Option<BlockId>) -> Result<State, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let address = pool.address();
      let base_token = pool.base_currency().to_erc20().address;
      let pool2 = V3Pool2 {
         pool: address,
         base_token,
      };

      let pool_data = batch_request::get_v3_state(client.clone(), block, vec![pool2]).await?;
      let data = pool_data
         .get(0)
         .cloned()
         .ok_or_else(|| anyhow!("Pool data not found"))?;

      let v3_pool_state = V3PoolState::new(data, block)?;
      Ok(State::v3(v3_pool_state))
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let zero_for_one = self.zero_for_one_v3(currency_in.to_erc20().address);
      let (amount_out, _) = super::calculate_swap(self, zero_for_one, amount_in)?;
      Ok(amount_out)
   }

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let zero_for_one = self.zero_for_one_v3(currency_in.to_erc20().address);
      let (amount_out, current_state) = super::calculate_swap(self, zero_for_one, amount_in)?;

      // update the state of the pool
      let mut state = self
         .state()
         .v3_state()
         .ok_or(anyhow!("State not initialized"))?
         .clone();
      state.liquidity = current_state.liquidity;
      state.sqrt_price = current_state.sqrt_price_x_96;
      state.tick = current_state.tick;

      self.set_state(State::v3(state));

      Ok(amount_out)
   }

   fn quote_price(&self, base_usd: f64) -> Result<f64, anyhow::Error> {
      if base_usd == 0.0 {
         return Ok(0.0);
      }

      if !self.base_token_exists() {
         bail!("Base token not found in the pool");
      }

      let base = self.base_currency();
      let unit = parse_units("1", base.decimals())?.get_absolute();
      let amount_out = self.simulate_swap(base, unit)?;
      if amount_out == U256::ZERO {
         return Ok(0.0);
      }

      let amount_out = format_units(amount_out, self.quote_token().decimals)?.parse::<f64>()?;
      let price = base_usd / amount_out;

      Ok(price)
   }

   /// Get the usd value of Base and Quote token at a given block
   /// If block is None, the latest block is used
   ///
   /// ## Returns
   ///
   /// - (base_price, quote_price)
   async fn tokens_price<P, N>(&self, client: P, block: Option<BlockId>) -> Result<(f64, f64), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let chain_id = self.chain_id;

      if !self.base_token_exists() {
         bail!("Base token not found in the pool");
      }

      let base_price = get_base_token_price(client.clone(), chain_id, self.base_token().address, block).await?;
      let quote_price = self.quote_price(base_price)?;
      Ok((base_price, quote_price))
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::utils::{format_units, parse_units};

   use alloy_provider::ProviderBuilder;
   use url::Url;

   #[tokio::test]
   async fn can_swap() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let mut pool = UniswapV3Pool::usdt_uni();

      let state = UniswapV3Pool::fetch_state(client.clone(), pool.clone(), None)
         .await
         .unwrap();
      pool.set_state(state);

      // Swap 1 USDT for UNI
      let base = pool.base_currency();
      let quote = pool.quote_currency();

      let amount_in = parse_units("1", base.decimals()).unwrap().get_absolute();
      let amount_out = pool.simulate_swap(base, amount_in).unwrap();

      let amount_in = format_units(amount_in, base.decimals()).unwrap();
      let amount_out = format_units(amount_out, quote.decimals()).unwrap();

      println!("=== V3 Swap Test ===");
      println!(
         "Swapped {} {} For {} {}",
         amount_in,
         base.symbol(),
         amount_out,
         quote.symbol()
      );
   }

   #[test]
   fn pool_order() {
      let mut pool = UniswapV3Pool::usdt_uni();

      // UNI is token0 and USDT is token1
      let token0 = pool.is_token0(pool.quote_token().address);
      let token1 = pool.is_token1(pool.base_token().address);
      assert_eq!(token0, true);
      assert_eq!(token1, true);

      pool.toggle();
      // Now USDT is token0 and UNI is token1
      let token0 = pool.is_token0(pool.base_token().address);
      let token1 = pool.is_token1(pool.quote_token().address);
      assert_eq!(token0, true);
      assert_eq!(token1, true);

      pool.reorder();
      // Back to the original order
      let token0 = pool.is_token0(pool.quote_token().address);
      let token1 = pool.is_token1(pool.base_token().address);
      assert_eq!(token0, true);
      assert_eq!(token1, true);

      let base_exists = pool.base_token_exists();
      assert_eq!(base_exists, true);
   }

   #[tokio::test]
   async fn price_calculation() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let mut pool = UniswapV3Pool::usdt_uni();

      let state = UniswapV3Pool::fetch_state(client.clone(), pool.clone(), None)
         .await
         .unwrap();
      pool.set_state(state);

      let (base_price, quote_price) = pool.tokens_price(client.clone(), None).await.unwrap();
      let base_token = pool.base_token();
      let quote_token = pool.quote_token();

      // UNI in terms of USDT
      // let uni_in_usdt = pool.calculate_price(quote_token.address).unwrap();
      // let usdt_in_uni = pool.calculate_price(base_token.address).unwrap();

      println!("{} Price: ${}", base_token.symbol, base_price);
      println!("{} Price: ${}", quote_token.symbol, quote_price);
      // println!("UNI in terms of USDT: {}", uni_in_usdt);
      // println!("USDT in terms of UNI: {}", usdt_in_uni);
   }
}
