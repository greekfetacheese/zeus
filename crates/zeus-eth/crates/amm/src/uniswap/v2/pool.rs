use alloy_primitives::{
   Address, U256,
   utils::{format_units, parse_units},
};
use alloy_rpc_types::BlockId;

use crate::{DexKind, minimum_liquidity};
use abi::uniswap::v2;
use currency::erc20::ERC20Token;
use utils::{is_base_token, price_feed::get_base_token_price};

use alloy_contract::private::{Network, Provider};

use anyhow::bail;
use serde::{Deserialize, Serialize};

/// Represents a Uniswap V2 Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV2Pool {
   pub chain_id: u64,
   pub address: Address,
   pub token0: ERC20Token,
   pub token1: ERC20Token,
   pub dex: DexKind,
   pub state: Option<PoolReserves>,

   /// Base token USD price
   pub base_usd: f64,

   /// Quote token USD price
   pub quote_usd: f64,
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
         token0,
         token1,
         dex,
         state: None,
         base_usd: 0.0,
         quote_usd: 0.0,
      }
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
      P: Provider<(), N> + Clone + 'static,
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
      std::mem::swap(&mut self.token0, &mut self.token1);
   }

   /// Restore the original order of the tokens
   pub fn reorder(&mut self) {
      if self.token0.address > self.token1.address {
         std::mem::swap(&mut self.token0, &mut self.token1);
      }
   }

   /// Return a reference to the state of this pool
   pub fn state(&self) -> Option<&PoolReserves> {
      self.state.as_ref()
   }

   /// Update the state for this pool
   pub fn update_state(&mut self, state: PoolReserves) {
      self.state = Some(state);
   }

   pub fn is_token0(&self, token: Address) -> bool {
      self.token0.address == token
   }

   pub fn is_token1(&self, token: Address) -> bool {
      self.token1.address == token
   }

   /// Does this pool have enough liquidity
   ///
   /// If state is None, it will return true so we dont accidentally remove pools
   pub fn enough_liquidity(&self) -> bool {
      let base_token = self.base_token();
      let reserve = if self.is_token0(base_token.address) && self.state.is_some() {
         self.state.as_ref().unwrap().reserve0
      } else if self.is_token1(base_token.address) && self.state.is_some() {
         self.state.as_ref().unwrap().reserve1
      } else {
         return true;
      };
      let threshold = minimum_liquidity(base_token);
      reserve >= threshold
   }

   /// See [is_base_token]
   pub fn base_token(&self) -> ERC20Token {
      if is_base_token(self.chain_id, self.token0.address) {
         self.token0.clone()
      } else {
         self.token1.clone()
      }
   }

   /// Anything that is not [is_base_token]
   pub fn quote_token(&self) -> ERC20Token {
      if is_base_token(self.chain_id, self.token0.address) {
         self.token1.clone()
      } else {
         self.token0.clone()
      }
   }

   /// Fetch the state of the pool at a given block
   /// If block is None, the latest block is used
   pub async fn fetch_state<P, N>(
      client: P,
      pool: Address,
      block: Option<BlockId>,
   ) -> Result<PoolReserves, anyhow::Error>
   where
      P: Provider<(), N> + Clone + 'static,
      N: Network,
   {
      let reserves = v2::pool::get_reserves(pool, client, block).await?;
      let reserve0 = U256::from(reserves.0);
      let reserve1 = U256::from(reserves.1);

      Ok(PoolReserves::new(reserve0, reserve1, reserves.2 as u64))
   }

   pub fn simulate_swap(&self, token_in: Address, amount_in: U256) -> Result<U256, anyhow::Error> {
      let state = self
         .state
         .as_ref()
         .ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

      if self.token0.address == token_in {
         Ok(super::get_amount_out(
            amount_in,
            state.reserve0,
            state.reserve1,
         ))
      } else {
         Ok(super::get_amount_out(
            amount_in,
            state.reserve1,
            state.reserve0,
         ))
      }
   }

   pub fn simulate_swap_mut(&mut self, token_in: Address, amount_in: U256) -> Result<U256, anyhow::Error> {
      let mut state = self
         .state
         .clone()
         .ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

      if self.token0.address == token_in {
         let amount_out = super::get_amount_out(amount_in, state.reserve0, state.reserve1);

         state.reserve0 += amount_in;
         state.reserve1 -= amount_out;
         self.state = Some(state);

         Ok(amount_out)
      } else {
         let amount_out = super::get_amount_out(amount_in, state.reserve1, state.reserve0);

         state.reserve0 -= amount_out;
         state.reserve1 += amount_in;
         self.state = Some(state);

         Ok(amount_out)
      }
   }

   /// Calculate quote token price
   pub fn caluclate_quote_price(&mut self, base_usd: f64) -> f64 {
      if base_usd == 0.0 {
         return 0.0;
      }

      let base_token = self.base_token();

      let unit = parse_units("1", base_token.decimals)
         .unwrap()
         .get_absolute();
      let amount_out = self.simulate_swap(base_token.address, unit).unwrap();
      let amount_out = format_units(amount_out, base_token.decimals)
         .unwrap()
         .parse::<f64>()
         .unwrap();

      if amount_out == 0.0 {
         return 0.0;
      }

      let quote_price = base_usd / amount_out;
      self.quote_usd = quote_price;
      quote_price
   }

   /// Token0 USD price but we need to know the usd price of token1
   pub fn token0_price(&self, token1_price: f64) -> Result<f64, anyhow::Error> {
      if token1_price == 0.0 {
         return Ok(0.0);
      }

      let unit = parse_units("1", self.token1.decimals)?.get_absolute();
      let amount_out = self.simulate_swap(self.token1.address, unit)?;
      if amount_out == U256::ZERO {
         return Ok(0.0);
      }

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
      if amount_out == U256::ZERO {
         return Ok(0.0);
      }

      let amount_out = format_units(amount_out, self.token0.decimals)?.parse::<f64>()?;
      Ok(token0_price / amount_out)
   }

   /// Get the usd value of token0 and token1 at a given block
   /// If block is None, the latest block is used
   pub async fn tokens_usd<P, N>(&self, client: P, block: Option<BlockId>) -> Result<(f64, f64), anyhow::Error>
   where
      P: Provider<(), N> + Clone + 'static,
      N: Network,
   {
      let chain_id = self.chain_id;

      // token0 is known
      if is_base_token(chain_id, self.token0.address) {
         let price0 = get_base_token_price(client.clone(), chain_id, self.token0.address, block).await?;
         let price1 = self.token1_price(price0)?;
         Ok((price0, price1))
      } else if is_base_token(chain_id, self.token1.address) {
         let price1 = get_base_token_price(client.clone(), chain_id, self.token1.address, block).await?;
         let price0 = self.token0_price(price1)?;
         Ok((price0, price1))
      } else {
         bail!("Could not determine a common paired token");
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::{
      address,
      utils::{format_units, parse_units},
   };
   use alloy_provider::ProviderBuilder;
   use url::Url;
   use utils::address::weth;

   #[tokio::test]
   async fn uniswap_v2_pool_test() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let weth = ERC20Token::new(client.clone(), weth(1).unwrap(), 1)
         .await
         .unwrap();
      let uni_addr = address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984");
      let uni = ERC20Token::new(client.clone(), uni_addr, 1).await.unwrap();

      let pool_address = address!("d3d2E2692501A5c9Ca623199D38826e513033a17");
      let mut pool = UniswapV2Pool::new(
         1,
         pool_address,
         weth.clone(),
         uni.clone(),
         DexKind::UniswapV2,
      );

      let state = UniswapV2Pool::fetch_state(client.clone(), pool_address, None)
         .await
         .unwrap();
      pool.update_state(state);

      let amount_in = parse_units("1", weth.decimals).unwrap().get_absolute();
      let amount_out = pool.simulate_swap(weth.address, amount_in).unwrap();

      let amount_in = format_units(amount_in, weth.decimals).unwrap();
      let amount_out = format_units(amount_out, uni.decimals).unwrap();

      println!("=== V2 Swap Test ===");
      println!(
         "Swapped {} {} For {} {}",
         amount_in, weth.symbol, amount_out, uni.symbol
      );
      println!("=== Tokens Price Test ===");

      let (token0_usd, token1_usd) = pool.tokens_usd(client.clone(), None).await.unwrap();
      println!("{} Price: ${}", pool.token0.symbol, token0_usd);
      println!("{} Price: ${}", pool.token1.symbol, token1_usd);

      println!("=== Quote Price Test ===");
      let quote_price = pool.caluclate_quote_price(token1_usd);
      println!("{} Price: ${}", pool.quote_token().symbol, quote_price);

      assert_eq!(pool.token0.address, uni.address);
      assert_eq!(pool.token1.address, weth.address);

      pool.toggle();
      assert_eq!(pool.token0.address, weth.address);
      assert_eq!(pool.token1.address, uni.address);

      pool.reorder();
      assert_eq!(pool.token0.address, uni.address);
      assert_eq!(pool.token1.address, weth.address);
   }
}
