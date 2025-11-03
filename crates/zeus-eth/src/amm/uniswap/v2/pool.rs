use alloy_primitives::{
   Address, B256, U256, address,
   utils::{format_units, parse_units},
};
use alloy_rpc_types::BlockId;
use core::panic;
use std::borrow::Cow;

use crate::amm::uniswap::state::get_v2_pool_state;
use crate::amm::uniswap::{DexKind, FeeAmount, State, SwapResult, UniswapPool};

use crate::abi::uniswap::{v4::PoolKey, v2};
use crate::currency::{Currency, ERC20Token};
use crate::utils::{NumericValue, price_feed::get_base_token_price};

use alloy_contract::private::{Network, Provider};
use core::cmp::Ordering;
use std::hash::{Hash, Hasher};

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

/// Represents a Uniswap V2 Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV2Pool {
   pub chain_id: u64,
   pub address: Address,
   pub currency0: Currency,
   pub currency1: Currency,
   pub fee: FeeAmount,
   pub dex: DexKind,
   #[serde(skip)]
   pub state: State,
}

impl Ord for UniswapV2Pool {
   fn cmp(&self, other: &Self) -> Ordering {
      self.address.cmp(&other.address)
   }
}

impl PartialOrd for UniswapV2Pool {
   fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
      Some(self.cmp(other))
   }
}

impl Eq for UniswapV2Pool {}

impl PartialEq for UniswapV2Pool {
   fn eq(&self, other: &Self) -> bool {
      self.address == other.address && self.chain_id == other.chain_id
   }
}

impl Hash for UniswapV2Pool {
   fn hash<H: Hasher>(&self, state: &mut H) {
      self.chain_id.hash(state);
      self.currency0.hash(state);
      self.currency1.hash(state);
      self.fee.hash(state);
      self.dex.hash(state);
   }
}

impl UniswapV2Pool {
   /// Tokens are re-ordered as token0 < token1
   pub fn new(chain_id: u64, address: Address, token0: ERC20Token, token1: ERC20Token, dex: DexKind) -> Self {
      let (token0, token1) = if token0.address < token1.address {
         (token0, token1)
      } else {
         (token1, token0)
      };

      Self {
         chain_id,
         address,
         currency0: Currency::from(token0),
         currency1: Currency::from(token1),
         fee: FeeAmount::MEDIUM, // 0.3% fee
         dex,
         state: State::none(),
      }
   }

   pub async fn from_address<P, N>(client: P, chain_id: u64, address: Address) -> Result<Self, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let dex_kind = DexKind::UniswapV2;
      let token0 = v2::pool::token0(address, client.clone()).await?;
      let token1 = v2::pool::token1(address, client.clone()).await?;

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
         chain_id, address, erc_token0, erc_token1, dex_kind,
      ))
   }

   /// Create a new Uniswap V2 Pool from token0, token1 and the DEX
   ///
   /// Returns `None` if the pair does not exist
   pub async fn from_components<P, N>(
      client: P,
      chain_id: u64,
      token0: ERC20Token,
      token1: ERC20Token,
      dex: DexKind,
   ) -> Result<Option<Self>, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let factory = dex.factory(chain_id)?;
      let address = v2::factory::get_pair(client, factory, token0.address, token1.address).await?;
      match address {
         addr if addr.is_zero() => Ok(None),
         addr => Ok(Some(Self::new(chain_id, addr, token0, token1, dex))),
      }
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
}

impl UniswapPool for UniswapV2Pool {
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

   fn state(&self) -> &State {
      &self.state
   }

   fn hooks(&self) -> Address {
      Address::ZERO
   }

   fn zero_for_one(&self, _currency_in: &Currency) -> bool {
      panic!("This method only applies to V3/V4");
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

   fn set_state(&mut self, state: State) {
      self.state = state;
   }

   fn set_state_res(&mut self, state: State) -> Result<(), anyhow::Error> {
      if state.is_v2() {
         self.state = state;
         Ok(())
      } else {
         Err(anyhow::anyhow!("Pool state is not for v2"))
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

   fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error> {
      let price = super::calculate_price_64_x_64(self, currency_in)?;
      let price = super::q64_to_float(price);
      Ok(price)
   }

   fn pool_balances(&self) -> (NumericValue, NumericValue) {
      let state = self.state().v2_reserves();
      if state.is_none() {
         return (NumericValue::default(), NumericValue::default());
      }

      let state = state.unwrap();
      let amount0 = NumericValue::format_wei(state.reserve0, self.currency0().decimals());
      let amount1 = NumericValue::format_wei(state.reserve1, self.currency1().decimals());
      (amount0, amount1)
   }

   fn base_balance(&self) -> NumericValue {
      let state = self.state().v2_reserves();
      if state.is_none() {
         return NumericValue::default();
      }

      let state = state.unwrap();
      if self.currency0().is_base() {
         NumericValue::format_wei(state.reserve0, self.currency0().decimals())
      } else {
         NumericValue::format_wei(state.reserve1, self.currency1().decimals())
      }
   }

   fn quote_balance(&self) -> NumericValue {
      let state = self.state().v2_reserves();
      if state.is_none() {
         return NumericValue::default();
      }

      let state = state.unwrap();

      if self.currency0().is_base() {
         NumericValue::format_wei(state.reserve1, self.currency1().decimals())
      } else {
         NumericValue::format_wei(state.reserve0, self.currency0().decimals())
      }
   }

   fn compute_virtual_reserves(&mut self) -> Result<(), anyhow::Error> {
      Err(anyhow!("This method only applies to V4"))
   }

   async fn update_state<P, N>(&mut self, client: P, block: Option<BlockId>) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let state = get_v2_pool_state(client, self, block).await?;
      self.set_state(state);
      Ok(())
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let state = self
         .state
         .v2_reserves()
         .ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

      let token_in = currency_in.to_erc20().address;
      let fee = self.fee().fee();

      if self.currency0.to_erc20().address == token_in {
         Ok(super::get_amount_out(
            amount_in,
            state.reserve0,
            state.reserve1,
            fee,
         ))
      } else {
         Ok(super::get_amount_out(
            amount_in,
            state.reserve1,
            state.reserve0,
            fee,
         ))
      }
   }

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let mut state = self
         .state
         .v2_reserves()
         .cloned()
         .ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

      let token_in = currency_in.to_erc20().address;
      let fee = self.fee().fee();

      if self.currency0.to_erc20().address == token_in {
         let amount_out = super::get_amount_out(amount_in, state.reserve0, state.reserve1, fee);

         state.reserve0 += amount_in;
         state.reserve1 -= amount_out;
         self.state = State::v2(state);

         Ok(amount_out)
      } else {
         let amount_out = super::get_amount_out(amount_in, state.reserve1, state.reserve0, fee);

         state.reserve0 -= amount_out;
         state.reserve1 += amount_in;
         self.state = State::v2(state);

         Ok(amount_out)
      }
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

      let base_currency = self.base_currency();
      let unit = parse_units("1", base_currency.decimals())?.get_absolute();
      let amount_out = self.simulate_swap(base_currency, unit)?;
      if amount_out == U256::ZERO {
         return Ok(0.0);
      }

      let amount_out = format_units(amount_out, self.quote_currency().decimals())?.parse::<f64>()?;
      let price = base_usd / amount_out;

      Ok(price)
   }

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

impl UniswapV2Pool {
   pub fn weth_uni() -> Self {
      let weth = ERC20Token::weth();
      let uni_addr = address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984");
      let uni = ERC20Token {
         chain_id: 1,
         address: uni_addr,
         decimals: 18,
         symbol: "UNI".to_string(),
         name: "Uniswap Token".to_string(),
         total_supply: U256::ZERO,
      };

      let pool_address = address!("d3d2E2692501A5c9Ca623199D38826e513033a17");
      UniswapV2Pool::new(1, pool_address, weth, uni, DexKind::UniswapV2)
   }

   pub fn weth_wbtc() -> Self {
      let weth = ERC20Token::weth();
      let wbtc = address!("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");
      let wbtc = ERC20Token {
         chain_id: 1,
         address: wbtc,
         decimals: 8,
         symbol: "WBTC".to_string(),
         name: "Wrapped BTC".to_string(),
         total_supply: U256::ZERO,
      };

      let pool_address = address!("0xBb2b8038a1640196FbE3e38816F3e67Cba72D940");
      UniswapV2Pool::new(1, pool_address, weth, wbtc, DexKind::UniswapV2)
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

      let mut pool = UniswapV2Pool::weth_uni();

      pool.update_state(client.clone(), None).await.unwrap();

      // Swap 1 WETH for UNI
      let base = pool.base_currency();
      let quote = pool.quote_currency();

      let amount_in = NumericValue::parse_to_wei("1", base.decimals());
      let swap_result = pool
         .simulate_swap_result(base, quote, amount_in.clone())
         .unwrap();

      println!("=== V2 Swap Test ===");
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

      let mut pool = UniswapV2Pool::weth_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      let (base_price, quote_price) = pool.tokens_price(client.clone(), None).await.unwrap();
      let base_token = pool.base_token();
      let quote_token = pool.quote_token();

      let uni_in_eth = pool.calculate_price(pool.quote_currency()).unwrap();
      let eth_in_uni = pool.calculate_price(pool.base_currency()).unwrap();

      println!("{} Price: ${}", base_token.symbol, base_price);
      println!("{} Price: ${}", quote_token.symbol, quote_price);
      println!("UNI in terms of ETH: {}", uni_in_eth);
      println!("ETH in terms of UNI: {}", eth_in_uni);
   }
}
