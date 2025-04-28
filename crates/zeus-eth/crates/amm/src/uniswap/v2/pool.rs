use alloy_primitives::{
   Address, B256, U256, address,
   utils::{format_units, parse_units},
};
use alloy_rpc_types::BlockId;
use std::borrow::Cow;

use crate::{
   DexKind, minimum_liquidity,
   uniswap::{State, UniswapPool, v4::FeeAmount},
};
use abi::uniswap::v2;
use crate::uniswap::PoolKey;
use currency::{Currency, ERC20Token};
use utils::{is_base_token, price_feed::get_base_token_price};

use alloy_contract::private::{Network, Provider};

use anyhow::bail;
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
   pub state: State,
}

/// Represents the state of a Uniswap V2 Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolReserves {
   pub reserve0: U256,
   pub reserve1: U256,
   pub block: u64,
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

impl UniswapV2Pool {
   /// Tokens are re-ordered as per the Uniswap protocol
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
         fee: FeeAmount::CUSTOM(300), // 0.3% fee
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
   pub async fn from<P, N>(
      client: P,
      chain_id: u64,
      token0: ERC20Token,
      token1: ERC20Token,
      dex: DexKind,
   ) -> Result<Self, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let factory = dex.factory(chain_id)?;
      let address = v2::factory::get_pair(client, factory, token0.address, token1.address).await?;
      if address.is_zero() {
         bail!("Pair not found");
      }
      Ok(Self::new(chain_id, address, token0, token1, dex))
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

   /// Test pool
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

   fn pool_id(&self) -> B256 {
      B256::ZERO
   }

   fn dex_kind(&self) -> DexKind {
      self.dex
   }

   fn state(&self) -> &State {
      &self.state
   }

   fn zero_for_one_v3(&self, _token_in: Address) -> bool {
      panic!("This method only applies to V3");
   }

   fn zero_for_one_v4(&self, _currency_in: &Currency) -> bool {
      panic!("This method only applies to V4");
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

   fn is_token0(&self, token: Address) -> bool {
      self.token0().address == token
   }

   fn is_token1(&self, token: Address) -> bool {
      self.token1().address == token
   }

   fn enough_liquidity(&self) -> bool {
      if !self.state.is_v2() {
         return true;
      }
      let state = self.state.v2_reserves().unwrap();
      let base_token = self.base_token();
      let reserve = if self.is_token0(base_token.address) {
         state.reserve0
      } else {
         state.reserve1
      };

      let threshold = minimum_liquidity(&base_token);
      reserve >= threshold
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
      let reserves = v2::pool::get_reserves(pool.address(), client, block).await?;
      let reserve0 = U256::from(reserves.0);
      let reserve1 = U256::from(reserves.1);
      let reserves = PoolReserves::new(reserve0, reserve1, reserves.2 as u64);

      Ok(State::v2(reserves))
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let state = self
         .state
         .v2_reserves()
         .ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

      let token_in = currency_in.to_erc20().address;

      if self.currency0.to_erc20().address == token_in {
         Ok(super::get_amount_out(
            amount_in,
            self.fee.fee(),
            state.reserve0,
            state.reserve1,
         ))
      } else {
         Ok(super::get_amount_out(
            amount_in,
            self.fee.fee(),
            state.reserve1,
            state.reserve0,
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

      if self.currency0.to_erc20().address == token_in {
         let amount_out = super::get_amount_out(amount_in, self.fee.fee(), state.reserve0, state.reserve1);

         state.reserve0 += amount_in;
         state.reserve1 -= amount_out;
         self.state = State::v2(state);

         Ok(amount_out)
      } else {
         let amount_out = super::get_amount_out(amount_in, self.fee.fee(), state.reserve1, state.reserve0);

         state.reserve0 -= amount_out;
         state.reserve1 += amount_in;
         self.state = State::v2(state);

         Ok(amount_out)
      }
   }

   fn quote_price(&self, base_usd: f64) -> Result<f64, anyhow::Error> {
      if base_usd == 0.0 {
         return Ok(0.0);
      }

      if !self.base_token_exists() {
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

      let mut pool = UniswapV2Pool::weth_uni();

      let state = UniswapV2Pool::fetch_state(client.clone(), pool.clone(), None)
         .await
         .unwrap();
      pool.set_state(state);

      // Swap 1 WETH for UNI
      let base = pool.base_currency();
      let quote = pool.quote_currency();

      let amount_in = parse_units("1", base.decimals()).unwrap().get_absolute();
      let amount_out = pool.simulate_swap(base, amount_in).unwrap();

      let amount_in = format_units(amount_in, base.decimals()).unwrap();
      let amount_out = format_units(amount_out, quote.decimals()).unwrap();

      println!("=== V2 Swap Test ===");
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
      let mut pool = UniswapV2Pool::weth_uni();

      // UNI is token0 and WETH is token1
      let token0 = pool.is_token0(pool.quote_token().address);
      let token1 = pool.is_token1(pool.base_token().address);
      assert_eq!(token0, true);
      assert_eq!(token1, true);

      pool.toggle();
      // Now WETH is token0 and UNI is token1
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

      let mut pool = UniswapV2Pool::weth_uni();

      let state = UniswapV2Pool::fetch_state(client.clone(), pool.clone(), None)
         .await
         .unwrap();
      pool.set_state(state);

      let (base_price, quote_price) = pool.tokens_price(client.clone(), None).await.unwrap();
      let base_token = pool.base_token();
      let quote_token = pool.quote_token();

      println!("{} Price: ${}", base_token.symbol, base_price);
      println!("{} Price: ${}", quote_token.symbol, quote_price);
   }
}
