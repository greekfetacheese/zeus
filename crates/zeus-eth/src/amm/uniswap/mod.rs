use crate::currency::Currency;
use crate::types::ChainId;
use crate::utils::{NumericValue, address_book};
use alloy_primitives::{
   Address, B256, U256,
   aliases::{I24, U24},
};
use anyhow::bail;

use alloy_contract::private::{Network, Provider};
use alloy_rpc_types::BlockId;

use crate::abi::uniswap::v4::PoolKey;
use serde::{Deserialize, Serialize};

pub use {v2::UniswapV2Pool, v3::UniswapV3Pool, v4::UniswapV4Pool};

pub mod consts;
pub mod state;
pub mod sync;
pub mod v2;
pub mod v3;
pub mod v4;

pub use state::State;

pub use uniswap_v3_math;

pub const FEE_TIERS: [u32; 4] = [100, 500, 3000, 10000];

/// The default factory enabled fee amounts, denominated in hundredths of bips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum FeeAmount {
   LOWEST = 100,
   LOW = 500,
   MEDIUM = 3000,
   HIGH = 10000,
   CUSTOM(u32),
}

impl FeeAmount {
   pub fn fee(&self) -> u32 {
      match self {
         Self::LOWEST => 100,
         Self::LOW => 500,
         Self::MEDIUM => 3000,
         Self::HIGH => 10000,
         Self::CUSTOM(fee) => *fee,
      }
   }

   pub fn fee_u24(&self) -> U24 {
      U24::from(self.fee())
   }

   /// Convert the fee to human readable format
   pub fn fee_percent(&self) -> f32 {
      self.fee() as f32 / 10_000.0
   }

   /// The default factory tick spacings by fee amount.
   pub fn tick_spacing(&self) -> I24 {
      match self {
         Self::LOWEST => I24::ONE,
         Self::LOW => I24::from_limbs([10]),
         Self::MEDIUM => I24::from_limbs([60]),
         Self::HIGH => I24::from_limbs([200]),
         Self::CUSTOM(fee) => {
            // Ensure tick_spacing is at least 1
            let calculated_spacing = *fee as i32 / 50;
            if calculated_spacing < 1 {
               I24::ONE
            } else {
               I24::from_limbs([calculated_spacing as u64])
            }
         }
      }
   }

   pub fn tick_spacing_i32(&self) -> i32 {
      match self {
         Self::LOWEST => 1,
         Self::LOW => 10,
         Self::MEDIUM => 60,
         Self::HIGH => 200,
         Self::CUSTOM(fee) => {
            let calculated_spacing = *fee as i32 / 50;
            if calculated_spacing < 1 {
               1
            } else {
               calculated_spacing
            }
         }
      }
   }
}

impl From<u32> for FeeAmount {
   fn from(fee: u32) -> Self {
      match fee {
         100 => Self::LOWEST,
         500 => Self::LOW,
         3000 => Self::MEDIUM,
         10000 => Self::HIGH,
         _ => Self::CUSTOM(fee),
      }
   }
}

/// Enum to define in which DEX a pool belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DexKind {
   UniswapV2,
   UniswapV3,
   UniswapV4,
   PancakeSwapV2,
   PancakeSwapV3,
}

impl DexKind {
   /// Get the main DexKind for the given chain
   ///
   /// Panics if the chain is not supported
   pub fn main_dexes(chain: u64) -> Vec<DexKind> {
      let chain = ChainId::new(chain).unwrap();
      match chain {
         ChainId::Ethereum => vec![DexKind::UniswapV2, DexKind::UniswapV3, DexKind::UniswapV4],
         ChainId::BinanceSmartChain => vec![DexKind::PancakeSwapV2, DexKind::PancakeSwapV3],
         ChainId::Base => vec![DexKind::UniswapV2, DexKind::UniswapV3, DexKind::UniswapV4],
         ChainId::Optimism => vec![DexKind::UniswapV3, DexKind::UniswapV4],
         ChainId::Arbitrum => vec![DexKind::UniswapV3, DexKind::UniswapV4],
      }
   }

   /// Get all possible DEX kinds based on the chain
   ///
   /// Panics if the chain is not supported
   pub fn all(chain: u64) -> Vec<DexKind> {
      let chain = ChainId::new(chain).unwrap();
      match chain {
         ChainId::Ethereum => vec![
            DexKind::UniswapV2,
            DexKind::UniswapV3,
            DexKind::PancakeSwapV2,
            DexKind::PancakeSwapV3,
         ],
         ChainId::BinanceSmartChain => vec![
            DexKind::PancakeSwapV3,
            DexKind::UniswapV2,
            DexKind::UniswapV3,
         ],
         ChainId::Base => vec![
            DexKind::UniswapV2,
            DexKind::UniswapV3,
            DexKind::PancakeSwapV3,
         ],
         ChainId::Optimism => vec![DexKind::UniswapV3],
         ChainId::Arbitrum => vec![
            DexKind::UniswapV2,
            DexKind::UniswapV3,
            DexKind::PancakeSwapV3,
         ],
      }
   }

   pub fn as_vec() -> Vec<DexKind> {
      vec![
         DexKind::UniswapV2,
         DexKind::UniswapV3,
         DexKind::UniswapV4,
         DexKind::PancakeSwapV2,
         DexKind::PancakeSwapV3,
      ]
   }

   /// Get the DexKind from the factory address
   pub fn from_factory(chain: u64, factory: Address) -> Result<Self, anyhow::Error> {
      let uniswap_v2 = address_book::uniswap_v2_factory(chain)?;
      let uniswap_v3 = address_book::uniswap_v3_factory(chain)?;
      let pancake_v2 = address_book::pancakeswap_v2_factory(chain)?;
      let pancake_v3 = address_book::pancakeswap_v3_factory(chain)?;

      if factory == uniswap_v2 {
         Ok(DexKind::UniswapV2)
      } else if factory == uniswap_v3 {
         Ok(DexKind::UniswapV3)
      } else if factory == pancake_v2 {
         Ok(DexKind::PancakeSwapV2)
      } else if factory == pancake_v3 {
         Ok(DexKind::PancakeSwapV3)
      } else {
         bail!("Unknown factory address: {:?}", factory);
      }
   }

   /// Return the factory address of the DEX
   pub fn factory(&self, chain: u64) -> Result<Address, anyhow::Error> {
      let addr = match self {
         DexKind::UniswapV2 => address_book::uniswap_v2_factory(chain)?,
         DexKind::UniswapV3 => address_book::uniswap_v3_factory(chain)?,
         DexKind::UniswapV4 => panic!("Uniswap V4 does not have a factory"),
         DexKind::PancakeSwapV2 => address_book::pancakeswap_v2_factory(chain)?,
         DexKind::PancakeSwapV3 => address_book::pancakeswap_v3_factory(chain)?,
      };

      Ok(addr)
   }

   /// Return the creation block of this Dex
   ///
   /// For V2 & V3 this is the block in which the factory was deployed
   ///
   /// For V4 is the block which the PoolManager contract was deployed
   pub fn creation_block(&self, chain: u64) -> Result<u64, anyhow::Error> {
      match self {
         DexKind::UniswapV2 => uniswap_v2_factory_creation_block(chain),
         DexKind::UniswapV3 => uniswap_v3_factory_creation_block(chain),
         DexKind::UniswapV4 => uniswap_v4_pool_manager_creation_block(chain),
         DexKind::PancakeSwapV2 => pancakeswap_v2_factory_creation_block(chain),
         DexKind::PancakeSwapV3 => pancakeswap_v3_factory_creation_block(chain),
      }
   }

   pub fn is_uniswap(&self) -> bool {
      matches!(
         self,
         DexKind::UniswapV2 | DexKind::UniswapV3 | DexKind::UniswapV4
      )
   }

   pub fn is_pancake(&self) -> bool {
      matches!(self, DexKind::PancakeSwapV2 | DexKind::PancakeSwapV3)
   }

   pub fn is_v2(&self) -> bool {
      matches!(self, DexKind::UniswapV2 | DexKind::PancakeSwapV2)
   }

   pub fn is_v3(&self) -> bool {
      matches!(self, DexKind::UniswapV3 | DexKind::PancakeSwapV3)
   }

   pub fn is_v4(&self) -> bool {
      matches!(self, DexKind::UniswapV4)
   }

   pub fn is_uniswap_v2(&self) -> bool {
      matches!(self, DexKind::UniswapV2)
   }

   pub fn is_uniswap_v3(&self) -> bool {
      matches!(self, DexKind::UniswapV3)
   }

   pub fn is_uniswap_v4(&self) -> bool {
      matches!(self, DexKind::UniswapV4)
   }

   pub fn is_pancakeswap_v2(&self) -> bool {
      matches!(self, DexKind::PancakeSwapV2)
   }

   pub fn is_pancakeswap_v3(&self) -> bool {
      matches!(self, DexKind::PancakeSwapV3)
   }

   pub fn as_str(&self) -> &'static str {
      match self {
         DexKind::UniswapV2 => "Uniswap V2",
         DexKind::UniswapV3 => "Uniswap V3",
         DexKind::UniswapV4 => "Uniswap V4",
         DexKind::PancakeSwapV2 => "PancakeSwap V2",
         DexKind::PancakeSwapV3 => "PancakeSwap V3",
      }
   }

   pub fn version_str(&self) -> &'static str {
      match self {
         DexKind::UniswapV2 => "V2",
         DexKind::UniswapV3 => "V3",
         DexKind::UniswapV4 => "V4",
         DexKind::PancakeSwapV2 => "V2",
         DexKind::PancakeSwapV3 => "V3",
      }
   }
}

#[derive(Debug, Clone)]
pub struct SwapResult {
   pub amount_in: NumericValue,
   pub amount_out: NumericValue,
   pub ideal_amount_out: NumericValue,
   pub price_impact: f64,
}

pub trait UniswapPool {
   fn chain_id(&self) -> u64;

   /// For V4 pools this should return zero
   fn address(&self) -> Address;

   /// Return the pool id
   ///
   /// This applies only for V4 pools
   fn id(&self) -> B256;

   /// Return the pool key
   ///
   /// This applies only for V4 pools
   fn key(&self) -> PoolKey;

   fn fee(&self) -> FeeAmount;

   fn dex_kind(&self) -> DexKind;

   fn currency0(&self) -> &Currency;

   fn currency1(&self) -> &Currency;

   fn zero_for_one(&self, currency_in: &Currency) -> bool;

   fn have(&self, currency: &Currency) -> bool;

   fn is_currency0(&self, currency: &Currency) -> bool;

   fn is_currency1(&self, currency: &Currency) -> bool;

   fn base_currency_exists(&self) -> bool;

   fn state(&self) -> &State;

   fn set_state(&mut self, state: State);

   fn set_state_res(&mut self, state: State) -> Result<(), anyhow::Error>;

   /// Pool Balances (Currency0, Currency1)
   fn pool_balances(&self) -> (NumericValue, NumericValue);

   /// Base Currency Pool Balance
   fn base_balance(&self) -> NumericValue;

   /// Quote Currency Pool Balance
   fn quote_balance(&self) -> NumericValue;

   /// Computes the virtual reserves of the pool
   fn compute_virtual_reserves(&mut self) -> Result<(), anyhow::Error>;

   /// Get the base currency of this pool
   fn base_currency(&self) -> &Currency;

   /// Get the quote currency of this pool
   fn quote_currency(&self) -> &Currency;

   /// Calculate the price of currency_in in terms of the other currency in the pool
   fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error>;

   /// This is V4 specific
   fn hooks(&self) -> Address;

   #[allow(async_fn_in_trait)]
   /// Update the state for this pool at the given block
   ///
   /// If `block` is `None`, the latest block is used
   async fn update_state<P, N>(&mut self, client: P, block: Option<BlockId>) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network;

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error>;

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error>;

   fn simulate_swap_result(
      &self,
      currency_in: &Currency,
      currency_out: &Currency,
      amount_in: NumericValue,
   ) -> Result<SwapResult, anyhow::Error>;

   /// Quote token USD price but we need to know the usd price of base token
   fn quote_price(&self, base_usd: f64) -> Result<f64, anyhow::Error>;

   /// Get the usd value of Base and Quote token at a given block
   ///
   /// If block is None, the latest block is used
   ///
   /// ## Returns
   ///
   /// - (base_price, quote_price)
   #[allow(async_fn_in_trait)]
   async fn tokens_price<P, N>(&self, client: P, block: Option<BlockId>) -> Result<(f64, f64), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum AnyUniswapPool {
   V2(UniswapV2Pool),
   V3(UniswapV3Pool),
   V4(UniswapV4Pool),
}

impl Default for AnyUniswapPool {
   fn default() -> Self {
      Self::V2(UniswapV2Pool::weth_uni())
   }
}

impl From<UniswapV2Pool> for AnyUniswapPool {
   fn from(pool: UniswapV2Pool) -> Self {
      Self::V2(pool)
   }
}

impl From<UniswapV3Pool> for AnyUniswapPool {
   fn from(pool: UniswapV3Pool) -> Self {
      Self::V3(pool)
   }
}

impl From<UniswapV4Pool> for AnyUniswapPool {
   fn from(pool: UniswapV4Pool) -> Self {
      Self::V4(pool)
   }
}

impl AnyUniswapPool {
   pub fn from_pool(pool: impl UniswapPool) -> Self {
      if pool.dex_kind().is_v2() {
         let p = UniswapV2Pool {
            chain_id: pool.chain_id(),
            address: pool.address(),
            currency0: pool.currency0().clone(),
            currency1: pool.currency1().clone(),
            fee: pool.fee(),
            dex: pool.dex_kind(),
            state: pool.state().clone(),
         };
         AnyUniswapPool::V2(p)
      } else if pool.dex_kind().is_v3() {
         let (amount0, amount1) = pool.pool_balances();
         let p = UniswapV3Pool {
            chain_id: pool.chain_id(),
            address: pool.address(),
            fee: pool.fee(),
            currency0: pool.currency0().clone(),
            currency1: pool.currency1().clone(),
            dex: pool.dex_kind(),
            state: pool.state().clone(),
            liquidity_amount0: amount0.wei(),
            liquidity_amount1: amount1.wei(),
         };
         AnyUniswapPool::V3(p)
      } else if pool.dex_kind().is_v4() {
         let (amount0, amount1) = pool.pool_balances();
         let p = UniswapV4Pool {
            chain_id: pool.chain_id(),
            fee: pool.fee(),
            dex: pool.dex_kind(),
            currency0: pool.currency0().clone(),
            currency1: pool.currency1().clone(),
            state: pool.state().clone(),
            hooks: pool.hooks(),
            liquidity_amount0: amount0.wei(),
            liquidity_amount1: amount1.wei(),
         };
         AnyUniswapPool::V4(p)
      } else {
         panic!("Unknown dex kind");
      }
   }

   pub fn v2_mut<F>(&mut self, f: F)
   where
      F: FnOnce(&mut UniswapV2Pool),
   {
      if let AnyUniswapPool::V2(pool) = self {
         f(pool);
      }
   }

   pub fn v3_mut<F>(&mut self, f: F)
   where
      F: FnOnce(&mut UniswapV3Pool),
   {
      if let AnyUniswapPool::V3(pool) = self {
         f(pool);
      }
   }

   pub fn v4_mut<F>(&mut self, f: F)
   where
      F: FnOnce(&mut UniswapV4Pool),
   {
      if let AnyUniswapPool::V4(pool) = self {
         f(pool);
      }
   }
}

impl UniswapPool for AnyUniswapPool {
   fn chain_id(&self) -> u64 {
      match self {
         AnyUniswapPool::V2(pool) => pool.chain_id(),
         AnyUniswapPool::V3(pool) => pool.chain_id(),
         AnyUniswapPool::V4(pool) => pool.chain_id(),
      }
   }

   fn address(&self) -> Address {
      match self {
         AnyUniswapPool::V2(pool) => pool.address(),
         AnyUniswapPool::V3(pool) => pool.address(),
         AnyUniswapPool::V4(pool) => pool.address(),
      }
   }

   fn id(&self) -> B256 {
      match self {
         AnyUniswapPool::V4(pool) => pool.id(),
         AnyUniswapPool::V2(pool) => pool.id(),
         AnyUniswapPool::V3(pool) => pool.id(),
      }
   }

   fn key(&self) -> PoolKey {
      match self {
         AnyUniswapPool::V2(pool) => pool.key(),
         AnyUniswapPool::V3(pool) => pool.key(),
         AnyUniswapPool::V4(pool) => pool.key(),
      }
   }

   fn hooks(&self) -> Address {
      match self {
         AnyUniswapPool::V4(pool) => pool.hooks(),
         AnyUniswapPool::V3(pool) => pool.hooks(),
         AnyUniswapPool::V2(pool) => pool.hooks(),
      }
   }

   fn fee(&self) -> FeeAmount {
      match self {
         AnyUniswapPool::V2(pool) => pool.fee(),
         AnyUniswapPool::V3(pool) => pool.fee(),
         AnyUniswapPool::V4(pool) => pool.fee(),
      }
   }

   fn dex_kind(&self) -> DexKind {
      match self {
         AnyUniswapPool::V2(pool) => pool.dex_kind(),
         AnyUniswapPool::V3(pool) => pool.dex_kind(),
         AnyUniswapPool::V4(pool) => pool.dex_kind(),
      }
   }

   fn have(&self, currency: &Currency) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.have(currency),
         AnyUniswapPool::V3(pool) => pool.have(currency),
         AnyUniswapPool::V4(pool) => pool.have(currency),
      }
   }

   fn currency0(&self) -> &Currency {
      match self {
         AnyUniswapPool::V2(pool) => pool.currency0(),
         AnyUniswapPool::V3(pool) => pool.currency0(),
         AnyUniswapPool::V4(pool) => pool.currency0(),
      }
   }

   fn currency1(&self) -> &Currency {
      match self {
         AnyUniswapPool::V2(pool) => pool.currency1(),
         AnyUniswapPool::V3(pool) => pool.currency1(),
         AnyUniswapPool::V4(pool) => pool.currency1(),
      }
   }

   fn zero_for_one(&self, currency_in: &Currency) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.zero_for_one(currency_in),
         AnyUniswapPool::V3(pool) => pool.zero_for_one(currency_in),
         AnyUniswapPool::V4(pool) => pool.zero_for_one(currency_in),
      }
   }

   fn is_currency0(&self, currency: &Currency) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.is_currency0(currency),
         AnyUniswapPool::V3(pool) => pool.is_currency0(currency),
         AnyUniswapPool::V4(pool) => pool.is_currency0(currency),
      }
   }

   fn is_currency1(&self, currency: &Currency) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.is_currency1(currency),
         AnyUniswapPool::V3(pool) => pool.is_currency1(currency),
         AnyUniswapPool::V4(pool) => pool.is_currency1(currency),
      }
   }

   fn base_currency_exists(&self) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.base_currency_exists(),
         AnyUniswapPool::V3(pool) => pool.base_currency_exists(),
         AnyUniswapPool::V4(pool) => pool.base_currency_exists(),
      }
   }

   fn state(&self) -> &State {
      match self {
         AnyUniswapPool::V2(pool) => pool.state(),
         AnyUniswapPool::V3(pool) => pool.state(),
         AnyUniswapPool::V4(pool) => pool.state(),
      }
   }

   fn set_state(&mut self, state: State) {
      match self {
         AnyUniswapPool::V2(pool) => pool.set_state(state),
         AnyUniswapPool::V3(pool) => pool.set_state(state),
         AnyUniswapPool::V4(pool) => pool.set_state(state),
      }
   }

   fn set_state_res(&mut self, state: State) -> Result<(), anyhow::Error> {
      match self {
         AnyUniswapPool::V2(pool) => pool.set_state_res(state),
         AnyUniswapPool::V3(pool) => pool.set_state_res(state),
         AnyUniswapPool::V4(pool) => pool.set_state_res(state),
      }
   }

   fn base_currency(&self) -> &Currency {
      match self {
         AnyUniswapPool::V2(pool) => pool.base_currency(),
         AnyUniswapPool::V3(pool) => pool.base_currency(),
         AnyUniswapPool::V4(pool) => pool.base_currency(),
      }
   }

   fn quote_currency(&self) -> &Currency {
      match self {
         AnyUniswapPool::V2(pool) => pool.quote_currency(),
         AnyUniswapPool::V3(pool) => pool.quote_currency(),
         AnyUniswapPool::V4(pool) => pool.quote_currency(),
      }
   }

   fn pool_balances(&self) -> (NumericValue, NumericValue) {
      match self {
         AnyUniswapPool::V2(pool) => pool.pool_balances(),
         AnyUniswapPool::V3(pool) => pool.pool_balances(),
         AnyUniswapPool::V4(pool) => pool.pool_balances(),
      }
   }

   fn base_balance(&self) -> NumericValue {
      match self {
         AnyUniswapPool::V2(pool) => pool.base_balance(),
         AnyUniswapPool::V3(pool) => pool.base_balance(),
         AnyUniswapPool::V4(pool) => pool.base_balance(),
      }
   }

   fn quote_balance(&self) -> NumericValue {
      match self {
         AnyUniswapPool::V2(pool) => pool.quote_balance(),
         AnyUniswapPool::V3(pool) => pool.quote_balance(),
         AnyUniswapPool::V4(pool) => pool.quote_balance(),
      }
   }

   fn calculate_price(&self, currency_in: &Currency) -> Result<f64, anyhow::Error> {
      match self {
         AnyUniswapPool::V2(pool) => pool.calculate_price(currency_in),
         AnyUniswapPool::V3(pool) => pool.calculate_price(currency_in),
         AnyUniswapPool::V4(pool) => pool.calculate_price(currency_in),
      }
   }

   fn compute_virtual_reserves(&mut self) -> Result<(), anyhow::Error> {
      match self {
         AnyUniswapPool::V2(pool) => pool.compute_virtual_reserves(),
         AnyUniswapPool::V3(pool) => pool.compute_virtual_reserves(),
         AnyUniswapPool::V4(pool) => pool.compute_virtual_reserves(),
      }
   }

   async fn update_state<P, N>(&mut self, client: P, block: Option<BlockId>) -> Result<(), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      match self {
         AnyUniswapPool::V2(pool) => pool.update_state(client, block).await,
         AnyUniswapPool::V3(pool) => pool.update_state(client, block).await,
         AnyUniswapPool::V4(pool) => pool.update_state(client, block).await,
      }
   }

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      match self {
         AnyUniswapPool::V2(pool) => pool.simulate_swap(currency_in, amount_in),
         AnyUniswapPool::V3(pool) => pool.simulate_swap(currency_in, amount_in),
         AnyUniswapPool::V4(pool) => pool.simulate_swap(currency_in, amount_in),
      }
   }

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error> {
      match self {
         AnyUniswapPool::V2(pool) => pool.simulate_swap_mut(currency_in, amount_in),
         AnyUniswapPool::V3(pool) => pool.simulate_swap_mut(currency_in, amount_in),
         AnyUniswapPool::V4(pool) => pool.simulate_swap_mut(currency_in, amount_in),
      }
   }

   fn simulate_swap_result(
      &self,
      currency_in: &Currency,
      currency_out: &Currency,
      amount_in: NumericValue,
   ) -> Result<SwapResult, anyhow::Error> {
      match self {
         AnyUniswapPool::V2(pool) => pool.simulate_swap_result(currency_in, currency_out, amount_in),
         AnyUniswapPool::V3(pool) => pool.simulate_swap_result(currency_in, currency_out, amount_in),
         AnyUniswapPool::V4(pool) => pool.simulate_swap_result(currency_in, currency_out, amount_in),
      }
   }

   fn quote_price(&self, base_usd: f64) -> Result<f64, anyhow::Error> {
      match self {
         AnyUniswapPool::V2(pool) => pool.quote_price(base_usd),
         AnyUniswapPool::V3(pool) => pool.quote_price(base_usd),
         AnyUniswapPool::V4(pool) => pool.quote_price(base_usd),
      }
   }

   async fn tokens_price<P, N>(&self, client: P, block: Option<BlockId>) -> Result<(f64, f64), anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      match self {
         AnyUniswapPool::V2(pool) => pool.tokens_price(client, block).await,
         AnyUniswapPool::V3(pool) => pool.tokens_price(client, block).await,
         AnyUniswapPool::V4(pool) => pool.tokens_price(client, block).await,
      }
   }
}

/// Uniswap V3 NFT Position Manager contract creation block
pub fn nft_position_manager_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum => Ok(12369651),
      ChainId::Optimism => Ok(0), // Genesis
      ChainId::BinanceSmartChain => Ok(26324045),
      ChainId::Base => Ok(1371714),
      ChainId::Arbitrum => Ok(173),
   }
}

fn uniswap_v2_factory_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum => Ok(10000835),
      ChainId::Optimism => Ok(112197986),
      ChainId::BinanceSmartChain => Ok(33496018),
      ChainId::Base => Ok(6601915),
      ChainId::Arbitrum => Ok(150442611),
   }
}

fn uniswap_v3_factory_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum => Ok(12369621),
      ChainId::Optimism => Ok(0), // Genesis
      ChainId::BinanceSmartChain => Ok(26324014),
      ChainId::Base => Ok(1371680),
      ChainId::Arbitrum => Ok(165),
   }
}

fn uniswap_v4_pool_manager_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum => Ok(21688329),
      ChainId::Optimism => Ok(130947675),
      ChainId::BinanceSmartChain => Ok(45970610),
      ChainId::Base => Ok(25350988),
      ChainId::Arbitrum => Ok(297842872),
   }
}

fn pancakeswap_v2_factory_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum => Ok(15614590),
      ChainId::Optimism => bail!("PancakeSwap V2 is not available on Optimism"),
      ChainId::BinanceSmartChain => Ok(6809737),
      ChainId::Base => Ok(2910387),
      ChainId::Arbitrum => Ok(101022992),
   }
}

fn pancakeswap_v3_factory_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum => Ok(16950686),
      ChainId::Optimism => bail!("PancakeSwap V3 is not available on Optimism"),
      ChainId::BinanceSmartChain => Ok(26956207),
      ChainId::Base => Ok(2912007),
      ChainId::Arbitrum => Ok(101028949),
   }
}
