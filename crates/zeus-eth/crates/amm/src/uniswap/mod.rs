pub mod router;
pub mod state;
pub mod v2;
pub mod v3;
pub mod v4;

use super::DexKind;
use abi::uniswap::v4::PoolKey;
use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, B256, U256};
use alloy_rpc_types::BlockId;
use currency::Currency;
use state::State;
use v4::FeeAmount;

pub use v2::pool::UniswapV2Pool;
pub use v3::pool::{FEE_TIERS, UniswapV3Pool};
pub use v4::pool::UniswapV4Pool;

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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AnyUniswapPool {
   V2(UniswapV2Pool),
   V3(UniswapV3Pool),
   V4(UniswapV4Pool),
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
         let p = UniswapV3Pool {
            chain_id: pool.chain_id(),
            address: pool.address(),
            fee: pool.fee(),
            currency0: pool.currency0().clone(),
            currency1: pool.currency1().clone(),
            dex: pool.dex_kind(),
            state: pool.state().clone(),
         };
         AnyUniswapPool::V3(p)
      } else if pool.dex_kind().is_v4() {
         let p = UniswapV4Pool {
            chain_id: pool.chain_id(),
            fee: pool.fee(),
            dex: pool.dex_kind(),
            currency0: pool.currency0().clone(),
            currency1: pool.currency1().clone(),
            state: pool.state().clone(),
            pool_key: pool.get_pool_key().unwrap(),
            pool_id: pool.pool_id(),
            hooks: Address::ZERO,
         };
         AnyUniswapPool::V4(p)
      } else {
         panic!("Unknown dex kind");
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

   fn pool_id(&self) -> B256 {
      match self {
         AnyUniswapPool::V4(pool) => pool.pool_id(),
         AnyUniswapPool::V2(pool) => pool.pool_id(),
         AnyUniswapPool::V3(pool) => pool.pool_id(),
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

   fn zero_for_one_v3(&self, token_in: Address) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.zero_for_one_v3(token_in),
         AnyUniswapPool::V3(pool) => pool.zero_for_one_v3(token_in),
         AnyUniswapPool::V4(pool) => pool.zero_for_one_v3(token_in),
      }
   }

   fn zero_for_one_v4(&self, currency_in: &Currency) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.zero_for_one_v4(currency_in),
         AnyUniswapPool::V3(pool) => pool.zero_for_one_v4(currency_in),
         AnyUniswapPool::V4(pool) => pool.zero_for_one_v4(currency_in),
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

   fn is_token0(&self, token: Address) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.is_token0(token),
         AnyUniswapPool::V3(pool) => pool.is_token0(token),
         AnyUniswapPool::V4(pool) => pool.is_token0(token),
      }
   }

   fn is_token1(&self, token: Address) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.is_token1(token),
         AnyUniswapPool::V3(pool) => pool.is_token1(token),
         AnyUniswapPool::V4(pool) => pool.is_token1(token),
      }
   }

   fn base_token_exists(&self) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.base_token_exists(),
         AnyUniswapPool::V3(pool) => pool.base_token_exists(),
         AnyUniswapPool::V4(pool) => pool.base_token_exists(),
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

   fn enough_liquidity(&self) -> bool {
      match self {
         AnyUniswapPool::V2(pool) => pool.enough_liquidity(),
         AnyUniswapPool::V3(pool) => pool.enough_liquidity(),
         AnyUniswapPool::V4(pool) => pool.enough_liquidity(),
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

   fn get_pool_key(&self) -> Result<PoolKey, anyhow::Error> {
      match self {
         AnyUniswapPool::V2(pool) => pool.get_pool_key(),
         AnyUniswapPool::V3(pool) => pool.get_pool_key(),
         AnyUniswapPool::V4(pool) => pool.get_pool_key(),
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
