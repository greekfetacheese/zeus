pub mod pool;
pub mod position;

use alloy_primitives::{Address, I256, U256};

use super::UniswapPool;
use crate::consts::{Q96, Q128, U256_1};
use crate::uniswap::state::{TickInfo, V3PoolState};
use std::cmp::Ordering;
use uniswap_v3_math::{full_math::mul_div, sqrt_price_math, tick_math::*};

use anyhow::anyhow;


/// Calculate the tick from a given price
pub fn get_tick_from_price(price: f64) -> i32 {
   if price == 0.0 {
      return 0;
   }
   
   let sqrt_price = price.sqrt();

   let tick = (sqrt_price.ln() / (1.0001_f64).sqrt().ln()).round() as i32;

   tick
}


/// Calculates the price from a given tick, adjusting for token decimals.
///
/// The price returned is the price of token0 in terms of token1 (i.e., how much token1 you get for 1 token0).
///
/// # Arguments
///
/// * `tick` - The tick to convert to a price.
/// * `token0_decimals` - The number of decimals for the pool's token0.
/// * `token1_decimals` - The number of decimals for the pool's token1.
///
/// # Returns
///
/// * `f64` - The calculated price.
pub fn get_price_from_tick(tick: i32, token0_decimals: u8, token1_decimals: u8) -> f64 {
    let base_price = 1.0001_f64.powi(tick);
    let decimal_factor = 10_f64.powi(token0_decimals as i32 - token1_decimals as i32);
    
    base_price * decimal_factor
}



#[derive(Debug, Clone, PartialEq)]
pub struct Position {
   pub owner: Address,
   pub tick_lower: i32,
   pub tick_upper: i32,
   pub liquidity: u128,
   pub fee_growth_inside_0_last_x128: U256,
   pub fee_growth_inside_1_last_x128: U256,
   pub tokens_owed_0: U256,
   pub tokens_owed_1: U256,
}

impl Position {
   /// Create a new Position with the given state of a pool
   ///
   /// # Arguments
   ///
   /// * `pool_state` - The state of the pool
   /// * `lower_tick` - The lower tick of the position
   /// * `upper_tick` - The upper tick of the position
   /// * `liquidity` - The liquidity of the position
   pub fn new(
      pool_state: &V3PoolState,
      lower_tick: i32,
      upper_tick: i32,
      liquidity: u128,
   ) -> Result<Self, anyhow::Error> {
      let (fee_growth_inside_0, fee_growth_inside_1) = get_fee_growth_inside(pool_state, lower_tick, upper_tick);

      Ok(Self {
         owner: Address::ZERO,
         tick_lower: lower_tick,
         tick_upper: upper_tick,
         liquidity,
         fee_growth_inside_0_last_x128: fee_growth_inside_0,
         fee_growth_inside_1_last_x128: fee_growth_inside_1,
         tokens_owed_0: U256::ZERO,
         tokens_owed_1: U256::ZERO,
      })
   }

   /// Calculates the fees earned since the last update and updates the position state.
   pub fn update(&mut self, pool_state: &V3PoolState) -> Result<(U256, U256), anyhow::Error> {
      let (fee_growth_inside_0, fee_growth_inside_1) =
         get_fee_growth_inside(pool_state, self.tick_lower, self.tick_upper);

      // Calculate the difference in fee growth since the last update
      let fees_owed_0 = mul_div(
         fee_growth_inside_0 - self.fee_growth_inside_0_last_x128,
         U256::from(self.liquidity),
         Q128,
      )?;

      let fees_owed_1 = mul_div(
         fee_growth_inside_1 - self.fee_growth_inside_1_last_x128,
         U256::from(self.liquidity),
         Q128,
      )?;

      // Update the position's state
      self.tokens_owed_0 += fees_owed_0;
      self.tokens_owed_1 += fees_owed_1;

      self.fee_growth_inside_0_last_x128 = fee_growth_inside_0;
      self.fee_growth_inside_1_last_x128 = fee_growth_inside_1;

      Ok((fees_owed_0, fees_owed_1))
   }
}

#[derive(Default)]
pub struct CurrentState {
   pub amount_specified_remaining: I256,
   pub amount_calculated: I256,
   pub sqrt_price_x_96: U256,
   pub tick: i32,
   pub liquidity: u128,
}

#[derive(Default)]
struct StepComputations {
   pub sqrt_price_start_x_96: U256,
   pub tick_next: i32,
   pub initialized: bool,
   pub sqrt_price_next_x96: U256,
   pub amount_in: U256,
   pub amount_out: U256,
   pub fee_amount: U256,
}

pub fn calculate_swap(
   state: &V3PoolState,
   fee: u32,
   zero_for_one: bool,
   amount_in: U256,
) -> Result<U256, anyhow::Error> {
   if amount_in.is_zero() {
      return Ok(U256::ZERO);
   }

   // Set sqrt_price_limit_x_96 to the max or min sqrt price in the pool depending on zero_for_one
   let sqrt_price_limit_x_96 = if zero_for_one {
      MIN_SQRT_RATIO + U256_1
   } else {
      MAX_SQRT_RATIO - U256_1
   };

   // Initialize a mutable state state struct to hold the dynamic simulated state of the pool
   let mut current_state = CurrentState {
      sqrt_price_x_96: state.sqrt_price.clone(), //Active price on the pool
      amount_calculated: I256::ZERO,             //Amount of token_out that has been calculated
      amount_specified_remaining: I256::from_raw(amount_in), //Amount of token_in that has not been swapped
      tick: state.tick.clone(),                  //Current i24 tick of the pool
      liquidity: state.liquidity.clone(),        //Current available liquidity in the tick range
   };

   // Keep track of the fee growth for the token being swapped in
   let mut fee_growth_global = if zero_for_one {
      state.fee_growth_global_0_x128
   } else {
      state.fee_growth_global_1_x128
   };

   while current_state.amount_specified_remaining != I256::ZERO
      && current_state.sqrt_price_x_96 != sqrt_price_limit_x_96
   {
      // Initialize a new step struct to hold the dynamic state of the pool at each step
      let mut step = StepComputations {
         // Set the sqrt_price_start_x_96 to the current sqrt_price_x_96
         sqrt_price_start_x_96: current_state.sqrt_price_x_96,
         ..Default::default()
      };

      // Get the next tick from the current tick
      (step.tick_next, step.initialized) = uniswap_v3_math::tick_bitmap::next_initialized_tick_within_one_word(
         &state.tick_bitmap,
         current_state.tick,
         state.tick_spacing,
         zero_for_one,
      )?;

      // ensure that we do not overshoot the min/max tick, as the tick bitmap is not aware of these bounds
      step.tick_next = step.tick_next.clamp(MIN_TICK, MAX_TICK);

      // Get the next sqrt price from the input amount
      step.sqrt_price_next_x96 = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(step.tick_next)?;

      // Target spot price
      let swap_target_sqrt_ratio = if zero_for_one {
         if step.sqrt_price_next_x96 < sqrt_price_limit_x_96 {
            sqrt_price_limit_x_96
         } else {
            step.sqrt_price_next_x96
         }
      } else if step.sqrt_price_next_x96 > sqrt_price_limit_x_96 {
         sqrt_price_limit_x_96
      } else {
         step.sqrt_price_next_x96
      };

      // Compute swap step and update the current state
      (
         current_state.sqrt_price_x_96,
         step.amount_in,
         step.amount_out,
         step.fee_amount,
      ) = uniswap_v3_math::swap_math::compute_swap_step(
         current_state.sqrt_price_x_96,
         swap_target_sqrt_ratio,
         current_state.liquidity,
         current_state.amount_specified_remaining,
         fee,
      )?;

      // Decrement the amount remaining to be swapped and amount received from the step
      current_state.amount_specified_remaining = current_state
         .amount_specified_remaining
         .overflowing_sub(I256::from_raw(
            step.amount_in.overflowing_add(step.fee_amount).0,
         ))
         .0;

      current_state.amount_calculated -= I256::from_raw(step.amount_out);

      // Update the global fee growth
      if current_state.liquidity > 0 {
         fee_growth_global += mul_div(step.fee_amount, Q128, U256::from(current_state.liquidity))?;
      }

      // If the price moved all the way to the next price, recompute the liquidity change for the next iteration
      if current_state.sqrt_price_x_96 == step.sqrt_price_next_x96 {
         if step.initialized {
            let mut liquidity_net = if let Some(info) = state.ticks.get(&step.tick_next) {
               info.liquidity_net
            } else {
               0
            };

            // we are on a tick boundary, and the next tick is initialized, so we must charge a protocol fee
            if zero_for_one {
               liquidity_net = -liquidity_net;
            }

            current_state.liquidity = if liquidity_net < 0 {
               if current_state.liquidity < (-liquidity_net as u128) {
                  return Err(anyhow::anyhow!("Liquidity underflow"));
               } else {
                  current_state.liquidity - (-liquidity_net as u128)
               }
            } else {
               current_state.liquidity + (liquidity_net as u128)
            };
         }
         // Increment the current tick
         current_state.tick = if zero_for_one {
            step.tick_next.wrapping_sub(1)
         } else {
            step.tick_next
         };
         // If the current_state sqrt price is not equal to the step sqrt price, then we are not on the same tick.
         // Update the current_state.tick to the tick at the current_state.sqrt_price_x_96
      } else if current_state.sqrt_price_x_96 != step.sqrt_price_start_x_96 {
         current_state.tick = uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio(current_state.sqrt_price_x_96)?;
      }
   }

   let amount_out = (-current_state.amount_calculated).into_raw();

   Ok(amount_out)
}

pub fn calculate_swap_mut(
   state: &mut V3PoolState,
   fee: u32,
   zero_for_one: bool,
   amount_in: U256,
) -> Result<U256, anyhow::Error> {
   if amount_in.is_zero() {
      return Ok(U256::ZERO);
   }

   let mut fee_growth_global_during_swap = if zero_for_one {
      state.fee_growth_global_0_x128
   } else {
      state.fee_growth_global_1_x128
   };

   let sqrt_price_limit_x_96 = if zero_for_one {
      MIN_SQRT_RATIO + U256_1
   } else {
      MAX_SQRT_RATIO - U256_1
   };

   let mut amount_specified_remaining = I256::from_raw(amount_in);
   let mut amount_calculated = I256::ZERO;

   while amount_specified_remaining != I256::ZERO && state.sqrt_price != sqrt_price_limit_x_96 {
      let mut step = StepComputations::default();
      step.sqrt_price_start_x_96 = state.sqrt_price;

      (step.tick_next, step.initialized) = uniswap_v3_math::tick_bitmap::next_initialized_tick_within_one_word(
         &state.tick_bitmap,
         state.tick,
         state.tick_spacing,
         zero_for_one,
      )?;

      step.tick_next = step.tick_next.clamp(MIN_TICK, MAX_TICK);
      step.sqrt_price_next_x96 = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(step.tick_next)?;

      // Target spot price
      let swap_target_sqrt_ratio = if zero_for_one {
         if step.sqrt_price_next_x96 < sqrt_price_limit_x_96 {
            sqrt_price_limit_x_96
         } else {
            step.sqrt_price_next_x96
         }
      } else if step.sqrt_price_next_x96 > sqrt_price_limit_x_96 {
         sqrt_price_limit_x_96
      } else {
         step.sqrt_price_next_x96
      };

      (
         state.sqrt_price,
         step.amount_in,
         step.amount_out,
         step.fee_amount,
      ) = uniswap_v3_math::swap_math::compute_swap_step(
         step.sqrt_price_start_x_96,
         swap_target_sqrt_ratio,
         state.liquidity,
         amount_specified_remaining,
         fee,
      )?;

      amount_specified_remaining = amount_specified_remaining
         .overflowing_sub(I256::from_raw(
            step.amount_in.overflowing_add(step.fee_amount).0,
         ))
         .0;
      amount_calculated -= I256::from_raw(step.amount_out);

      // Update the LOCAL fee growth variable.
      if state.liquidity > 0 {
         fee_growth_global_during_swap += mul_div(step.fee_amount, Q128, U256::from(state.liquidity))?;
      }

      if state.sqrt_price == step.sqrt_price_next_x96 {
         if step.initialized {
            if let Some(crossed_tick_info) = state.ticks.get_mut(&step.tick_next) {
               let (fee_growth_global_0_for_cross, fee_growth_global_1_for_cross) = if zero_for_one {
                  (
                     fee_growth_global_during_swap,
                     state.fee_growth_global_1_x128,
                  )
               } else {
                  (
                     state.fee_growth_global_0_x128,
                     fee_growth_global_during_swap,
                  )
               };

               crossed_tick_info.fee_growth_outside_0_x128 =
                  fee_growth_global_0_for_cross - crossed_tick_info.fee_growth_outside_0_x128;
               crossed_tick_info.fee_growth_outside_1_x128 =
                  fee_growth_global_1_for_cross - crossed_tick_info.fee_growth_outside_1_x128;
            }

            let liquidity_net = if let Some(info) = state.ticks.get(&step.tick_next) {
               if zero_for_one {
                  -info.liquidity_net
               } else {
                  info.liquidity_net
               }
            } else {
               0
            };

            state.liquidity = if liquidity_net < 0 {
               state
                  .liquidity
                  .checked_sub((-liquidity_net) as u128)
                  .ok_or(anyhow!("Liquidity underflow"))?
            } else {
               state.liquidity + (liquidity_net as u128)
            };
         }
         state.tick = if zero_for_one {
            step.tick_next - 1
         } else {
            step.tick_next
         };
      } else if state.sqrt_price != step.sqrt_price_start_x_96 {
         state.tick = uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio(state.sqrt_price)?;
      }
   }

   if zero_for_one {
      state.fee_growth_global_0_x128 = fee_growth_global_during_swap;
   } else {
      state.fee_growth_global_1_x128 = fee_growth_global_during_swap;
   }

   let amount_out = (-amount_calculated).into_raw();
   Ok(amount_out)
}

pub fn calculate_price(pool: &impl UniswapPool, zero_for_one: bool) -> Result<f64, anyhow::Error> {
   let state = pool
      .state()
      .v3_state()
      .ok_or_else(|| anyhow::anyhow!("State not initialized"))?;

   let tick = uniswap_v3_math::tick_math::get_tick_at_sqrt_ratio(state.sqrt_price)?;
   let shift = (pool.currency0().decimals() as i8) - (pool.currency1().decimals() as i8);

   let price = match shift.cmp(&0) {
      Ordering::Less => (1.0001_f64).powi(tick) / (10_f64).powi(-shift as i32),
      Ordering::Greater => (1.0001_f64).powi(tick) * (10_f64).powi(shift as i32),
      Ordering::Equal => (1.0001_f64).powi(tick),
   };

   if zero_for_one {
      Ok(price)
   } else {
      Ok(1.0 / price)
   }
}




/// Computes the maximum amount of liquidity received for a given amount of token0, token1, the current
/// pool prices and the prices at the tick boundaries
///
/// # Arguments
///
/// * `sqrt_price_x96` - The current pool price
/// * `sqrt_ratio_ax96` - The price at the lower tick boundary
/// * `sqrt_ratio_bx96` - The price at the upper tick boundary
/// * `amount0_desired` - The amount of token0 desired by the user
/// * `amount1_desired` - The amount of token1 desired by the user
///
/// # Returns
///
/// * `liquidity` - The maximum amount of liquidity received
///
/// https://github.com/Uniswap/v3-periphery/blob/main/contracts/libraries/LiquidityAmounts.sol
pub fn get_liquidity_for_amounts(
   sqrt_price_x96: U256,
   mut sqrt_ratio_ax96: U256,
   mut sqrt_ratio_bx96: U256,
   amount0_desired: U256,
   amount1_desired: U256,
) -> Result<u128, anyhow::Error> {
   let liquidity: u128;

   if sqrt_ratio_ax96 > sqrt_ratio_bx96 {
      (sqrt_ratio_ax96, sqrt_ratio_bx96) = (sqrt_ratio_bx96, sqrt_ratio_ax96)
   }

   if sqrt_price_x96 <= sqrt_ratio_ax96 {
      liquidity = get_liquidity_for_amount0(sqrt_ratio_ax96, sqrt_ratio_bx96, amount0_desired)?;
   } else if sqrt_price_x96 < sqrt_ratio_bx96 {
      let liquidity0 = get_liquidity_for_amount0(sqrt_ratio_ax96, sqrt_ratio_bx96, amount0_desired)?;
      let liquidity1 = get_liquidity_for_amount1(sqrt_ratio_ax96, sqrt_ratio_bx96, amount1_desired)?;
      liquidity = liquidity0.min(liquidity1);
   } else {
      liquidity = get_liquidity_for_amount1(sqrt_ratio_ax96, sqrt_ratio_bx96, amount1_desired)?;
   }

   Ok(liquidity)
}

/// Computes the amount of liquidity received for a given amount of token0 and price range
///
/// # Arguments
///
/// * `sqrt_ratio_ax96` - A sqrt price representing the first tick boundary
/// * `sqrt_ratio_bx96` - A sqrt price representing the second tick boundary
/// * `amount0` - The amount0 being sent in
///
/// # Returns
///
/// * `liquidity` - The amount of liquidity received
///
/// https://github.com/Uniswap/v3-periphery/blob/main/contracts/libraries/LiquidityAmounts.sol
fn get_liquidity_for_amount0(
   mut sqrt_ratio_ax96: U256,
   mut sqrt_ratio_bx96: U256,
   amount0: U256,
) -> Result<u128, anyhow::Error> {
   if sqrt_ratio_ax96 > sqrt_ratio_bx96 {
      (sqrt_ratio_ax96, sqrt_ratio_bx96) = (sqrt_ratio_bx96, sqrt_ratio_ax96)
   }

   let intermidiate = mul_div(sqrt_ratio_ax96, sqrt_ratio_bx96, Q96)?;
   let liquidity = mul_div(amount0, intermidiate, sqrt_ratio_bx96 - sqrt_ratio_ax96)?;
   let liquidity: u128 = liquidity.to_string().parse()?;

   Ok(liquidity)
}

/// Computes the amount of liquidity received for a given amount of token1 and price range
///
/// # Arguments
///
/// * `sqrt_ratio_ax96` - A sqrt price representing the first tick boundary
/// * `sqrt_ratio_bx96` - A sqrt price representing the second tick boundary
/// * `amount1` - The amount1 being sent in
///
/// # Returns
///
/// * `liquidity` - The amount of liquidity received
///
/// https://github.com/Uniswap/v3-periphery/blob/main/contracts/libraries/LiquidityAmounts.sol
fn get_liquidity_for_amount1(
   mut sqrt_ratio_ax96: U256,
   mut sqrt_ratio_bx96: U256,
   amount1: U256,
) -> Result<u128, anyhow::Error> {
   if sqrt_ratio_ax96 > sqrt_ratio_bx96 {
      (sqrt_ratio_ax96, sqrt_ratio_bx96) = (sqrt_ratio_bx96, sqrt_ratio_ax96)
   }

   let liquidity = mul_div(amount1, Q96, sqrt_ratio_bx96 - sqrt_ratio_ax96)?;
   let liquidity: u128 = liquidity.to_string().parse()?;

   Ok(liquidity)
}



/// Calculates the liquidity needed
/// 
/// # Arguments
/// 
/// * `sqrt_ratio_current_x96` - The current price of the pool
/// * `sqrt_ratio_a_x96` - The sqrt price at the lower tick boundary
/// * `sqrt_ratio_b_x96` - The sqrt price at the upper tick boundary
/// * `amount_desired` - The amount of one of the tokens to deposit
/// * `is_token0` - Is amount_desired for token0?
/// 
/// # Returns
/// 
/// * `u128` - The liquidity amount
pub fn calculate_liquidity_needed(
    sqrt_ratio_current_x96: U256,
    sqrt_ratio_a_x96: U256,
    sqrt_ratio_b_x96: U256,
    amount_desired: U256,
    is_token0: bool,     
) -> Result<u128, anyhow::Error> {
   if amount_desired == U256::ZERO {
      return Ok(0);
   }

    if is_token0 {
        if sqrt_ratio_current_x96 <= sqrt_ratio_a_x96 {
            // Price is below the range, position is fully in token0
            get_liquidity_for_amount0(sqrt_ratio_a_x96, sqrt_ratio_b_x96, amount_desired)
        } else if sqrt_ratio_current_x96 < sqrt_ratio_b_x96 {
            // Price is in the range
            get_liquidity_for_amount0(sqrt_ratio_current_x96, sqrt_ratio_b_x96, amount_desired)
        } else {
            // Price is above the range, position is fully in token1, so no token0 is needed.
            Ok(0)
        }
    } else {
        if sqrt_ratio_current_x96 <= sqrt_ratio_a_x96 {
            // Price is below the range, position is fully in token0, so no token1 is needed.
            Ok(0)
        } else if sqrt_ratio_current_x96 < sqrt_ratio_b_x96 {
            // Price is in the range
            get_liquidity_for_amount1(sqrt_ratio_a_x96, sqrt_ratio_current_x96, amount_desired)
        } else {
            // Price is above the range, position is fully in token1
            get_liquidity_for_amount1(sqrt_ratio_a_x96, sqrt_ratio_b_x96, amount_desired)
        }
    }
}


/// Calculate the required amount0 and amount1 for a liquidity amount
/// 
/// # Arguments
/// 
/// * `sqrt_price_lower` - The lower tick's sqrt ratio
/// * `sqrt_price_upper` - The upper tick's sqrt ratio
/// * `liquidity` - The liquidity amount
/// * `current_pool_sqrt_price` - The current pool's sqrt price
/// 
/// # Returns
/// 
/// * `(U256, U256)` - The required amount0 and amount1
pub fn calculate_liquidity_amounts(
   current_pool_sqrt_price: U256,
   sqrt_price_lower: U256,
   sqrt_price_upper: U256,
   liquidity: u128,
) -> Result<(U256, U256), anyhow::Error> {
   let mut amount0 = U256::ZERO;
   let mut amount1 = U256::ZERO;

   if liquidity == 0 {
      return Ok((amount0, amount1));
   }

   let (sp_lower, sp_upper) = if sqrt_price_lower > sqrt_price_upper {
      (sqrt_price_upper, sqrt_price_lower)
   } else {
      (sqrt_price_lower, sqrt_price_upper)
   };

   if current_pool_sqrt_price <= sp_lower {
      amount0 = sqrt_price_math::_get_amount_0_delta(sp_lower, sp_upper, liquidity, false)?;
   } else if current_pool_sqrt_price < sp_upper {
      amount0 = sqrt_price_math::_get_amount_0_delta(current_pool_sqrt_price, sp_upper, liquidity, false)?;
      amount1 = sqrt_price_math::_get_amount_1_delta(sp_lower, current_pool_sqrt_price, liquidity, false)?;
   } else {
      amount1 = sqrt_price_math::_get_amount_1_delta(sp_lower, sp_upper, liquidity, false)?;
   }
   Ok((amount0, amount1))
}

pub fn get_fee_growth_inside(state: &V3PoolState, tick_lower: i32, tick_upper: i32) -> (U256, U256) {
   let default_tick_info = TickInfo::default();
   let lower_tick_info = state.ticks.get(&tick_lower).unwrap_or(&default_tick_info);
   let upper_tick_info = state.ticks.get(&tick_upper).unwrap_or(&default_tick_info);

   let fee_growth_global_0 = state.fee_growth_global_0_x128;
   let fee_growth_global_1 = state.fee_growth_global_1_x128;
   let current_tick = state.tick;

   // Calculate fee growth below the lower tick
   let (fee_growth_below_0, fee_growth_below_1) = if current_tick >= tick_lower {
      (
         lower_tick_info.fee_growth_outside_0_x128,
         lower_tick_info.fee_growth_outside_1_x128,
      )
   } else {
      (
         fee_growth_global_0 - lower_tick_info.fee_growth_outside_0_x128,
         fee_growth_global_1 - lower_tick_info.fee_growth_outside_1_x128,
      )
   };

   // Calculate fee growth above the upper tick
   let (fee_growth_above_0, fee_growth_above_1) = if current_tick < tick_upper {
      (
         upper_tick_info.fee_growth_outside_0_x128,
         upper_tick_info.fee_growth_outside_1_x128,
      )
   } else {
      (
         fee_growth_global_0 - upper_tick_info.fee_growth_outside_0_x128,
         fee_growth_global_1 - upper_tick_info.fee_growth_outside_1_x128,
      )
   };

   // feeGrowthInside = feeGrowthGlobal - feeGrowthBelow - feeGrowthAbove
   let fee_growth_inside_0 = fee_growth_global_0 - fee_growth_below_0 - fee_growth_above_0;
   let fee_growth_inside_1 = fee_growth_global_1 - fee_growth_below_1 - fee_growth_above_1;

   (fee_growth_inside_0, fee_growth_inside_1)
}
