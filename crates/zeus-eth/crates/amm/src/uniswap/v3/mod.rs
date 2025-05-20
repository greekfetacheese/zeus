pub mod fee_math;
pub mod pool;
pub mod position;

use alloy_primitives::{I256, U256};

use super::UniswapPool;
use crate::consts::U256_1;
use std::cmp::Ordering;
use uniswap_v3_math::{sqrt_price_math, tick_math::*};

use anyhow::anyhow;

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
   pool: &impl UniswapPool,
   zero_for_one: bool,
   amount_in: U256,
) -> Result<(U256, CurrentState), anyhow::Error> {
   if amount_in.is_zero() {
      return Ok((U256::ZERO, CurrentState::default()));
   }

   let state = pool
      .state()
      .v3_or_v4_state()
      .ok_or_else(|| anyhow!("State not initialized"))?;
   let fee = pool.fee();

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
      // Note: this could be removed as we are clamping in the batch contract
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
         fee.fee(),
      )?;

      // Decrement the amount remaining to be swapped and amount received from the step
      current_state.amount_specified_remaining = current_state
         .amount_specified_remaining
         .overflowing_sub(I256::from_raw(
            step.amount_in.overflowing_add(step.fee_amount).0,
         ))
         .0;

      current_state.amount_calculated -= I256::from_raw(step.amount_out);

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

   Ok((amount_out, current_state))
}

pub fn calculate_price(pool: &impl UniswapPool, zero_for_one: bool) -> Result<f64, anyhow::Error> {
   let state = pool
      .state()
      .v3_or_v4_state()
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


pub fn calculate_liquidity_amounts(
   sqrt_price_lower: U256,
   sqrt_price_upper: U256,
   liquidity: u128,
   current_pool_sqrt_price: U256,
) -> Result<(U256, U256), anyhow::Error> {
   let mut amount0 = U256::ZERO;
   let mut amount1 = U256::ZERO;

   let (sp_lower, sp_upper) = if sqrt_price_lower > sqrt_price_upper {
      (sqrt_price_upper, sqrt_price_lower)
   } else {
      (sqrt_price_lower, sqrt_price_upper)
   };

   if current_pool_sqrt_price <= sp_lower {
      amount0 = sqrt_price_math::_get_amount_0_delta(sp_lower, sp_upper, liquidity, false)?;
   } else if current_pool_sqrt_price < sp_upper {
      amount0 = sqrt_price_math::_get_amount_0_delta(
         current_pool_sqrt_price,
         sp_upper,
         liquidity,
         false,
      )?;
      amount1 = sqrt_price_math::_get_amount_1_delta(
         sp_lower,
         current_pool_sqrt_price,
         liquidity,
         false,
      )?;
   } else {
      amount1 = sqrt_price_math::_get_amount_1_delta(
         sp_lower,
         sp_upper,
         liquidity,
         false,
      )?;
   }
   Ok((amount0, amount1))
}