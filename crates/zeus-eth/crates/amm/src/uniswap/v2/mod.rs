pub mod pool;

use crate::consts::*;
use alloy_primitives::U256;

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
   let fee = (10000 - 300 / 10) / 10; //Fee of 300 => (10,000 - 30) / 10  = 997
   let amount_in_with_fee = amount_in * U256::from(fee);
   let numerator = amount_in_with_fee * reserve_out;
   let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;

   numerator / denominator
}
