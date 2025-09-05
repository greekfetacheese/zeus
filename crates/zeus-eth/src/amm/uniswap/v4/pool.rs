use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, B256, U256, address,
   utils::{format_units, keccak256, parse_units},
};
use alloy_rpc_types::BlockId;
use alloy_sol_types::SolValue;

use crate::amm::uniswap::state::get_v4_pool_state;
use crate::amm::uniswap::{
   DexKind, FeeAmount, State, SwapResult, UniswapPool, minimum_liquidity,
   v3::{calculate_price, calculate_swap, calculate_swap_mut},
};

use crate::abi::uniswap::universal_router_v2::PoolKey;
use crate::currency::{Currency, ERC20Token, NativeCurrency};
use crate::utils::{NumericValue, price_feed::get_base_token_price};
use uniswap_v3_math::sqrt_price_math::Q96;

use anyhow::{anyhow, bail};
use core::cmp::Ordering;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// Max fee to avoid overflows, It doesnt make any sense to swap on these pools anyway
pub const MAX_FEE: f32 = 10.0;

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

impl Ord for UniswapV4Pool {
   fn cmp(&self, other: &Self) -> Ordering {
      self.pool_id.cmp(&other.pool_id)
   }
}

impl PartialOrd for UniswapV4Pool {
   fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
      Some(self.cmp(other))
   }
}

impl Eq for UniswapV4Pool {}

impl PartialEq for UniswapV4Pool {
   fn eq(&self, other: &Self) -> bool {
      self.pool_id == other.pool_id && self.chain_id == other.chain_id
   }
}

impl Hash for UniswapV4Pool {
   fn hash<H: Hasher>(&self, state: &mut H) {
      self.chain_id.hash(state);
      self.currency0.hash(state);
      self.currency1.hash(state);
      self.fee.hash(state);
      self.dex.hash(state);
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

      let (currency0, currency1) = if currency_a.address() < currency_b.address() {
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
      let address_a = currency_a.address();
      let address_b = currency_b.address();

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

   pub fn uni_usdc() -> Self {
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

   #[allow(dead_code)]
   fn dsync_eth() -> Self {
      let dync = ERC20Token {
         chain_id: 1,
         address: address!("0xf94e7d0710709388bCe3161C32B4eEA56d3f91CC"),
         symbol: "DSYNC".to_string(),
         name: "dSync".to_string(),
         decimals: 18,
         total_supply: U256::ZERO,
      };

      let eth = Currency::from(NativeCurrency::from(1));
      let dsync = Currency::from(dync);

      Self::from(
         1,
         eth,
         dsync,
         FeeAmount::HIGH,
         DexKind::UniswapV4,
         Address::ZERO,
      )
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
      if state.is_v3() {
         self.state = state;
         Ok(())
      } else {
         Err(anyhow::anyhow!("Pool state is not for v4"))
      }
   }

   fn enough_liquidity(&self) -> bool {
      let threshold = minimum_liquidity(&self.base_currency().to_erc20(), self.dex);
      let balance = self.base_balance();
      balance.wei() >= threshold
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

   fn compute_virtual_reserves(&mut self) -> Result<(), anyhow::Error> {
      let state = self
         .state()
         .v3_state()
         .ok_or_else(|| anyhow::anyhow!("Pool state has not been initialized"))?;

      let liquidity = state.liquidity;

      if liquidity == 0 {
         self.liquidity_amount0 = U256::ZERO;
         self.liquidity_amount1 = U256::ZERO;
         return Ok(());
      }

      let sqrt_price_x96 = state.sqrt_price;

      if sqrt_price_x96.is_zero() {
         return Err(anyhow::anyhow!("Invalid state: sqrt_price is zero"));
      }

      let liquidity_u256 = U256::from(liquidity);

      // virtual amount of token0
      let amount0 = (liquidity_u256 * Q96) / sqrt_price_x96;

      // virtual amount of token1
      let amount1 = (liquidity_u256 * sqrt_price_x96) / Q96;

     // let amnt0 = NumericValue::format_wei(amount0, self.currency0().decimals());
     // let amnt1 = NumericValue::format_wei(amount1, self.currency1().decimals());

      /* 
      eprintln!(
         "Pool {} / {} ({}) Virtual Reserves {} {} - {} {}",
         self.quote_currency().symbol(),
         self.base_currency().symbol(),
         self.fee().fee_percent(),
         self.currency0().symbol(),
         amnt0.format_abbreviated(),
         self.currency1().symbol(),
         amnt1.format_abbreviated()
      );
      */

      self.liquidity_amount0 = amount0;
      self.liquidity_amount1 = amount1;

      Ok(())
   }

   async fn update_state<P, N>(&mut self, client: P, block: Option<BlockId>) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let state = get_v4_pool_state(client, self, block).await?;
      self.set_state(state);
      self.compute_virtual_reserves()?;
      Ok(())
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let fee = self.fee.fee_percent();
      if fee > MAX_FEE {
         return Err(anyhow!("Pool Fee {} exceeds max fee {}", fee, MAX_FEE));
      }

      if self.hook_impacts_swap() {
         return Err(anyhow!("Unsupported Hook"));
      }

      let zero_for_one = self.zero_for_one_v4(currency_in);
      let fee = self.fee.fee();
      let state = self
         .state()
         .v3_state()
         .ok_or(anyhow!("State not initialized"))?;
      let amount_out = calculate_swap(state, fee, zero_for_one, amount_in)?;
      Ok(amount_out)
   }

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let fee = self.fee.fee_percent();
      if fee > MAX_FEE {
         return Err(anyhow!("Pool Fee {} exceeds max fee {}", fee, MAX_FEE));
      }

      if self.hook_impacts_swap() {
         return Err(anyhow!("Unsupported Hook"));
      }

      let zero_for_one = self.zero_for_one_v4(currency_in);
      let fee = self.fee.fee();
      let state = self
         .state_mut()
         .v3_state_mut()
         .ok_or(anyhow!("State not initialized"))?;
      let amount_out = calculate_swap_mut(state, fee, zero_for_one, amount_in)?;

      Ok(amount_out)
   }

   fn simulate_swap_result(
      &self,
      currency_in: &Currency,
      currency_out: &Currency,
      amount_in: NumericValue,
   ) -> Result<SwapResult, anyhow::Error> {
      let fee = self.fee.fee_percent();
      if fee > MAX_FEE {
         return Err(anyhow::anyhow!(
            "Pool Fee {} exceeds max fee {}",
            fee,
            MAX_FEE
         ));
      }

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
   use alloy_primitives::B256;
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

      let amount_in = NumericValue::parse_to_wei("10", base.decimals());
      let swap_result = pool
         .simulate_swap_result(base, quote, amount_in.clone())
         .unwrap();

      println!("=== V4 Swap Test ===");
      println!(
         "Ideal Output: {:.6} {}",
         swap_result.ideal_amount_out.formatted(),
         quote.symbol()
      );
      println!(
         "Swapped {} {} For {} {}",
         amount_in.formatted(),
         base.symbol(),
         swap_result.amount_out.formatted(),
         quote.symbol()
      );
      println!("With Price Impact: {:.4}%", swap_result.price_impact);
   }

   #[tokio::test]
   async fn test_virtual_reserves_uni_usdc() {
      let url = Url::parse("https://reth-ethereum.ithaca.xyz/rpc").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV4Pool::uni_usdc();
      pool.update_state(client.clone(), None).await.unwrap();

      let usdc_balance = pool.base_balance();
      let uni_balance = pool.quote_balance();

      println!("USDC Balance: {}", usdc_balance.formatted());
      println!("UNI Balance: {}", uni_balance.formatted());
   }

   #[tokio::test]
   async fn test_base_token_liquidity_eth_uni() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.compute_virtual_reserves().unwrap();

      let eth_balance = pool.base_balance();
      let uni_balance = pool.quote_balance();

      println!("ETH Balance: {}", eth_balance.formatted());
      println!("UNI Balance: {}", uni_balance.formatted());
   }

   #[tokio::test]
   async fn test_base_token_liquidity_usdc_usdt() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV4Pool::usdc_usdt();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.compute_virtual_reserves().unwrap();

      let (balance0, balance1) = pool.pool_balances();

      eprintln!(
         "{} {}",
         pool.currency0().symbol(),
         balance0.format_abbreviated()
      );
      eprintln!(
         "{} {}",
         pool.currency1().symbol(),
         balance1.format_abbreviated()
      );
   }

   #[tokio::test]
   async fn compute_reserves_dsync_eth() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV4Pool::dsync_eth();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.compute_virtual_reserves().unwrap();

      let (balance0, balance1) = pool.pool_balances();

      eprintln!(
         "{} {}",
         pool.currency0().symbol(),
         balance0.format_abbreviated()
      );
      eprintln!(
         "{} {}",
         pool.currency1().symbol(),
         balance1.format_abbreviated()
      );
   }

   #[tokio::test]
   async fn test_base_token_liquidity_usdc_wbtc() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV4Pool::usdc_wbtc();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.compute_virtual_reserves().unwrap();

      let (balance0, balance1) = pool.pool_balances();

      eprintln!(
         "{} {}",
         pool.currency0().symbol(),
         balance0.format_abbreviated()
      );
      eprintln!(
         "{} {}",
         pool.currency1().symbol(),
         balance1.format_abbreviated()
      );
   }

   #[tokio::test]
   async fn price_calculation() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();
      pool.compute_virtual_reserves().unwrap();

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
