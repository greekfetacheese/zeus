use alloy_primitives::{
   U256,
   utils::{format_units, parse_units},
};
use serde::{Deserialize, Serialize};

/// Number of zeros immediately after the decimal point, e.g. `0.001` → 2.
///
/// Fast path: values `>= 0.1` (the common price/balance case) need no float formatting.
pub fn leading_zeros_after_decimal(x: f64) -> usize {
   let mut n = x.abs();
   if n == 0.0 || !n.is_finite() || n >= 0.1 {
      return 0;
   }

   let mut zeros = 0usize;
   while n < 0.1 && zeros < 16 {
      n *= 10.0;
      zeros += 1;
   }
   zeros
}

fn add_comma_separators(number: &str) -> String {
   let (integer_part, decimal_part) = match number.split_once('.') {
      Some((i, d)) => (i, d),
      None => (number, ""),
   };

   let extra_commas = integer_part.len().saturating_sub(1) / 3;
   let mut result = String::with_capacity(number.len() + extra_commas);
   let bytes = integer_part.as_bytes();
   let len = bytes.len();
   for (i, &b) in bytes.iter().enumerate() {
      if i != 0 && (len - i) % 3 == 0 {
         result.push(',');
      }
      result.push(char::from(b));
   }

   if !decimal_part.is_empty() {
      result.push('.');
      result.push_str(decimal_part);
   }
   result
}

pub fn format_dynamic_precision(x: f64, sig_digits: usize) -> String {
   let leading_zeros = leading_zeros_after_decimal(x);
   let total_decimals = leading_zeros + sig_digits;
   // Cap at a reasonable max to avoid f64 precision loss (e.g., 15-17 digits)
   let prec = total_decimals.min(15);
   format!("{:.prec$}", x)
}

fn remove_trailing_zeros(mut s: String) -> String {
   while s.ends_with('0') {
      s.pop();
   }

   if s.ends_with('.') {
      s.pop();
   }
   s
}

fn format_number(n: f64) -> String {
   let zeros = leading_zeros_after_decimal(n);

   // For very small number starting from 0.00
   if zeros > 1 {
      // Same as `format_dynamic_precision(n, zeros)` without recomputing zeros.
      let prec = (zeros.saturating_mul(2)).min(15);
      let s = format!("{:.prec$}", n);
      remove_trailing_zeros(s)

      // From 10k start adding commas
   } else if n > 9999.0 {
      let s = format!("{:.2}", n);
      add_comma_separators(&s)
   } else {
      format!("{:.2}", n)
   }
}

fn format_abbreviated(n: f64) -> Option<String> {
   if n < 1_000_000.0 {
      return None;
   }

   const ONE_SEXTILLION: f64 = 1_000_000_000_000_000_000_000.0;

   // Just return unlimited for now, these numbers doesn't make sense anyway
   if n > ONE_SEXTILLION {
      return Some(String::from("Unlimited"));
   }

   // Up to sextillion 10^21
   const SUFFIXES: [&str; 8] = ["", "K", "M", "B", "T", "Q", "Q", "S"];
   let magnitude = (n.log10() / 3.0).floor() as usize;
   let magnitude = magnitude.min(SUFFIXES.len() - 1);
   let scaled = n / 1000.0f64.powi(magnitude as i32);

   let mut formatted = format!("{:.2}", scaled);
   while formatted.ends_with('0') {
      formatted.pop();
   }
   if formatted.ends_with('.') {
      formatted.pop();
   }
   formatted.push_str(SUFFIXES[magnitude]);
   Some(formatted)
}

/// Represents a numeric value in different formats
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NumericValue {
   /// For [Self::value] & [Self::currency_price] is None
   pub wei: Option<U256>,
   pub f64: f64,
   pub formatted: String,
   pub abbreviated: Option<String>,
}

impl Default for NumericValue {
   fn default() -> Self {
      Self {
         wei: Some(U256::ZERO),
         f64: 0.0,
         formatted: String::from("0.00"),
         abbreviated: None,
      }
   }
}

// Builders

impl NumericValue {
   fn from_f64_parts(wei: Option<U256>, value: f64) -> Self {
      Self {
         wei,
         f64: value,
         formatted: format_number(value),
         abbreviated: format_abbreviated(value),
      }
   }

   fn from_wei(wei: U256, decimals: u8) -> Self {
      let units = format_units(wei, decimals).unwrap_or_else(|_| String::from("0"));
      let value = units.parse().unwrap_or(0.0);
      Self::from_f64_parts(Some(wei), value)
   }

   pub fn is_abbreviated_unlimited(&self) -> bool {
      self.abbreviated().eq_ignore_ascii_case("Unlimited")
   }

   /// Format a wei value to a readable format
   ///
   /// Example:
   /// ```
   /// // 1 ETH in wei
   /// let wei = U256::from(1000000000000000000u128);
   /// let value = NumericValue::format_wei(wei, 18);
   /// assert_eq!(value.wei().unwrap(), U256::from(1000000000000000000u128));
   /// assert_eq!(value.f64(), 1.0);
   /// ```
   pub fn format_wei(wei: U256, decimals: u8) -> Self {
      Self::from_wei(wei, decimals)
   }

   /// Parse a value doing the 10^decimals conversion
   ///
   /// The wei value is stored and its being formatted to a readable format [f64] and [String]
   ///
   /// Example:
   /// ```
   /// let amount = "1";
   /// let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
   /// assert_eq!(value.wei().unwrap(), U256::from(1000000000000000000u128));
   /// assert_eq!(value.f64, 1.0);
   /// ```
   pub fn parse_to_wei(amount: &str, currency_decimals: u8) -> Self {
      let wei = if let Ok(units) = parse_units(amount, currency_decimals) {
         units.get_absolute()
      } else {
         U256::ZERO
      };
      Self::from_wei(wei, currency_decimals)
   }

   /// Format a wei value to gwei in a readable format
   ///
   /// Example:
   /// ```
   /// // 1 GWei in wei
   /// let wei = U256::from(1000000000u128);
   /// let value = NumericValue::format_to_gwei(wei);
   /// assert_eq!(value.wei().unwrap(), U256::from(1000000000u128));
   /// assert_eq!(value.f64, 1.0);
   /// ```
   pub fn format_to_gwei(amount: U256) -> Self {
      Self::from_wei(amount, 9)
   }

   /// Parse a value doing the 10^9 conversion
   ///
   /// The wei value is stored and its being formatted to a readable format [f64] and [String]
   ///
   /// Example:
   /// ```
   /// let amount = "1";
   /// let value = NumericValue::parse_to_gwei(&amount.to_string());
   /// assert_eq!(value.wei().unwrap(), U256::from(1000000000u128));
   /// assert_eq!(value.f64, 1.0);
   /// ```
   pub fn parse_to_gwei(amount: &str) -> Self {
      let wei = if let Ok(units) = parse_units(amount, 9) {
         units.get_absolute()
      } else {
         U256::ZERO
      };
      Self::from_wei(wei, 9)
   }

   /// Computes the new amount by applying the given slippage percentage.
   /// The `slippage_percent` is in percentage points, e.g., 1.0 for 1%.
   /// Panics if `self.wei` is `None`.
   pub fn calc_slippage(&self, slippage: f64, decimals: u8) -> Self {
      let wei = self.wei();
      let slippage_bps = (slippage * 100.0) as u64;
      let denominator = 10000u64;
      let factor_num = denominator - slippage_bps;
      let wei = (wei * U256::from(factor_num)) / U256::from(denominator);
      let value = NumericValue::format_wei(wei, decimals);
      value
   }

   /// Calculate the new amount based on the given percentage
   pub fn calc_percent(&self, percent: f64, decimals: u8) -> Self {
      let wei = self.wei();
      let wei = (wei * U256::from(percent)) / U256::from(100.0);
      let value = NumericValue::format_wei(wei, decimals);
      value
   }

   /// Create a new NumericValue to represent a currency balance
   pub fn currency_balance(balance: U256, currency_decimals: u8) -> Self {
      Self::from_wei(balance, currency_decimals)
   }

   /// Create a new NumericValue to represent a currency price
   pub fn currency_price(price: f64) -> Self {
      Self::from_f64_parts(None, price)
   }

   /// Create a new NumericValue to represent a value
   ///
   /// `amount` * `price`
   pub fn value(amount: f64, price: f64) -> Self {
      let value = if amount == 0.0 || price == 0.0 {
         0.0
      } else {
         amount * price
      };
      Self::from_f64_parts(None, value)
   }

   pub fn from_f64(float: f64) -> Self {
      Self::from_f64_parts(None, float)
   }

   pub fn is_zero(&self) -> bool {
      if let Some(wei) = self.wei {
         return wei == U256::ZERO;
      }

      self.f64 == 0.0 || self.formatted == "0.00"
   }

   /// Panics if [Self::wei] is None
   pub fn wei(&self) -> U256 {
      self.wei.unwrap()
   }

   pub fn f64(&self) -> f64 {
      self.f64
   }

   /// Remove the commas from the formatted string
   pub fn flatten(&self) -> String {
      let string = self.f64.to_string();
      string.replace(",", "")
   }

   pub fn formatted(&self) -> &str {
      &self.formatted
   }

   pub fn abbreviated(&self) -> &str {
      self.abbreviated.as_deref().unwrap_or(&self.formatted)
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::utils::parse_ether;

   #[test]
   fn test_zero() {
      let value = NumericValue::currency_balance(U256::ZERO, 18);
      assert_eq!(value.is_zero(), true);
   }

   #[test]
   fn test_unlimited() {
      let value = NumericValue::currency_balance(U256::MAX, 18);
      assert_eq!(value.is_abbreviated_unlimited(), true);
   }

   #[test]
   fn format_abbreviated() {
      // 725,000,000.34 → "725M"
      let amount = parse_ether("725000000.34").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.abbreviated(), "725M");

      // 725,230,000.00 → "725.23M"
      let amount = parse_ether("725230000.00").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.abbreviated(), "725.23M");

      // 1,234.56 → "1234.56"
      let amount = parse_ether("1234.56").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.abbreviated(), "1234.56");

      // 12,345,678,900,000 → "12.35T"
      let amount = parse_ether("12345678900000").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.abbreviated(), "12.35T");

      // 1 quadrillion → "1Q"
      let one_quad = 1_000_000_000_000_000.0;
      let amount = parse_ether(&one_quad.to_string()).unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.abbreviated(), "1Q");

      // 2 Sextillion → "Unlimited"
      let two_sextillion = 2_000_000_000_000_000_000_000.0;
      let amount = parse_ether(&two_sextillion.to_string()).unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.abbreviated(), "Unlimited");
   }

   #[test]
   fn test_calc_slippage() {
      let value = NumericValue::parse_to_wei("1", 18);
      let value_after_slippage = value.calc_slippage(10.0, 18);
      assert_eq!(
         value_after_slippage.wei(),
         U256::from(900000000000000000u128)
      );
      assert_eq!(value_after_slippage.f64, 0.9);
   }

   #[test]
   fn test_parse_to_wei() {
      // 1 ETH
      let amount = "1";
      let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
      assert_eq!(value.wei(), U256::from(1000000000000000000u128));
      assert_eq!(value.f64, 1.0);
   }

   #[test]
   fn test_parse_to_wei_very_low_amount() {
      let amount = "0.00000001";
      let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
      assert_eq!(value.wei(), U256::from(10000000000u128));
      assert_eq!(value.f64, 0.00000001);
   }

   #[test]
   fn test_formatting_very_low_amounts() {
      let amount = "0.00000100";
      let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
      assert_eq!(value.wei(), U256::from(1000000000000u128));
      assert_eq!(value.f64, 0.000001);

      let abbreviated = format!("{:.10}", value.abbreviated());
      assert_eq!(abbreviated, "0.000001");
   }

   #[test]
   fn test_parse_to_gwei() {
      let amount = "1";
      let value = NumericValue::parse_to_gwei(&amount.to_string());
      assert_eq!(value.wei(), U256::from(1000000000u128));
      assert_eq!(value.f64, 1.0);

      let amount = "0.000000070";
      let value = NumericValue::parse_to_gwei(&amount.to_string());
      assert_eq!(value.wei(), U256::from(70u128));
      assert_eq!(value.f64, 0.000000070);
   }

   #[test]
   fn test_format_to_gwei() {
      let amount = U256::from(1000000000u128);
      let value = NumericValue::format_to_gwei(amount);
      assert_eq!(value.wei(), U256::from(1000000000u128));
      assert_eq!(value.f64, 1.0);
   }

   #[test]
   fn test_high_amount_value() {
      let amount = parse_ether("2133.073141862605681577").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.f64, 2133.073141862605681577);
      assert_eq!(value.flatten(), "2133.0731418626056");
   }

   #[test]
   fn test_very_low_price() {
      let price = 0.000001834247995202872;
      let value = NumericValue::currency_price(price);
      let value_formatted = format!("{:.10}", value.abbreviated());
      assert_eq!(value_formatted, "0.00000183");
   }

   #[test]
   fn test_formatted() {
      let v = 0.000001075424985484;
      let value = NumericValue::parse_to_wei(&v.to_string(), 18);
      let value_formatted = value.formatted();
      assert_eq!(value_formatted, "0.0000010754");
      assert_eq!(format!("{:.10}", value_formatted), "0.00000107");

      let v = 0.000001834247995202872;
      let value = NumericValue::currency_price(v);
      let value_formatted = value.formatted();
      assert_eq!(value_formatted, "0.0000018342");

      let v = 0.01;
      let value = NumericValue::currency_price(v);
      let value_formatted = value.formatted();
      assert_eq!(value_formatted, "0.01");

      let v = 0.001;
      let value = NumericValue::currency_price(v);
      let value_formatted = value.formatted();
      assert_eq!(value_formatted, "0.001");

      let wei = U256::from(3009581964807856u128);
      let value = NumericValue::format_wei(wei, 18);
      assert_eq!(value.f64(), 0.003009581964807856);
      assert_eq!(value.formatted(), "0.003");

      let price = 4304.34;
      let value = NumericValue::currency_price(price);
      let value_formatted = value.formatted();
      assert_eq!(value_formatted, "4304.34");

      let v = 10000.0;
      let value = NumericValue::currency_price(v);
      let value_formatted = value.formatted();
      assert_eq!(value_formatted, "10,000.00");

      let v = 100000.00;
      let value = NumericValue::currency_price(v);
      let value_formatted = value.formatted();
      assert_eq!(value_formatted, "100,000.00");
   }

   fn legacy_leading_zeros(x: f64) -> usize {
      let sci = format!("{:e}", x);
      let position = if let Some(exp_str) = sci.split('e').nth(1) {
         if let Ok(exp) = exp_str.parse::<i32>() {
            if exp < 0 { (-exp) as usize } else { 1 }
         } else {
            1
         }
      } else {
         1
      };
      position.saturating_sub(1)
   }

   fn legacy_format_number(n: f64) -> String {
      let zeros = legacy_leading_zeros(n);
      if zeros > 1 {
         let prec = (zeros + zeros).min(15);
         let mut s = format!("{:.prec$}", n);
         while s.ends_with('0') {
            s.pop();
         }
         if s.ends_with('.') {
            s.pop();
         }
         s
      } else if n > 9999.0 {
         let s = format!("{:.2}", n);
         let mut parts = s.splitn(2, '.');
         let integer_part = parts.next().unwrap_or("0");
         let decimal_part = parts.next().unwrap_or("");
         let mut result = String::new();
         let chars: Vec<char> = integer_part.chars().rev().collect();
         for (i, c) in chars.iter().enumerate() {
            if i > 0 && i % 3 == 0 {
               result.insert(0, ',');
            }
            result.insert(0, *c);
         }
         if !decimal_part.is_empty() {
            result.push('.');
            result.push_str(decimal_part);
         }
         result
      } else {
         format!("{:.2}", n)
      }
   }

   fn legacy_format_abbreviated(n: f64) -> Option<String> {
      if n < 1_000_000.0 {
         return None;
      }
      let one_sextillion = 1_000_000_000_000_000_000_000.0;
      if n > one_sextillion {
         return Some(format!("Unlimited"));
      }
      let suffixes = ["", "K", "M", "B", "T", "Q", "Q", "S"];
      let magnitude = (n.log10() / 3.0).floor() as usize;
      let magnitude = magnitude.min(suffixes.len() - 1);
      let divisor = 1000.0f64.powi(magnitude as i32);
      let scaled = n / divisor;
      let formatted =
         format!("{:.2}", scaled).trim_end_matches('0').trim_end_matches('.').to_string();
      Some(format!("{}{}", formatted, suffixes[magnitude]))
   }

   #[test]
   fn format_matches_legacy_across_magnitudes() {
      let mut samples = vec![
         0.0,
         0.1,
         0.01,
         0.001,
         0.0001,
         0.000001075424985484,
         0.000001834247995202872,
         0.003009581964807856,
         1.0,
         12.47,
         9999.0,
         9999.99,
         10000.0,
         10000.01,
         100_000.0,
         4304.34,
         725_000_000.34,
         725_230_000.0,
         1_234.56,
         12_345_678_900_000.0,
         1_000_000_000_000_000.0,
      ];
      for e in -18..=8 {
         let p = 10f64.powi(e);
         samples.push(p);
         samples.push(p * 1.23456789);
         samples.push(p * 9.999);
      }

      for x in samples {
         assert_eq!(
            format_number(x),
            legacy_format_number(x),
            "formatted mismatch for {x:?}"
         );
         assert_eq!(
            super::format_abbreviated(x),
            legacy_format_abbreviated(x),
            "abbreviated mismatch for {x:?}"
         );
      }
   }
}
