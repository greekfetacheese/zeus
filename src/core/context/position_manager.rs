use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};


use serde::{Deserialize, Serialize};
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};

use zeus_eth::{
    alloy_contract::private::{Network, Provider},
    alloy_primitives::{Address, U256, B256},
    amm::{DexKind, FeeAmount, UniswapPool, AnyUniswapPool, uniswap_v3_math, uniswap::v3::calculate_liquidity_amounts},
    utils::{NumericValue, batch, price_feed::get_base_token_price},
    currency::Currency,
};


/// Represents a liquidity position on a Uniswap V3/V4 Pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
   pub chain_id: u64,
   pub owner: Address,
   pub dex: DexKind,
   /// The block which this position was created
   pub block: u64,
   pub timestamp: u64,
   /// Id of the position
   pub id: U256,
   /// Nonce for permits
   pub nonce: U256,
   /// Address that is approved for spending
   pub operator: Address,
   pub token0: Currency,
   pub token1: Currency,
   /// Fee tier of the pool
   pub fee: FeeAmount,
   /// Zero if its a V4 pool
   pub pool_address: Address,
   /// Zero if its a V3 pool
   pub pool_id: B256,
   pub tick_lower: i32,
   pub tick_upper: i32,
   pub liquidity: u128,
   pub fee_growth_inside0_last_x128: U256,
   pub fee_growth_inside1_last_x128: U256,
   /// Amount0 of token0
   pub amount0: NumericValue,
   /// Amount1 of token1
   pub amount1: NumericValue,
   /// Unclaimed fees
   pub tokens_owed0: NumericValue,
   /// Unclaimed fees
   pub tokens_owed1: NumericValue,

   pub apr: f64,
}

impl PartialEq for Position {
   fn eq(&self, other: &Self) -> bool {
      self.id == other.id
   }
}

impl Position {
   /// Update the amount0 and amount1 based on the given Pool state
   pub fn update_amounts(&mut self, pool: &AnyUniswapPool) -> Result<(), anyhow::Error> {
      if pool.dex_kind() != self.dex {
         return Err(anyhow::anyhow!("Dex kind mismatch"));
      }

      if pool.address() != self.pool_address {
         return Err(anyhow::anyhow!("Pool address mismatch"));
      }

      if pool.pool_id() != self.pool_id {
         return Err(anyhow::anyhow!("Pool ID mismatch"));
      }

      let state = pool.state().v3_state();
      if state.is_none() {
         return Err(anyhow::anyhow!("State not initialized"));
      }

      let state = state.unwrap();

      let sqrt_price_lower = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(self.tick_lower)?;
      let sqrt_price_upper = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(self.tick_upper)?;

      let (amount0, amount1) = calculate_liquidity_amounts(
         state.sqrt_price,
         sqrt_price_lower,
         sqrt_price_upper,
         self.liquidity,
      )?;

      self.amount0 = NumericValue::format_wei(amount0, self.token0.decimals());
      self.amount1 = NumericValue::format_wei(amount1, self.token1.decimals());

      Ok(())
   }
}
