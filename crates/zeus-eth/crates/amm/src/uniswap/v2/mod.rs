pub mod pool;

use crate::consts::*;
use crate::uniswap::UniswapPool;
use alloy_primitives::U256;
use currency::Currency;

pub const FEE: u32 = 300;

pub fn div_uu(x: U256, y: U256) -> Result<u128, anyhow::Error> {
   if !y.is_zero() {
      let mut answer;

      if x <= U256_0XFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF {
         answer = (x << U256_64) / y;
      } else {
         let mut msb = U256_192;
         let mut xc = x >> U256_192;

         if xc >= U256_0X100000000 {
            xc >>= U256_32;
            msb += U256_32;
         }

         if xc >= U256_0X10000 {
            xc >>= U256_16;
            msb += U256_16;
         }

         if xc >= U256_0X100 {
            xc >>= U256_8;
            msb += U256_8;
         }

         if xc >= U256_16 {
            xc >>= U256_4;
            msb += U256_4;
         }

         if xc >= U256_4 {
            xc >>= U256_2;
            msb += U256_2;
         }

         if xc >= U256_2 {
            msb += U256_1;
         }

         answer = (x << (U256_255 - msb)) / (((y - U256_1) >> (msb - U256_191)) + U256_1);
      }

      if answer > U256_0XFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF {
         return Ok(0);
      }

      let hi = answer * (y >> U256_128);
      let mut lo = answer * (y & U256_0XFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF);

      let mut xh = x >> U256_192;
      let mut xl = x << U256_64;

      if xl < lo {
         xh -= U256_1;
      }

      xl = xl.overflowing_sub(lo).0;
      lo = hi << U256_128;

      if xl < lo {
         xh -= U256_1;
      }

      xl = xl.overflowing_sub(lo).0;

      if xh != (hi >> U256_128) {
         return Err(anyhow::anyhow!("Rounding Error"));
      }

      answer += xl / y;

      if answer > U256_0XFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF {
         return Ok(0_u128);
      }

      Ok(answer.to::<u128>())
   } else {
      Err(anyhow::anyhow!("Y is zero"))
   }
}

/// Calculates the amount received for a given `amount_in` `reserve_in` and `reserve_out`.
pub fn get_amount_out(amount_in: U256, reserve_in: U256, reserve_out: U256) -> U256 {
   if amount_in.is_zero() || reserve_in.is_zero() || reserve_out.is_zero() {
      return U256::ZERO;
   }
   let fee = (10000 - FEE / 10) / 10; //Fee of 300 => (10,000 - 30) / 10  = 997
   let amount_in_with_fee = amount_in * U256::from(fee);
   let numerator = amount_in_with_fee * reserve_out;
   let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;

   numerator / denominator
}

/// Calculates the price of the currency_in in terms of the other currency in the pool
///
/// Returned as a Q64 fixed point number.
pub fn calculate_price_64_x_64(pool: &impl UniswapPool, currency_in: &Currency) -> Result<u128, anyhow::Error> {
   let state = pool
      .state()
      .v2_reserves()
      .ok_or(anyhow::anyhow!("State not initialized"))?;
   let decimal_shift = pool.currency0().decimals() as i8 - pool.currency1().decimals() as i8;

   let (r_0, r_1) = if decimal_shift < 0 {
      (
         U256::from(state.reserve0) * U256::from(10u128.pow(decimal_shift.unsigned_abs() as u32)),
         U256::from(state.reserve1),
      )
   } else {
      (
         U256::from(state.reserve0),
         U256::from(state.reserve1) * U256::from(10u128.pow(decimal_shift as u32)),
      )
   };

   if currency_in.address() == pool.currency0().address() {
      if r_0.is_zero() {
         Ok(U128_0X10000000000000000)
      } else {
         div_uu(r_1, r_0)
      }
   } else if r_1.is_zero() {
      Ok(U128_0X10000000000000000)
   } else {
      div_uu(r_0, r_1)
   }
}

pub fn q64_to_float(num: u128) -> f64 {
   const Q64: u128 = 1 << 64; // 2^64
   let integer_part = (num >> 64) as u64; // High 64 bits
   let fractional_part = (num & (Q64 - 1)) as u64; // Low 64 bits

   let integer_f64 = integer_part as f64;
   let fractional_f64 = (fractional_part as f64) / (Q64 as f64);

   integer_f64 + fractional_f64
}
