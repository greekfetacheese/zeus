use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, B256, U256, address,
   utils::{format_units, keccak256, parse_units},
};
use alloy_rpc_types::BlockId;
use alloy_sol_types::SolValue;
use std::collections::HashMap;

use crate::{
   DexKind, minimum_liquidity, sorts_before,
   uniswap::{UniswapPool, state::*, v3::*, v4::FeeAmount},
};

use abi::uniswap::v4::PoolKey;
use currency::{Currency, ERC20Token, NativeCurrency};
use uniswap_v3_math::{
   sqrt_price_math,
   tick_math::{self, MAX_TICK, MIN_TICK},
};
use utils::{NumericValue, batch, price_feed::get_base_token_price};

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
   pub liquidity_amount0: U256,
   pub liquidity_amount1: U256,
}

impl PartialEq for UniswapV4Pool {
   fn eq(&self, other: &Self) -> bool {
      self.pool_id == other.pool_id && self.chain_id == other.chain_id
   }
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
   ) -> Self {
      let pool_key = Self::get_pool_key(&currency_a, &currency_b, fee, hooks);
      let pool_id = Self::get_pool_id(&currency_a, &currency_b, fee, hooks);

      let (currency0, currency1) = if sorts_before(&currency_a, &currency_b) {
         (currency_a, currency_b)
      } else {
         (currency_b, currency_a)
      };

      Self {
         chain_id,
         fee,
         dex,
         currency0,
         currency1,
         state,
         pool_key,
         pool_id,
         hooks,
         liquidity_amount0: U256::ZERO,
         liquidity_amount1: U256::ZERO,
      }
   }

   pub fn from(
      chain_id: u64,
      currency_a: Currency,
      currency_b: Currency,
      fee: FeeAmount,
      dex: DexKind,
      hooks: Address,
   ) -> Self {
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

   pub fn get_pool_id(currency_a: &Currency, currency_b: &Currency, fee: FeeAmount, hooks: Address) -> B256 {
      let (currency0_addr, currency1_addr) = Self::sort_currency_address(currency_a, currency_b);
      keccak256(
         (
            currency0_addr,
            currency1_addr,
            fee.fee_u24(),
            fee.tick_spacing(),
            hooks,
         )
            .abi_encode(),
      )
   }

   pub fn get_pool_key(currency_a: &Currency, currency_b: &Currency, fee: FeeAmount, hooks: Address) -> PoolKey {
      let (currency0_addr, currency1_addr) = Self::sort_currency_address(currency_a, currency_b);
      PoolKey {
         currency0: currency0_addr,
         currency1: currency1_addr,
         fee: fee.fee_u24(),
         tickSpacing: fee.tick_spacing(),
         hooks,
      }
   }

   pub fn sort_currency_address(currency_a: &Currency, currency_b: &Currency) -> (Address, Address) {
      let address_a = if currency_a.is_native() {
         Address::ZERO
      } else {
         currency_a.address()
      };

      let address_b = if currency_b.is_native() {
         Address::ZERO
      } else {
         currency_b.address()
      };

      if address_a < address_b {
         (address_a, address_b)
      } else {
         (address_b, address_a)
      }
   }

   pub fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error> {
      let zero_for_one = self.zero_for_one_v4(currency_in);
      let price = calculate_price(self, zero_for_one)?;
      Ok(price)
   }

   pub fn state_mut(&mut self) -> &mut State {
      &mut self.state
   }

   fn hook_impacts_swap(&self) -> bool {
      super::has_swap_permissions(self.hooks)
   }

   // * Test pools
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
   }

   pub fn usdc_usdt() -> Self {
      let currency_a = Currency::from(ERC20Token::usdc());
      let currency_b = Currency::from(ERC20Token::usdt());
      let fee = FeeAmount::CUSTOM(10); // 0.001%

      Self::from(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn eth_usdt() -> Self {
      let currency_a = Currency::from(NativeCurrency::from(1));
      let currency_b = Currency::from(ERC20Token::usdt());
      let fee = FeeAmount::MEDIUM;

      Self::from(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn link_usdc() -> Self {
      let currency_a = Currency::from(ERC20Token::link());
      let currency_b = Currency::from(ERC20Token::usdc());
      let fee = FeeAmount::MEDIUM;

      Self::from(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn usdc_wbtc() -> Self {
      let currency_a = Currency::from(ERC20Token::usdc());
      let currency_b = Currency::from(ERC20Token::wbtc());
      let fee = FeeAmount::MEDIUM;

      Self::from(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn wbtc_usdt() -> Self {
      let currency_a = Currency::from(ERC20Token::usdt());
      let currency_b = Currency::from(ERC20Token::wbtc());
      let fee = FeeAmount::MEDIUM;

      Self::from(
         1,
         currency_a,
         currency_b,
         fee,
         DexKind::UniswapV4,
         Address::ZERO,
      )
   }

   pub fn set_tick_data(&mut self, ticks: HashMap<i32, TickInfo>, tick_bitmap: HashMap<i16, U256>) {
      let mut state = self.state.v3_or_v4_state().cloned().unwrap();
      state.ticks = ticks;
      state.tick_bitmap = tick_bitmap;
      self.set_state(State::V4(state));
   }

   /*
   /// Gets all tick data (liquidityGross, liquidityNet) for initialized ticks
   /// and the corresponding tick bitmaps for a given Uniswap V4 pool.
   pub async fn get_all_tick_data<P, N>(
      client: P,
      pool: impl UniswapPool,
      state_view: Address,
      block: Option<BlockId>,
   ) -> Result<(HashMap<i32, TickInfo>, HashMap<i16, U256>), anyhow::Error>
   where
      P: Provider<N> + Clone + Send + Sync + 'static,
      N: Network,
   {
      let mut all_ticks_info: HashMap<i32, TickInfo> = HashMap::new();
      let mut all_tick_bitmaps: HashMap<i16, U256> = HashMap::new();

      let tick_spacing_i32 = pool.fee().tick_spacing_i32();
      let tick_spacing_i24 = pool.fee().tick_spacing();

      if tick_spacing_i32 <= 0 {
         return Err(anyhow::anyhow!("Tick spacing must be positive"));
      }

      let min_compressed_tick = MIN_TICK / tick_spacing_i32;
      let max_compressed_tick = MAX_TICK / tick_spacing_i32;

      let min_word_pos = (min_compressed_tick >> 8) as i16;
      let max_word_pos = (max_compressed_tick >> 8) as i16;

      let result = batch::get_v4_pool_tick_data(
         client,
         pool.pool_id(),
         state_view,
         min_word_pos,
         max_word_pos,
         tick_spacing_i24,
         block,
      )
      .await?;

      let ticks = result.allTicksInfo;
      let tick_bitmaps = result.populatedBitmapWords;
      let word_positions = result.correspondingWordPositions;

      for tick in ticks {
         let actual_tick: i32 = tick.actualTick.try_into()?;
         all_ticks_info.insert(
            actual_tick,
            TickInfo {
               liquidity_gross: tick.liquidityGross,
               liquidity_net: tick.liquidityNet,
               initialized: true,
            },
         );
      }

      for (i, word) in tick_bitmaps.iter().enumerate() {
         all_tick_bitmaps.insert(word_positions[i], *word);
      }

      Ok((all_ticks_info, all_tick_bitmaps))
   }
   */
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

   fn enough_liquidity(&self) -> bool {
      let threshold = minimum_liquidity(&self.base_currency().to_erc20());
      let balance = self.base_balance();
      balance.wei2() >= threshold
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

   fn get_pool_key(&self) -> Result<PoolKey, anyhow::Error> {
      Ok(self.pool_key.clone())
   }

   fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error> {
      let zero_for_one = self.zero_for_one_v4(currency_in);
      let price = calculate_price(self, zero_for_one)?;
      Ok(price)
   }

   fn pool_balances(&self) -> (NumericValue, NumericValue) {
      let amount0 = NumericValue::format_wei(self.liquidity_amount0, self.currency0().decimals());
      let amount1 = NumericValue::format_wei(self.liquidity_amount1, self.currency1().decimals());
      (amount0, amount1)
   }

   fn base_balance(&self) -> NumericValue {
      let amount0 = NumericValue::format_wei(self.liquidity_amount0, self.currency0().decimals());
      let amount1 = NumericValue::format_wei(self.liquidity_amount1, self.currency1().decimals());
      if self.currency0().is_base() {
         amount0
      } else {
         amount1
      }
   }

   fn quote_balance(&self) -> NumericValue {
      let amount0 = NumericValue::format_wei(self.liquidity_amount1, self.currency1().decimals());
      let amount1 = NumericValue::format_wei(self.liquidity_amount0, self.currency0().decimals());
      if self.currency0().is_base() {
         amount1
      } else {
         amount0
      }
   }

   fn calculate_liquidity2(&mut self) -> Result<(), anyhow::Error> {
      let state = self
         .state()
         .v3_or_v4_state()
         .cloned()
         .ok_or(anyhow!("State not initialized"))?;

      let current_price = self.calculate_price(self.currency0())?;

      let is_stable_pair = self.currency0().is_stablecoin() && self.currency1().is_stablecoin();

      // for stable pairs use a 0.01% range, for others 1%
      let (lower_price, upper_price) = if is_stable_pair {
         let lower_price = current_price * 0.999;
         let upper_price = current_price * 1.001;
         (lower_price, upper_price)
      } else {
         let lower_price = current_price * 0.99;
         let upper_price = current_price * 1.01;
         (lower_price, upper_price)
      };

      let lower_tick = fee_math::get_tick_from_price(lower_price);
      let upper_tick = fee_math::get_tick_from_price(upper_price);

      let sqrt_price_ax96 = tick_math::get_sqrt_ratio_at_tick(lower_tick)?;
      let sqrt_price_bx96 = tick_math::get_sqrt_ratio_at_tick(upper_tick)?;

      let liquidity0 = sqrt_price_math::_get_amount_0_delta(sqrt_price_ax96, sqrt_price_bx96, state.liquidity, true)?;
      let liquidity1 = sqrt_price_math::_get_amount_1_delta(sqrt_price_ax96, sqrt_price_bx96, state.liquidity, true)?;

      self.liquidity_amount0 = liquidity0;
      self.liquidity_amount1 = liquidity1;
      Ok(())
   }

   #[allow(non_snake_case)]
   fn calculate_liquidity(&mut self) -> Result<(), anyhow::Error> {
      let v3_state = self
         .state()
         .v3_or_v4_state()
         .cloned()
         .ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

      let current_pool_sqrt_price_x96 = v3_state.sqrt_price;

      let mut total_calculated_amount0 = U256::ZERO;
      let mut total_calculated_amount1 = U256::ZERO;

      let mut sorted_tick_indices: Vec<i32> = v3_state.ticks.keys().cloned().collect();
      sorted_tick_indices.sort_unstable();

      if sorted_tick_indices.is_empty() {
         if v3_state.liquidity > 0 {
            let (amount0, amount1) = calculate_liquidity_amounts(
               tick_math::MIN_SQRT_RATIO,
               tick_math::MAX_SQRT_RATIO,
               v3_state.liquidity,
               current_pool_sqrt_price_x96,
            )?;
            total_calculated_amount0 = amount0;
            total_calculated_amount1 = amount1;
         }
      } else {
         let mut current_L_net_accumulator: i128 = 0;
         let mut tick_lower_bound_for_segment: i32 = tick_math::MIN_TICK;

         for i in 0..=sorted_tick_indices.len() {
            let tick_upper_bound_for_segment: i32 = if i < sorted_tick_indices.len() {
               sorted_tick_indices[i]
            } else {
               tick_math::MAX_TICK
            };

            if current_L_net_accumulator > 0 {
               let liquidity_for_this_segment = current_L_net_accumulator as u128;

               let sqrt_price_segment_lower = tick_math::get_sqrt_ratio_at_tick(tick_lower_bound_for_segment)?;
               let sqrt_price_segment_upper = tick_math::get_sqrt_ratio_at_tick(tick_upper_bound_for_segment)?;

               if sqrt_price_segment_lower != sqrt_price_segment_upper {
                  let (segment_amount0, segment_amount1) = calculate_liquidity_amounts(
                     sqrt_price_segment_lower,
                     sqrt_price_segment_upper,
                     liquidity_for_this_segment,
                     current_pool_sqrt_price_x96,
                  )?;

                  total_calculated_amount0 = total_calculated_amount0.saturating_add(segment_amount0);
                  total_calculated_amount1 = total_calculated_amount1.saturating_add(segment_amount1);
               }
            }

            if i < sorted_tick_indices.len() {
               let current_processed_tick_index = sorted_tick_indices[i];
               if let Some(tick_info) = v3_state.ticks.get(&current_processed_tick_index) {
                  current_L_net_accumulator += tick_info.liquidity_net;
               } else {
                  return Err(anyhow::anyhow!(
                     "Tick info missing for tick: {}",
                     current_processed_tick_index
                  ));
               }
            }

            tick_lower_bound_for_segment = tick_upper_bound_for_segment;

            if tick_lower_bound_for_segment >= tick_math::MAX_TICK && i < sorted_tick_indices.len() {
               break;
            }
         }
      }

      self.liquidity_amount0 = total_calculated_amount0;
      self.liquidity_amount1 = total_calculated_amount1;

      Ok(())
   }

   async fn update_state<P, N>(&mut self, client: P, block: Option<BlockId>) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let state = get_v4_pool_state(client, self, block).await?;
      self.set_state(state);
      self.calculate_liquidity2()?;
      Ok(())
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      if self.hook_impacts_swap() {
         return Err(anyhow::anyhow!("Unsupported Hook"));
      }

      let zero_for_one = self.zero_for_one_v4(currency_in);
      let fee = self.fee.fee();
      let state = self
         .state()
         .v3_or_v4_state()
         .ok_or(anyhow::anyhow!("State not initialized"))?;
      let amount_out = calculate_swap(state, fee, zero_for_one, amount_in)?;
      Ok(amount_out)
   }

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      if self.hook_impacts_swap() {
         return Err(anyhow::anyhow!("Unsupported Hook"));
      }

      let zero_for_one = self.zero_for_one_v4(currency_in);
      let fee = self.fee.fee();
      let mut state = self
         .state_mut()
         .v3_or_v4_state_mut()
         .ok_or(anyhow::anyhow!("State not initialized"))?;
      let amount_out = calculate_swap(&mut state, fee, zero_for_one, amount_in)?;

      Ok(amount_out)
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

      if !self.base_currency_exists() {
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
   use alloy_primitives::{
      B256,
      utils::{format_units, parse_units},
   };
   use alloy_provider::ProviderBuilder;
   use std::str::FromStr;
   use url::Url;

   #[test]
   fn correct_pool_creation() {
      let pool1 = UniswapV4Pool::link_usdc();
      let id1 = B256::from_str("0x50ae33c238824aa1937d5d9f1766c487bca39b548f8d957994e8357eeeca3280").unwrap();
      assert_eq!(pool1.pool_id(), id1);
   }

   #[tokio::test]
   async fn can_swap() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

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

   /*
   #[tokio::test]
   async fn test_base_token_liquidity_eth_uni() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let state_view = utils::address::uniswap_v4_stateview(1).unwrap();
      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();
      let (ticks, bitmaps) = UniswapV4Pool::get_all_tick_data(client.clone(), pool.clone(), state_view, None)
         .await
         .unwrap();

      pool.set_tick_data(ticks, bitmaps);
      pool.calculate_liquidity().unwrap();

      let eth_balance = pool.base_balance();

      println!("ETH Liquidity: {}", eth_balance.formatted());
   }
   */

   #[tokio::test]
   async fn test_base_token_liquidity_usdc_usdt() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV4Pool::usdc_usdt();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.calculate_liquidity2().unwrap();
      let base_liq = pool.base_balance();

      println!(
         "{} Liquidity: {}",
         pool.base_currency().symbol(),
         base_liq.formatted()
      );
   }

   #[tokio::test]
   async fn test_base_token_liquidity_usdc_wbtc() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV4Pool::usdc_wbtc();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.calculate_liquidity2().unwrap();
      let base_liq = pool.base_balance();

      println!(
         "{} Liquidity: {}",
         pool.base_currency().symbol(),
         base_liq.formatted()
      );
   }

   #[tokio::test]
   async fn price_calculation() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.calculate_liquidity2().unwrap();

      let (base_price, quote_price) = pool.tokens_price(client.clone(), None).await.unwrap();

      let uni_in_eth = pool.calculate_price(pool.quote_currency()).unwrap();
      let eth_in_uni = pool.calculate_price(pool.base_currency()).unwrap();
      let eth_balance = pool.base_balance();

      println!("{} Price: ${}", pool.base_currency().symbol(), base_price);
      println!("{} Price: ${}", pool.quote_currency().symbol(), quote_price);
      println!("UNI in terms of ETH: {}", uni_in_eth);
      println!("ETH in terms of UNI: {}", eth_in_uni);
      println!("ETH Liquidity: {}", eth_balance.formatted());
   }

   #[test]
   fn pool_order() {
      let pool = UniswapV4Pool::eth_uni();

      let eth = pool.base_currency().clone();
      let uni = pool.quote_currency().clone();

      let currency0 = pool.is_currency0(&eth);
      let currency1 = pool.is_currency1(&uni);
      assert_eq!(currency0, true);
      assert_eq!(currency1, true);

      let base_exists = pool.base_currency_exists();
      assert_eq!(base_exists, true);
   }
}
