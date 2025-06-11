use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, B256, Signed, U256, address,
   utils::{format_units, parse_units},
};
use alloy_rpc_types::BlockId;
use std::borrow::Cow;

use crate::uniswap::PoolKey;
use crate::{
   DexKind, minimum_liquidity,
   uniswap::{
      AnyUniswapPool, UniswapPool,
      state::{State, TickInfo, get_v3_pool_state},
      v4::FeeAmount,
   },
};
use abi::uniswap::v3;
use currency::{Currency, ERC20Token};
use uniswap_v3_math::tick_math::{MAX_TICK, MIN_TICK};
use utils::{NumericValue, batch, price_feed::get_base_token_price};

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

pub const FEE_TIERS: [u32; 4] = [100, 500, 3000, 10000];

/// Represents a Uniswap V3 Pool
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

   pub fn state_mut(&mut self) -> &mut State {
      &mut self.state
   }

   pub fn tick_to_word(&self, tick: i32, tick_spacing: i32) -> i32 {
      let mut compressed = tick / tick_spacing;
      if tick < 0 && tick % tick_spacing != 0 {
         compressed -= 1;
      }

      compressed >> 8
   }

   /// Make sure you have first called [UniswapV3Pool::update_state]
   pub async fn populate_tick_bitmaps<P, N>(
      &mut self,
      client: P,
      block: Option<BlockId>,
      concurrency: usize,
   ) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let mut state = self
         .state()
         .v3_or_v4_state()
         .ok_or_else(|| anyhow!("State not initialized"))?
         .clone();

      let current_word_pos = state.word_pos;
      let tick_spacing = self.fee().tick_spacing_i32();

      let min_compressed_tick = MIN_TICK / tick_spacing;
      let max_compressed_tick = MAX_TICK / tick_spacing;

      let min_word_pos = (min_compressed_tick >> 8) as i16;
      let max_word_pos = (max_compressed_tick >> 8) as i16;
      eprintln!("Current Word Pos: {}", current_word_pos);
      eprintln!("Min Word Pos: {}", min_word_pos);
      eprintln!("Max Word Pos: {}", max_word_pos);

      let mut all_words_pos = Vec::new();
      for word_pos in min_word_pos..=max_word_pos {
         if word_pos == current_word_pos {
            continue;
         }

         all_words_pos.push(word_pos);
      }

      const BATCH_SIZE: usize = 200;

      let semaphore = Arc::new(Semaphore::new(concurrency));
      let collected_bitmaps = Arc::new(Mutex::new(Vec::new()));

      let mut tasks: Vec<tokio::task::JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

      for word_pos in all_words_pos.chunks(BATCH_SIZE) {
         let client = client.clone();
         let semaphore = semaphore.clone();
         let collected_bitmaps = collected_bitmaps.clone();

         let pool_info = batch::TickBitmapFetchV3::PoolInfo {
            pool: self.address,
            WordPositions: word_pos.to_vec(),
         };

         let task = tokio::spawn(async move {
            let _permit = semaphore.acquire_owned().await.unwrap();
            let tick_bitmaps = batch::get_v3_pool_tick_bitmaps(client.clone(), pool_info, block).await?;
            collected_bitmaps.lock().await.extend(tick_bitmaps);
            Ok(())
         });
         tasks.push(task);
      }

      for task in tasks {
         if let Err(e) = task.await? {
            tracing::error!(target: "zeus_eth::amm::uniswap::v3::pool", "Error fetching V3 pool tick bitmaps: {:?}", e);
            eprintln!("Error fetching V3 pool tick bitmaps: {:?}", e);
         }
      }

      let tick_bitmaps = collected_bitmaps.lock().await;
      for tick_bitmap in tick_bitmaps.iter() {
         if tick_bitmap.bitmap != U256::ZERO {
            state
               .tick_bitmap
               .insert(tick_bitmap.wordPos, tick_bitmap.bitmap);
         }
      }

      self.set_state(State::V3(state));
      Ok(())
   }

   /// Make sure you have first called [UniswapV3Pool::update_state] and [UniswapV3Pool::populate_tick_bitmaps]
   pub async fn populate_ticks<P, N>(
      &mut self,
      client: P,
      block: Option<BlockId>,
      concurrency: usize,
   ) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let mut state = self
         .state()
         .v3_or_v4_state()
         .ok_or_else(|| anyhow!("State not initialized"))?
         .clone();

      let all_bitmaps = state.tick_bitmap.clone().into_iter().collect::<Vec<_>>();
      let tick_spacing = self.fee().tick_spacing_i32();

      let mut ticks_to_populate = Vec::new();
      for (word_pos, bitmap) in all_bitmaps {
         let ticks_in_word = crate::uniswap::state::decode_bitmap(word_pos, bitmap, tick_spacing);
         let ticks: Vec<Signed<24, 1>> = ticks_in_word
            .iter()
            .map(|t| t.to_string().parse())
            .collect::<Result<Vec<_>, _>>()?;
         ticks_to_populate.extend(ticks);
      }

      const BATCH_SIZE: usize = 50;
      let semaphore = Arc::new(Semaphore::new(concurrency));
      let collected_ticks = Arc::new(Mutex::new(Vec::new()));

      let mut tasks: Vec<tokio::task::JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

      for tick_batch in ticks_to_populate.chunks(BATCH_SIZE) {
         let client = client.clone();
         let semaphore = semaphore.clone();
         let collected_ticks = collected_ticks.clone();

         let pool_info = batch::TickDataFetchV3::PoolInfo {
            pool: self.address,
            ticks: tick_batch.to_vec(),
         };

         let task = tokio::spawn(async move {
            let _permit = semaphore.acquire_owned().await.unwrap();
            let tick_info = batch::get_v3_pool_ticks(client.clone(), pool_info, block).await?;
            collected_ticks.lock().await.extend(tick_info);
            Ok(())
         });
         tasks.push(task);
      }

      for task in tasks {
         if let Err(e) = task.await? {
            tracing::error!(target: "zeus_eth::amm::uniswap::v3::pool", "Error fetching V3 pool ticks: {:?}", e);
            eprintln!("Error fetching V3 pool ticks: {:?}", e);
         }
      }

      let ticks = collected_ticks.lock().await;
      for tick_info in ticks.iter() {
         let tick: i32 = tick_info.tick.to_string().parse()?;
         let info = TickInfo {
            fee_growth_outside_0_x128: tick_info.feeGrowthOutside0X128,
            fee_growth_outside_1_x128: tick_info.feeGrowthOutside1X128,
            liquidity_gross: tick_info.liquidityGross,
            liquidity_net: tick_info.liquidityNet,
            initialized: tick_info.initialized,
         };
         state.ticks.insert(tick, info);
      }

      self.set_state(State::v3(state));

      Ok(())
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

   fn hooks(&self) -> Address {
      Address::ZERO
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

   fn enough_liquidity(&self) -> bool {
      let threshold = minimum_liquidity(&self.base_token());
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

   fn get_pool_key(&self) -> Result<PoolKey, anyhow::Error> {
      bail!("Pool Key method only applies to V4");
   }

   fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error> {
      let zero_for_one = self.zero_for_one_v3(currency_in.to_erc20().address);
      let price = super::calculate_price(self, zero_for_one)?;
      Ok(price)
   }

   fn calculate_liquidity(&mut self) -> Result<(), anyhow::Error> {
      return Err(anyhow!("Not implemented"));
   }

   fn calculate_liquidity2(&mut self) -> Result<(), anyhow::Error> {
      return Err(anyhow!("Not implemented"));
   }

   async fn update_state<P, N>(&mut self, client: P, block: Option<BlockId>) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let (state, data) = get_v3_pool_state(client, self, block).await?;
      self.liquidity_amount0 = data.token0Balance;
      self.liquidity_amount1 = data.token1Balance;
      self.set_state(state);
      Ok(())
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let zero_for_one = self.zero_for_one_v3(currency_in.to_erc20().address);
      let fee = self.fee.fee();
      let state = self
         .state()
         .v3_or_v4_state()
         .ok_or(anyhow::anyhow!("State not initialized"))?;
      let amount_out = super::calculate_swap(state, fee, zero_for_one, amount_in)?;
      Ok(amount_out)
   }

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      let zero_for_one = self.zero_for_one_v3(currency_in.to_erc20().address);
      let fee = self.fee.fee();
      let mut state = self
         .state_mut()
         .v3_or_v4_state_mut()
         .ok_or(anyhow::anyhow!("State not initialized"))?;
      let amount_out = super::calculate_swap_mut(&mut state, fee, zero_for_one, amount_in)?;

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

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::utils::{format_units, parse_units};

   use alloy_provider::ProviderBuilder;
   use url::Url;

   #[tokio::test]
   async fn can_swap() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let mut pool = UniswapV3Pool::usdt_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      // Swap 1 USDT for UNI
      let base = pool.base_currency();
      let quote = pool.quote_currency();

      let amount_in = parse_units("100000", base.decimals())
         .unwrap()
         .get_absolute();
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

      let base_liq = pool.base_balance();
      println!("{} Liquidity: {}", base.symbol(), base_liq.formatted());
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
