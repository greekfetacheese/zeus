use alloy_primitives::{
   Address, U256, address,
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
      }
   }

   pub async fn from_address<P, N>(client: P, chain_id: u64, address: Address) -> Result<Self, anyhow::Error>
   where
      P: Provider<(), N> + Clone + 'static,
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

      Ok(Self::new(chain_id, address, erc_token0, erc_token1, dex_kind))
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

   /// Set the state for this pool
   pub fn set_state(&mut self, state: PoolReserves) {
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

   pub fn base_token_exists(&self) -> bool {
      if is_base_token(self.chain_id, self.token0.address) {
         true
      } else if is_base_token(self.chain_id, self.token1.address) {
         true
      } else {
         false
      }
   }

   /// Get the base token of this pool
   ///
   /// See [is_base_token]
   pub fn base_token(&self) -> &ERC20Token {
      if is_base_token(self.chain_id, self.token0.address) {
         &self.token0
      } else {
         &self.token1
      }
   }

   /// Get the quote token of this pool
   ///
   /// Anything that is not [is_base_token]
   pub fn quote_token(&self) -> &ERC20Token {
      if is_base_token(self.chain_id, self.token0.address) {
         &self.token1
      } else {
         &self.token0
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

   /// Quote token USD price but we need to know the usd price of base token
   pub fn quote_price(&self, base_usd: f64) -> Result<f64, anyhow::Error> {
      if base_usd == 0.0 {
         return Ok(0.0);
      }

      if !self.base_token_exists() {
         bail!("Base token not found in the pool");
      }

      let unit = parse_units("1", self.base_token().decimals)?.get_absolute();
      let amount_out = self.simulate_swap(self.base_token().address, unit)?;
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
   pub async fn tokens_price<P, N>(&self, client: P, block: Option<BlockId>) -> Result<(f64, f64), anyhow::Error>
   where
      P: Provider<(), N> + Clone + 'static,
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

      let state = UniswapV2Pool::fetch_state(client.clone(), pool.address, None)
         .await
         .unwrap();
      pool.set_state(state);

      // Swap 1 WETH for UNI
      let base_token = pool.base_token().clone();
      let quote_token = pool.quote_token().clone();

      let amount_in = parse_units("1", base_token.decimals)
         .unwrap()
         .get_absolute();
      let amount_out = pool.simulate_swap(base_token.address, amount_in).unwrap();

      let amount_in = format_units(amount_in, base_token.decimals).unwrap();
      let amount_out = format_units(amount_out, quote_token.decimals).unwrap();

      println!("=== V2 Swap Test ===");
      println!(
         "Swapped {} {} For {} {}",
         amount_in, base_token.symbol, amount_out, quote_token.symbol
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

      let state = UniswapV2Pool::fetch_state(client.clone(), pool.address, None)
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
