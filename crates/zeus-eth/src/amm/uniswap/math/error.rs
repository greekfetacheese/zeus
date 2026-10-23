use alloy_primitives::ruint::ParseError;
use thiserror::Error;

// TODO: make these errors better, some errors in univ3 libs are just require(condition) without a message.
#[derive(Error, Debug)]
pub enum UniswapV3MathError {
   #[error("Denominator is 0")]
   DenominatorIsZero,
   #[error("Result is U256::MAX")]
   ResultIsU256MAX,
   #[error("Sqrt price is 0")]
   SqrtPriceIsZero,
   #[error("Sqrt price is less than or equal to quotient")]
   SqrtPriceIsLteQuotient,
   #[error("Can not get most significant bit or least significant bit on zero value")]
   ZeroValue,
   #[error("Liquidity is 0")]
   LiquidityIsZero,
   //TODO: Update this, shield your eyes for now
   #[error("require((product = amount * sqrtPX96) / amount == sqrtPX96 && numerator1 > product);")]
   ProductDivAmount,
   #[error("Denominator is less than or equal to prod_1")]
   DenominatorIsLteProdOne,
   #[error("Liquidity Sub")]
   LiquiditySub,
   #[error("Liquidity Add")]
   LiquidityAdd,
   #[error("The given tick must be less than, or equal to, the maximum tick")]
   T,
   #[error(
      "Second inequality must be < because the price can never reach the price at the max tick"
   )]
   R,
   #[error("Overflow when casting to U160")]
   SafeCastToU160Overflow,
   #[error("Tick spacing error")]
   TickSpacingError,
   #[error("Middleware error when getting next_initialized_tick_within_one_word")]
   MiddlewareError(String),
   #[error("Parse error")]
   ParseError(#[from] ParseError),
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::ruint::BaseConvertError;
   use std::error::Error as _;

   fn parse_error() -> ParseError {
      ParseError::BaseConvertError(BaseConvertError::InvalidDigit(63, 37))
   }

   // Every message is pinned byte-exact: they surface in pool math failures.
   #[test]
   fn display_matches_the_rendered_messages() {
      assert_eq!(
         UniswapV3MathError::DenominatorIsZero.to_string(),
         "Denominator is 0"
      );
      assert_eq!(
         UniswapV3MathError::ResultIsU256MAX.to_string(),
         "Result is U256::MAX"
      );
      assert_eq!(
         UniswapV3MathError::SqrtPriceIsZero.to_string(),
         "Sqrt price is 0"
      );
      assert_eq!(
         UniswapV3MathError::SqrtPriceIsLteQuotient.to_string(),
         "Sqrt price is less than or equal to quotient"
      );
      assert_eq!(
         UniswapV3MathError::ZeroValue.to_string(),
         "Can not get most significant bit or least significant bit on zero value"
      );
      assert_eq!(
         UniswapV3MathError::LiquidityIsZero.to_string(),
         "Liquidity is 0"
      );
      assert_eq!(
         UniswapV3MathError::ProductDivAmount.to_string(),
         "require((product = amount * sqrtPX96) / amount == sqrtPX96 && numerator1 > product);"
      );
      assert_eq!(
         UniswapV3MathError::DenominatorIsLteProdOne.to_string(),
         "Denominator is less than or equal to prod_1"
      );
      assert_eq!(
         UniswapV3MathError::LiquiditySub.to_string(),
         "Liquidity Sub"
      );
      assert_eq!(
         UniswapV3MathError::LiquidityAdd.to_string(),
         "Liquidity Add"
      );
      assert_eq!(
         UniswapV3MathError::T.to_string(),
         "The given tick must be less than, or equal to, the maximum tick"
      );
      assert_eq!(
         UniswapV3MathError::R.to_string(),
         "Second inequality must be < because the price can never reach the price at the max tick"
      );
      assert_eq!(
         UniswapV3MathError::SafeCastToU160Overflow.to_string(),
         "Overflow when casting to U160"
      );
      assert_eq!(
         UniswapV3MathError::TickSpacingError.to_string(),
         "Tick spacing error"
      );
      assert_eq!(
         UniswapV3MathError::MiddlewareError("boom".to_string()).to_string(),
         "Middleware error when getting next_initialized_tick_within_one_word"
      );
      // `ParseError` deliberately renders only its own label, not the inner error.
      assert_eq!(
         UniswapV3MathError::ParseError(parse_error()).to_string(),
         "Parse error"
      );
   }

   // Only `ParseError` carries a cause; a unit variant and a plain `String` payload do not.
   #[test]
   fn variants_without_an_error_source_have_none() {
      assert!(
         UniswapV3MathError::DenominatorIsZero.source().is_none(),
         "a unit variant must not expose a source"
      );
      assert!(
         UniswapV3MathError::MiddlewareError("not found".to_string()).source().is_none(),
         "a String payload is not an error source"
      );
   }

   // `ParseError` was `#[from]` without `transparent`: Display is the label, `source()` the inner.
   #[test]
   fn parse_error_hides_its_inner_in_display_but_exposes_it_as_source() {
      let inner = parse_error();
      let inner_display = inner.to_string();
      let err = UniswapV3MathError::ParseError(inner);

      assert_eq!(err.to_string(), "Parse error");
      assert_eq!(
         err.source().expect("ParseError must expose its inner error").to_string(),
         inner_display
      );
   }
}
