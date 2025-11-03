use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, B256, U256, address,
   utils::{format_units, parse_units},
};
use alloy_rpc_types::BlockId;
use std::borrow::Cow;

use crate::amm::uniswap::{
   AnyUniswapPool, DexKind, FeeAmount, State, SwapResult, UniswapPool, state::get_v3_pool_state,
};

use crate::abi::uniswap::{v4::PoolKey, v3};
use crate::currency::{Currency, ERC20Token};
use crate::utils::{NumericValue, price_feed::get_base_token_price};

use anyhow::bail;
use core::cmp::Ordering;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// Represents a Uniswap V3 Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV3Pool {
   pub chain_id: u64,
   pub address: Address,
   pub fee: FeeAmount,
   pub currency0: Currency,
   pub currency1: Currency,
   pub dex: DexKind,
   #[serde(skip)]
   pub state: State,
   pub liquidity_amount0: U256,
   pub liquidity_amount1: U256,
}

impl Ord for UniswapV3Pool {
   fn cmp(&self, other: &Self) -> Ordering {
      self.address.cmp(&other.address)
   }
}

impl PartialOrd for UniswapV3Pool {
   fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
      Some(self.cmp(other))
   }
}

impl Eq for UniswapV3Pool {}

impl PartialEq for UniswapV3Pool {
   fn eq(&self, other: &Self) -> bool {
      self.address == other.address && self.chain_id == other.chain_id
   }
}

impl Hash for UniswapV3Pool {
   fn hash<H: Hasher>(&self, state: &mut H) {
      self.chain_id.hash(state);
      self.currency0.hash(state);
      self.currency1.hash(state);
      self.fee.hash(state);
      self.dex.hash(state);
   }
}

impl TryFrom<AnyUniswapPool> for UniswapV3Pool {
   type Error = anyhow::Error;

   fn try_from(pool: AnyUniswapPool) -> Result<Self, Self::Error> {
      match pool {
         AnyUniswapPool::V3(pool) => Ok(pool),
         _ => Err(anyhow::anyhow!("Not a V3 Pool")),
      }
   }
}

impl UniswapV3Pool {
   /// Create a new Uniswap V3 Pool
   ///
   /// Tokens are ordered as token0 < token1
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
         liquidity_amount0: U256::ZERO,
         liquidity_amount1: U256::ZERO,
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

   pub async fn from_components<P, N>(
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

   pub fn token0(&self) -> Cow<'_, ERC20Token> {
      self.currency0.to_erc20()
   }

   pub fn token1(&self) -> Cow<'_, ERC20Token> {
      self.currency1.to_erc20()
   }

   pub fn base_token(&self) -> Cow<'_, ERC20Token> {
      self.base_currency().to_erc20()
   }

   pub fn quote_token(&self) -> Cow<'_, ERC20Token> {
      self.quote_currency().to_erc20()
   }

   pub fn state_mut(&mut self) -> &mut State {
      &mut self.state
   }

   pub fn clear_state(&mut self) {
      self.state = State::None;
   }

   pub fn tick_to_word(&self, tick: i32, tick_spacing: i32) -> i32 {
      let mut compressed = tick / tick_spacing;
      if tick < 0 && tick % tick_spacing != 0 {
         compressed -= 1;
      }

      compressed >> 8
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

   fn id(&self) -> B256 {
      B256::ZERO
   }

   fn key(&self) -> PoolKey {
      PoolKey::default()
   }

   fn dex_kind(&self) -> DexKind {
      self.dex
   }

   fn hooks(&self) -> Address {
      Address::ZERO
   }

   fn zero_for_one(&self, currency_in: &Currency) -> bool {
      currency_in.address() == self.currency0().address()
   }

   fn have(&self, currency: &Currency) -> bool {
      self.is_currency0(currency) || self.is_currency1(currency)
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

   fn base_currency_exists(&self) -> bool {
      self.currency0().is_base() || self.currency1().is_base()
   }

   fn base_currency(&self) -> &Currency {
      if self.currency0.is_base() {
         &self.currency0
      } else {
         &self.currency1
      }
   }

   fn quote_currency(&self) -> &Currency {
      if self.currency0.is_base() {
         &self.currency1
      } else {
         &self.currency0
      }
   }

   fn pool_balances(&self) -> (NumericValue, NumericValue) {
      let amount0 = NumericValue::format_wei(self.liquidity_amount0, self.currency0().decimals());
      let amount1 = NumericValue::format_wei(self.liquidity_amount1, self.currency1().decimals());
      (amount0, amount1)
   }

   fn base_balance(&self) -> NumericValue {
      if self.currency0().is_base() {
         NumericValue::format_wei(self.liquidity_amount0, self.currency0().decimals())
      } else {
         NumericValue::format_wei(self.liquidity_amount1, self.currency1().decimals())
      }
   }

   fn quote_balance(&self) -> NumericValue {
      if self.currency0().is_base() {
         NumericValue::format_wei(self.liquidity_amount1, self.currency1().decimals())
      } else {
         NumericValue::format_wei(self.liquidity_amount0, self.currency0().decimals())
      }
   }

   fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error> {
      let zero_for_one = self.zero_for_one(currency_in);
      let price = super::calculate_price(self, zero_for_one)?;
      Ok(price)
   }

   fn compute_virtual_reserves(&mut self) -> Result<(), anyhow::Error> {
      Ok(())
   }

   async fn update_state<P, N>(&mut self, client: P, block: Option<BlockId>) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let (state, data) = get_v3_pool_state(client, self, block).await?;
      self.liquidity_amount0 = data.tokenABalance;
      self.liquidity_amount1 = data.tokenBBalance;
      self.set_state(state);
      Ok(())
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let zero_for_one = self.zero_for_one(currency_in);
      let fee = self.fee.fee();
      let state = self
         .state()
         .v3_state()
         .ok_or(anyhow::anyhow!("State not initialized"))?;
      let amount_out = super::calculate_swap(state, fee, zero_for_one, amount_in)?;
      Ok(amount_out)
   }

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let zero_for_one = self.zero_for_one(currency_in);
      let fee = self.fee.fee();
      let state = self
         .state_mut()
         .v3_state_mut()
         .ok_or(anyhow::anyhow!("State not initialized"))?;
      let amount_out = super::calculate_swap_mut(state, fee, zero_for_one, amount_in)?;

      Ok(amount_out)
   }

   fn simulate_swap_result(
      &self,
      currency_in: &Currency,
      currency_out: &Currency,
      amount_in: NumericValue,
   ) -> Result<SwapResult, anyhow::Error> {
      let amount_out = self.simulate_swap(currency_in, amount_in.wei())?;
      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());
      let spot_price = self.calculate_price(currency_in)?;

      let fee_fraction = self.fee().fee_percent() as f64 / 100.0;
      let amount_in_after_fee = amount_in.f64() * (1.0 - fee_fraction);
      let ideal_amount_out = amount_in_after_fee * spot_price;

      let price_impact = (1.0 - (amount_out.f64() / ideal_amount_out)) * 100.0;

      Ok(SwapResult {
         amount_in,
         amount_out,
         ideal_amount_out: NumericValue::parse_to_wei(&ideal_amount_out.to_string(), currency_out.decimals()),
         price_impact,
      })
   }

   fn quote_price(&self, base_usd: f64) -> Result<f64, anyhow::Error> {
      if base_usd == 0.0 {
         return Ok(0.0);
      }

      if !self.base_currency_exists() {
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

      if !self.base_currency_exists() {
         bail!("Base token not found in the pool");
      }

      let base_price = get_base_token_price(client.clone(), chain_id, self.base_token().address, block).await?;
      let quote_price = self.quote_price(base_price)?;
      Ok((base_price, quote_price))
   }
}

// Well known pools for quick testing

impl UniswapV3Pool {
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

   pub fn weth_usdc_base() -> Self {
      let pool_address = address!("0xd0b53D9277642d899DF5C87A3966A349A798F224");
      UniswapV3Pool::new(
         8453,
         pool_address,
         500,
         ERC20Token::weth_base(),
         ERC20Token::usdc_base(),
         DexKind::UniswapV3,
      )
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_provider::ProviderBuilder;
   use url::Url;

   #[tokio::test]
   async fn swap_result() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV3Pool::usdt_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      // Swap 1 USDT for UNI
      let base = pool.base_currency();
      let quote = pool.quote_currency();

      let amount_in = NumericValue::parse_to_wei("100000", base.decimals());
      let swap_result = pool
         .simulate_swap_result(base, quote, amount_in.clone())
         .unwrap();

      println!("=== V3 Swap Test ===");
      println!(
         "Ideal Output: {:.6} {}",
         swap_result.ideal_amount_out.f64(),
         quote.symbol()
      );
      println!(
         "Swapped {} {} For {} {}",
         amount_in.f64(),
         base.symbol(),
         swap_result.amount_out.f64(),
         quote.symbol()
      );
      println!("With Price Impact: {:.4}%", swap_result.price_impact);
   }

   #[tokio::test]
   async fn price_calculation() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV3Pool::usdt_uni();
      pool.update_state(client.clone(), None).await.unwrap();

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
