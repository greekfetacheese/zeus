use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, B256, U256, address,
   utils::{format_units, keccak256, parse_units},
};
use alloy_rpc_types::BlockId;
use alloy_sol_types::SolValue;

use crate::{
   DexKind, sorts_before,
   uniswap::{
      UniswapPool,
      state::*,
      v3::{calculate_price, calculate_swap},
      v4::FeeAmount,
   },
};
use abi::uniswap::v4::PoolKey;
use currency::{Currency, ERC20Token, NativeCurrency};
use utils::{is_base_token, price_feed::get_base_token_price};
use uniswap_v3_math::tick_math::{MIN_TICK, MAX_TICK};

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniswapV4Pool {
   pub chain_id: u64,
   pub fee: FeeAmount,
   pub dex: DexKind,
   pub currency0: Currency,
   pub currency1: Currency,
   #[serde(skip)]
   pub state: State,
   pub pool_key: PoolKey,
   pub pool_id: B256,
   pub hooks: Address,
}

impl UniswapV4Pool {
   pub fn new(
      chain_id: u64,
      fee: FeeAmount,
      dex: DexKind,
      currency_a: Currency,
      currency_b: Currency,
      state: State,
      hooks: Address,
   ) -> Result<Self, anyhow::Error> {
      let pool_key = Self::get_pool_key(&currency_a, &currency_b, fee, hooks)?;
      let pool_id = Self::get_pool_id(&currency_a, &currency_b, fee, hooks)?;

      let (currency0, currency1) = if sorts_before(&currency_a, &currency_b) {
         (currency_a, currency_b)
      } else {
         (currency_b, currency_a)
      };

      Ok(Self {
         chain_id,
         fee,
         dex,
         currency0,
         currency1,
         state,
         pool_key,
         pool_id,
         hooks,
      })
   }

   pub fn from(
      chain_id: u64,
      currency_a: Currency,
      currency_b: Currency,
      fee: FeeAmount,
      dex: DexKind,
      hooks: Address,
   ) -> Result<Self, anyhow::Error> {
      Self::new(
         chain_id,
         fee,
         dex,
         currency_a,
         currency_b,
         State::none(),
         hooks,
      )
   }

   pub fn get_pool_id(
      currency_a: &Currency,
      currency_b: &Currency,
      fee: FeeAmount,
      hooks: Address,
   ) -> Result<B256, anyhow::Error> {
      let (currency0_addr, currency1_addr) = Self::sort_currency_address(currency_a, currency_b);
      Ok(keccak256(
         (
            currency0_addr,
            currency1_addr,
            fee.fee_u24(),
            fee.tick_spacing(),
            hooks,
         )
            .abi_encode(),
      ))
   }

   pub fn get_pool_key(
      currency_a: &Currency,
      currency_b: &Currency,
      fee: FeeAmount,
      hooks: Address,
   ) -> Result<PoolKey, anyhow::Error> {
      let (currency0_addr, currency1_addr) = Self::sort_currency_address(currency_a, currency_b);
      Ok(PoolKey {
         currency0: currency0_addr,
         currency1: currency1_addr,
         fee: fee.fee_u24(),
         tickSpacing: fee.tick_spacing(),
         hooks,
      })
   }

   pub fn sort_currency_address(currency_a: &Currency, currency_b: &Currency) -> (Address, Address) {
      if currency_a.is_native() {
         (Address::ZERO, currency_b.to_erc20().address)
      } else if currency_b.is_native() {
         (Address::ZERO, currency_a.to_erc20().address)
      } else if sorts_before(currency_a, currency_b) {
         (currency_a.to_erc20().address, currency_b.to_erc20().address)
      } else {
         (currency_b.to_erc20().address, currency_a.to_erc20().address)
      }
   }

   /// Switch the tokens in the pool
   pub fn toggle(&mut self) {
      std::mem::swap(&mut self.currency0, &mut self.currency1);
   }

   /// Restore the original order of the tokens
   pub fn reorder(&mut self) {
      if !sorts_before(&self.currency0, &self.currency1) {
         self.toggle();
      }
   }

   pub fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error> {
      let zero_for_one = self.zero_for_one_v4(currency_in);
      let price = calculate_price(self, zero_for_one)?;
      Ok(price)
   }

   fn hook_impacts_swap(&self) -> bool {
      // could use this function to clear certain hooks that may have swap Permissions, but we
      // know they don't interfere in the swap outcome
      super::has_swap_permissions(self.hooks)
   }

   pub fn tick_to_word(&self, tick: i32, tick_spacing: i32) -> i32 {
      let mut compressed = tick / tick_spacing;
      if tick < 0 && tick % tick_spacing != 0 {
         compressed -= 1;
      }

      compressed >> 8
   }

   /// Test pool
   pub fn eth_uni() -> Self {
      let uni_addr = address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984");
      let uni = ERC20Token {
         chain_id: 1,
         address: uni_addr,
         decimals: 18,
         symbol: "UNI".to_string(),
         name: "Uniswap Token".to_string(),
         total_supply: U256::ZERO,
      };
      let currency_a = Currency::from(uni);
      let currency_b = Currency::from(NativeCurrency::from(1));
      let fee = FeeAmount::MEDIUM;

      Self::from(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
      .unwrap()
   }
}

impl UniswapPool for UniswapV4Pool {
   fn chain_id(&self) -> u64 {
      self.chain_id
   }

   fn address(&self) -> Address {
      Address::ZERO
   }

   fn fee(&self) -> FeeAmount {
      self.fee
   }

   fn pool_id(&self) -> B256 {
      self.pool_id
   }

   fn dex_kind(&self) -> DexKind {
      self.dex
   }

   fn hooks(&self) -> Address {
      self.hooks
   }

   fn min_word(&self) -> i32 {
      self.tick_to_word(MIN_TICK, self.fee.tick_spacing_i32())
   }

   fn max_word(&self) -> i32 {
      self.tick_to_word(MAX_TICK, self.fee.tick_spacing_i32())
   }

   fn zero_for_one_v3(&self, _token_in: Address) -> bool {
      panic!("This method only applies to V3");
   }

   fn zero_for_one_v4(&self, currency_in: &Currency) -> bool {
      currency_in == &self.currency0
   }

   fn is_token0(&self, token: Address) -> bool {
      if self.currency0().is_native() {
         return false;
      }
      self.currency0().to_erc20().address == token
   }

   fn is_token1(&self, token: Address) -> bool {
      if self.currency1().is_native() {
         return false;
      }
      self.currency1().to_erc20().address == token
   }

   fn currency0(&self) -> &Currency {
      &self.currency0
   }

   fn currency1(&self) -> &Currency {
      &self.currency1
   }

   fn have(&self, currency: &Currency) -> bool {
      self.is_currency0(currency) || self.is_currency1(currency)
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
      if state.is_v4() {
         self.state = state;
         Ok(())
      } else {
         Err(anyhow::anyhow!("Pool state is not for v4"))
      }
   }

   // TODO
   fn enough_liquidity(&self) -> bool {
      /*
      let threshold = minimum_liquidity(&self.base_currency().to_erc20());
      if !self.state.is_v3() {
         return true;
      } else {
         return self.state.v3_state().unwrap().base_token_liquidity >= threshold;
      }
      */
      false
   }

   fn base_token_exists(&self) -> bool {
      if is_base_token(self.chain_id, self.currency0().to_erc20().address) {
         true
      } else if is_base_token(self.chain_id, self.currency1().to_erc20().address) {
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
      Ok(self.pool_key.clone())
   }

   async fn update_state<P, N>(&mut self, client: P, block: Option<BlockId>) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let state = get_v4_pool_state(client, self, block).await?;
      self.set_state(state);
      Ok(())
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      if self.hook_impacts_swap() {
         return Err(anyhow::anyhow!("Unsupported Hook"));
      }

      let zero_for_one = self.zero_for_one_v4(currency_in);
      let (amount_out, _) =
         calculate_swap(self, zero_for_one, amount_in).map_err(|e| anyhow!("Failed to calculate swap: {:?}", e))?;
      Ok(amount_out)
   }

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      if self.hook_impacts_swap() {
         return Err(anyhow::anyhow!("Unsupported Hook"));
      }

      let zero_for_one = self.zero_for_one_v4(currency_in);
      let (amount_out, current_state) = calculate_swap(self, zero_for_one, amount_in)?;

      // update the state of the pool
      let mut state = self
         .state()
         .v3_or_v4_state()
         .ok_or(anyhow!("State not initialized"))?
         .clone();
      state.liquidity = current_state.liquidity;
      state.sqrt_price = current_state.sqrt_price_x_96;
      state.tick = current_state.tick;

      self.set_state(State::v4(state));

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

      let amount_out = format_units(amount_out, self.quote_currency().decimals())?.parse::<f64>()?;
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

      let base = self.base_currency().to_erc20();
      let base_price = get_base_token_price(client.clone(), chain_id, base.address, block).await?;
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

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      // Swap 1 ETH for UNI
      let base = pool.base_currency();
      let quote = pool.quote_currency();

      let amount_in = parse_units("10", base.decimals()).unwrap().get_absolute();
      let amount_out = pool.simulate_swap(base, amount_in).unwrap();

      let amount_in = format_units(amount_in, base.decimals()).unwrap();
      let amount_out = format_units(amount_out, quote.decimals()).unwrap();

      println!("=== V4 Swap Test ===");
      println!(
         "Swapped {} {} For {} {}",
         amount_in,
         base.symbol(),
         amount_out,
         quote.symbol()
      );
   }

   #[tokio::test]
   async fn price_calculation() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      let (base_price, quote_price) = pool.tokens_price(client.clone(), None).await.unwrap();

      // UNI in terms of ETH
      // let uni_in_eth = pool.calculate_price(quote_token.address).unwrap();
      // let eth_in_uni = pool.calculate_price(base_token.address).unwrap();

      println!("{} Price: ${}", pool.base_currency().symbol(), base_price);
      println!("{} Price: ${}", pool.quote_currency().symbol(), quote_price);
      // println!("UNI in terms of ETH: {}", uni_in_eth);
      // println!("ETH in terms of UNI: {}", eth_in_uni);
   }

   #[test]
   fn pool_order() {
      let mut pool = UniswapV4Pool::eth_uni();

      let eth = Currency::from(NativeCurrency::from(1));

      let address = address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984");
      let uni = Currency::from(ERC20Token {
         chain_id: 1,
         address,
         decimals: 18,
         symbol: "UNI".to_string(),
         name: "Uniswap Token".to_string(),
         total_supply: U256::ZERO,
      });

      // ETH is currency0 and UNI is currency1
      let currency0 = pool.is_currency0(&eth);
      let currency1 = pool.is_currency1(&uni);
      assert_eq!(currency0, true);
      assert_eq!(currency1, true);

      pool.toggle();
      // Now UNI is currency0 and ETH is currency1
      let currency0 = pool.is_currency0(&uni);
      let currency1 = pool.is_currency1(&eth);
      assert_eq!(currency0, true);
      assert_eq!(currency1, true);

      pool.reorder();
      // Back to the original order
      let currency0 = pool.is_currency0(&eth);
      let currency1 = pool.is_currency1(&uni);
      assert_eq!(currency0, true);
      assert_eq!(currency1, true);

      let base_exists = pool.base_token_exists();
      assert_eq!(base_exists, true);
   }
}
