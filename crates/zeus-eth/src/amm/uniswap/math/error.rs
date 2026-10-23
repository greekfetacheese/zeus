use alloy_primitives::ruint::ParseError;
use std::fmt;

// TODO: make these errors better, some errors in univ3 libs are just require(condition) without a message.
#[derive(Debug)]
pub enum UniswapV3MathError {
   DenominatorIsZero,
   ResultIsU256MAX,
   SqrtPriceIsZero,
   SqrtPriceIsLteQuotient,
   ZeroValue,
   LiquidityIsZero,
   //TODO: Update this, shield your eyes for now
   ProductDivAmount,
   DenominatorIsLteProdOne,
   LiquiditySub,
   LiquidityAdd,
   T,
   R,
   SafeCastToU160Overflow,
   TickSpacingError,
   MiddlewareError(String),
   ParseError(ParseError),
}

impl fmt::Display for UniswapV3MathError {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      match self {
         UniswapV3MathError::DenominatorIsZero => f.write_str("Denominator is 0"),
         UniswapV3MathError::ResultIsU256MAX => f.write_str("Result is U256::MAX"),
         UniswapV3MathError::SqrtPriceIsZero => f.write_str("Sqrt price is 0"),
         UniswapV3MathError::SqrtPriceIsLteQuotient => {
            f.write_str("Sqrt price is less than or equal to quotient")
         }
         UniswapV3MathError::ZeroValue => f.write_str(
            "Can not get most significant bit or least significant bit on zero value",
         ),
         UniswapV3MathError::LiquidityIsZero => f.write_str("Liquidity is 0"),
         UniswapV3MathError::ProductDivAmount => f.write_str(
            "require((product = amount * sqrtPX96) / amount == sqrtPX96 && numerator1 > product);",
         ),
         UniswapV3MathError::DenominatorIsLteProdOne => {
            f.write_str("Denominator is less than or equal to prod_1")
         }
         UniswapV3MathError::LiquiditySub => f.write_str("Liquidity Sub"),
         UniswapV3MathError::LiquidityAdd => f.write_str("Liquidity Add"),
         UniswapV3MathError::T => {
            f.write_str("The given tick must be less than, or equal to, the maximum tick")
         }
         UniswapV3MathError::R => f.write_str(
            "Second inequality must be < because the price can never reach the price at the max tick",
         ),
         UniswapV3MathError::SafeCastToU160Overflow => {
            f.write_str("Overflow when casting to U160")
         }
         UniswapV3MathError::TickSpacingError => f.write_str("Tick spacing error"),
         UniswapV3MathError::MiddlewareError(_) => {
            f.write_str("Middleware error when getting next_initialized_tick_within_one_word")
         }
         // The label only: the inner error is the `source()`, not part of the message.
         UniswapV3MathError::ParseError(_) => f.write_str("Parse error"),
      }
   }
}

impl std::error::Error for UniswapV3MathError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         UniswapV3MathError::ParseError(err) => Some(err),
         _ => None,
      }
   }
}

impl From<ParseError> for UniswapV3MathError {
   fn from(err: ParseError) -> Self {
      UniswapV3MathError::ParseError(err)
   }
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
