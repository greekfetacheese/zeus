pub mod router;
pub mod v2;
pub mod v3;
pub mod v4;

use super::DexKind;
use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, B256, U256};
use alloy_rpc_types::BlockId;
use currency::Currency;
use v2::pool::PoolReserves;
use v3::pool::V3PoolState;
use v4::FeeAmount;
use abi::uniswap::v4::PoolKey;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum State {
   V2(PoolReserves),
   V3(V3PoolState),
   // Same as V3, just add it here to avoid any confusions
   V4(V3PoolState),
   None,
}

impl State {
   pub fn none() -> Self {
      Self::None
   }

   pub fn v2(reserves: PoolReserves) -> Self {
      Self::V2(reserves)
   }

   pub fn v3(state: V3PoolState) -> Self {
      Self::V3(state)
   }

   pub fn v4(state: V3PoolState) -> Self {
      Self::V4(state)
   }

   pub fn is_none(&self) -> bool {
      matches!(self, Self::None)
   }

   pub fn is_v2(&self) -> bool {
      matches!(self, Self::V2(_))
   }

   pub fn is_v3(&self) -> bool {
      matches!(self, Self::V3(_))
   }

   pub fn is_v4(&self) -> bool {
      matches!(self, Self::V4(_) | Self::V3(_))
   }

   pub fn v2_reserves(&self) -> Option<&PoolReserves> {
      match self {
         Self::V2(reserves) => Some(reserves),
         _ => None,
      }
   }

   pub fn v3_state(&self) -> Option<&V3PoolState> {
      match self {
         Self::V3(state) => Some(state),
         _ => None,
      }
   }

   pub fn v3_or_v4_state(&self) -> Option<&V3PoolState> {
      match self {
         Self::V3(state) => Some(state),
         Self::V4(state) => Some(state),
         _ => None,
      }
   }
}

pub trait UniswapPool {
   fn chain_id(&self) -> u64;

   /// For V4 pools this should return zero
   fn address(&self) -> Address;

   /// This applies only for V4 pools
   /// 
   /// For anything else it's always zero
   fn pool_id(&self) -> B256;

   fn fee(&self) -> FeeAmount;

   fn dex_kind(&self) -> DexKind;

   fn currency0(&self) -> &Currency;

   fn currency1(&self) -> &Currency;

   /// Zero for one is true if the token_in address equals the token0 address of the pool
   /// 
   /// This is V3 specific
   fn zero_for_one_v3(&self, token_in: Address) -> bool;

   /// Zero for one is true if currency_in equals the currency0 of the pool
   /// 
   /// This is V4 specific
   fn zero_for_one_v4(&self, currency_in: &Currency) -> bool;

   fn is_currency0(&self, currency: &Currency) -> bool;

   fn is_currency1(&self, currency: &Currency) -> bool;

   fn is_token0(&self, token: Address) -> bool;

   fn is_token1(&self, token: Address) -> bool;

   fn base_token_exists(&self) -> bool;

   fn state(&self) -> &State;

   fn set_state(&mut self, state: State);

   fn set_state_res(&mut self, state: State) -> Result<(), anyhow::Error>;

   /// Does this pool have enough liquidity
   fn enough_liquidity(&self) -> bool;

   /// Get the base currency of this pool
   fn base_currency(&self) -> &Currency;

   /// Get the quote currency of this pool
   fn quote_currency(&self) -> &Currency;

   /// Get the pool key
   /// 
   /// This is V4 specific
   fn get_pool_key(&self) -> Result<PoolKey, anyhow::Error>;

   #[allow(async_fn_in_trait)]
   async fn fetch_state<P, N>(
      client: P,
      pool: impl UniswapPool,
      block: Option<BlockId>,
   ) -> Result<State, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network;

   fn simulate_swap(&self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error>;

   fn simulate_swap_mut(&mut self, currency_in: &Currency, amount_in: U256) -> Result<U256, anyhow::Error>;

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
