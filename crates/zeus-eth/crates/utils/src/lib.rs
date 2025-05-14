pub mod address;
//pub mod batch_request;
pub mod batch;
pub mod block;
pub mod client;
pub mod price_feed;

use alloy_contract::private::{Network, Provider};
use alloy_dyn_abi::{Eip712Domain, Eip712Types, Resolver, TypedData};
use alloy_primitives::{
   Address, U256,
   utils::{format_units, parse_units},
};
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log};
use serde::{Deserialize, Serialize};

use anyhow::anyhow;
use serde_json::Value;
use std::sync::Arc;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::trace;
use types::BlockTime;

pub use alloy_network;
pub use alloy_rpc_client;
pub use alloy_transport;

pub fn parse_typed_data(json: Value) -> Result<TypedData, anyhow::Error> {
   let domain: Eip712Domain = serde_json::from_value(json["domain"].clone())?;
   let types: Eip712Types = serde_json::from_value(json["types"].clone())?;
   let resolver = Resolver::from(&types);
   let primary_type = json["primaryType"]
      .as_str()
      .ok_or(anyhow!("Missing primaryType"))?
      .to_string();

   let message = json["message"].clone();

   Ok(TypedData {
      domain,
      resolver,
      primary_type,
      message,
   })
}

/// Is this token a base token?
///
/// We consider base tokens those that are mostly used for liquidity.
///
/// eg. WETH, WBNB, USDC, USDT, DAI are all base tokens.
pub fn is_base_token(chain: u64, token: Address) -> bool {
   let weth = address::weth(chain).is_ok_and(|weth| weth == token);
   let wbnb = address::wbnb(chain).is_ok_and(|wbnb| wbnb == token);
   let usdc = address::usdc(chain).is_ok_and(|usdc| usdc == token);
   let usdt = address::usdt(chain).is_ok_and(|usdt| usdt == token);
   let dai = address::dai(chain).is_ok_and(|dai| dai == token);

   weth || wbnb || usdc || usdt || dai
}

/// Get logs for a given target address and events
///
/// - `block_time` The block time to go back from the latest block (eg. 1 day etc..)
///
/// - `concurrency` The number of concurrent requests to make to the RPC, set 1 for no concurrency
pub async fn get_logs_for<P, N>(
   client: P,
   chain_id: u64,
   target_address: Vec<Address>,
   events: impl IntoIterator<Item = impl AsRef<[u8]>>,
   block_time: BlockTime,
   concurrency: usize,
) -> Result<Vec<Log>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let latest_block = client.get_block_number().await?;
   let from_block = block_time.go_back(chain_id, latest_block)?;

   trace!(target: "zeus_eth::utils::lib",
      "Fetching logs from block {} to {}",
      from_block, latest_block
   );

   let filter = Filter::new()
      .address(target_address)
      .events(events)
      .from_block(BlockNumberOrTag::Number(from_block))
      .to_block(BlockNumberOrTag::Number(latest_block));

   let logs = Arc::new(Mutex::new(Vec::new()));
   let semaphore = Arc::new(Semaphore::new(concurrency));

   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

   if latest_block - from_block > 100_000 {
      let mut start_block = from_block;

      while start_block <= latest_block {
         let end_block = std::cmp::min(start_block + 100_000, latest_block);
         let client = client.clone();
         let logs_clone = Arc::clone(&logs);
         let filter_clone = filter.clone();
         let permit = Arc::clone(&semaphore).acquire_owned().await?;

         trace!(target: "zeus_eth::utils::lib",
            "Quering Logs for block range: {} - {}",
            start_block, end_block
         );

         let task = tokio::spawn(async move {
            let local_filter = filter_clone
               .from_block(BlockNumberOrTag::Number(start_block))
               .to_block(BlockNumberOrTag::Number(end_block));

            let log_chunk = client.get_logs(&local_filter).await?;
            let mut logs_lock = logs_clone.lock().await;
            logs_lock.extend(log_chunk);
            drop(permit);
            Ok(())
         });

         tasks.push(task);
         start_block = end_block + 1;
      }

      for task in tasks {
         match task.await {
            Ok(_) => {}
            Err(e) => {
               trace!(target: "zeus_eth::utils::lib", "Error fetching logs: {:?}", e);
            }
         }
      }

      return Ok(Arc::try_unwrap(logs).unwrap().into_inner());
   }

   let log_chunk = client.get_logs(&filter).await?;
   Ok(log_chunk)
}

pub fn truncate_address(s: &str, max_len: usize) -> String {
   if s.len() <= max_len {
      return s.to_string();
   }
   let prefix_len = 6;
   let suffix_len = 6;
   // Ensure "0x" prefix is handled if present
   if s.starts_with("0x") && max_len > 6 {
      // 2 for "0x", 3 for "...", 1 for actual char
      let prefix = &s[..prefix_len.max(2)]; // Keep at least "0x"
      let suffix = &s[s.len() - suffix_len..];
      format!("{}...{}", prefix, suffix)
   } else {
      format!("{}...{}", &s[..prefix_len], &s[s.len() - suffix_len..])
   }
}

/// Format a very large number into a readable string
pub fn format_number(amount_str: &str, decimal_places: usize, trim_trailing_zeros: bool) -> String {
   // Split the number into integer and decimal parts
   let parts: Vec<&str> = amount_str.split('.').collect();
   let integer_part = parts[0];
   let decimal_part = if parts.len() > 1 { parts[1] } else { "0" };

   // Format the integer part with thousands separators
   let formatted_integer = add_thousands_separators(integer_part);

   // Determine the effective number of decimal places
   let effective_decimal_places = if integer_part == "0" {
      // For small numbers (less than 1), show up to 4 decimal places
      6
   } else {
      // For larger numbers, use the provided decimal_places
      decimal_places
   };

   // Handle decimal places
   if effective_decimal_places == 0 {
      formatted_integer // Return only the integer part
   } else {
      // Truncate or pad the decimal part to the effective length
      let decimal_to_show = if decimal_part.len() < effective_decimal_places {
         format!(
            "{:0<width$}",
            decimal_part,
            width = effective_decimal_places
         ) // Pad with zeros
      } else {
         decimal_part[..effective_decimal_places].to_string() // Truncate to desired length
      };

      let mut result = format!("{}.{}", formatted_integer, decimal_to_show);

      // Trim trailing zeros if requested
      if trim_trailing_zeros {
         while result.ends_with('0') {
            result.pop();
         }
         if result.ends_with('.') {
            result.pop(); // Remove decimal point if no digits remain
         }
      }
      result
   }
}

// Helper function to add thousands separators (assumed from previous context)
fn add_thousands_separators(number: &str) -> String {
   let mut result = String::new();
   let chars: Vec<char> = number.chars().rev().collect();
   for (i, c) in chars.iter().enumerate() {
      if i > 0 && i % 3 == 0 {
         result.insert(0, ',');
      }
      result.insert(0, *c);
   }
   result
}

pub fn format_price(price: f64) -> String {
   if price == 0.0 {
      return "0.00".to_string();
   }

   let price_str = format!("{:.10}", price); // Use enough precision
   let parts: Vec<&str> = price_str.split('.').collect();
   let integer_part = parts[0];
   let decimal_part = parts[1];

   // Add commas to integer part
   let mut formatted_integer = String::new();
   let mut count = 0;
   for c in integer_part.chars().rev() {
      if count > 0 && count % 3 == 0 {
         formatted_integer.push(',');
      }
      formatted_integer.push(c);
      count += 1;
   }
   let formatted_integer = formatted_integer.chars().rev().collect::<String>();

   let decimal_places = if price >= 1.0 {
      2
   } else {
      // Find the first non-zero digit in decimal part
      let first_non_zero = decimal_part.find(|c: char| c != '0').unwrap_or(0);
      // Show up to first_non_zero + 2 for precision
      (first_non_zero + 2).min(10)
   };

   let formatted_decimal = &decimal_part[..decimal_places.min(decimal_part.len())];
   format!("{}.{}", formatted_integer, formatted_decimal)
}

/// Represents a numeric value in different formats
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NumericValue {
   /// For [Self::value] & [Self::currency_price] is None
   pub wei: Option<U256>,
   pub f64: f64,
   pub formatted: String,
}

impl Default for NumericValue {
   fn default() -> Self {
      Self {
         wei: Some(U256::ZERO),
         f64: 0.0,
         formatted: "0".to_string(),
      }
   }
}

// Builders

impl NumericValue {
   /// Format a wei value to a readable format
   ///
   /// Example:
   /// ```
   /// // 1 ETH in wei
   /// let wei = U256::from(1000000000000000000u128);
   /// let value = NumericValue::format_wei(wei, 18);
   /// assert_eq!(value.wei().unwrap(), U256::from(1000000000000000000u128));
   /// assert_eq!(value.f64(), 1.0);
   /// assert_eq!(value.formatted(), "1");
   /// ```
   pub fn format_wei(wei: U256, decimals: u8) -> Self {
      let units_formated = format_units(wei, decimals).unwrap_or("0".to_string());
      let f64 = units_formated.parse().unwrap_or(0.0);
      let formatted = format_number(&units_formated, 2, true);

      Self {
         wei: Some(wei),
         f64,
         formatted,
      }
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
   /// assert_eq!(value.formatted, "1");
   /// ```
   pub fn parse_to_wei(amount: &str, currency_decimals: u8) -> Self {
      let wei = if let Ok(units) = parse_units(amount, currency_decimals) {
         units.get_absolute()
      } else {
         U256::ZERO
      };

      let units_formated = format_units(wei, currency_decimals).unwrap_or("0".to_string());
      let f64 = units_formated.parse().unwrap_or(0.0);
      let formatted = format_number(&units_formated, 2, true);

      Self {
         wei: Some(wei),
         f64,
         formatted,
      }
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
   /// assert_eq!(value.formatted, "1");
   /// ```
   pub fn format_to_gwei(amount: U256) -> Self {
      let units_formated = format_units(amount, 9).unwrap_or("0".to_string());
      let f64 = units_formated.parse().unwrap_or(0.0);
      let formatted = format_number(&units_formated, 2, true);

      Self {
         wei: Some(amount),
         f64,
         formatted,
      }
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
   /// assert_eq!(value.formatted, "1");
   /// ```
   pub fn parse_to_gwei(amount: &str) -> Self {
      let wei = if let Ok(units) = parse_units(amount, 9) {
         units.get_absolute()
      } else {
         U256::ZERO
      };

      let units_formated = format_units(wei, 9).unwrap_or("0".to_string());
      let f64 = units_formated.parse().unwrap_or(0.0);
      let formatted = format_number(&units_formated, 2, true);

      Self {
         wei: Some(wei),
         f64,
         formatted,
      }
   }

   /// Computes the new amount by applying the given slippage percentage.
   /// The `slippage_percent` is in percentage points, e.g., 1.0 for 1%.
   /// Panics if `self.wei` is `None`.
   pub fn calc_slippage(&mut self, slippage: f64, decimals: u8) {
      let wei = self.wei2();
      let slippage_bps = (slippage * 100.0) as u64;
      let denominator = 10000u64;
      let factor_num = denominator - slippage_bps;
      let wei = (wei * U256::from(factor_num)) / U256::from(denominator);
      let value = NumericValue::format_wei(wei, decimals);
      self.wei = Some(wei);
      self.f64 = value.f64();
      self.formatted = value.formatted;
   }

   /// Create a new NumericValue to represent a currency balance
   pub fn currency_balance(balance: U256, currency_decimals: u8) -> Self {
      let value_string = format_units(balance, currency_decimals).unwrap_or("0".to_string());
      let float = value_string.parse().unwrap_or(0.0);
      let formatted = format_number(&value_string, 2, true);
      Self {
         wei: Some(balance),
         f64: float,
         formatted,
      }
   }

   /// Create a new NumericValue to represent a currency price
   pub fn currency_price(price: f64) -> Self {
      let formatted = format_price(price);
      Self {
         wei: None,
         f64: price,
         formatted,
      }
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
      let formatted = format_number(&value.to_string(), 2, true);
      Self {
         wei: None,
         f64: value,
         formatted,
      }
   }

   pub fn from_f64(float: f64) -> Self {
      let formatted = format_number(&float.to_string(), 2, true);
      Self {
         wei: None,
         f64: float,
         formatted,
      }
   }

   pub fn is_zero(&self) -> bool {
      matches!(self.f64, 0.0) || matches!(self.formatted.as_str(), "0")
   }

   pub fn wei(&self) -> Option<U256> {
      self.wei
   }

   /// Panics if [Self::wei] is None
   pub fn wei2(&self) -> U256 {
      self.wei.unwrap()
   }

   pub fn f64(&self) -> f64 {
      self.f64
   }

   pub fn formatted(&self) -> &String {
      &self.formatted
   }

   /// Remove the commas from the formatted string
   pub fn flatten(&self) -> String {
      self.formatted.replace(",", "")
   }

   /// Formats the `f64` value into a compact string with abbreviations.
   /// Examples:
   /// - 1,234.56 → "1234.56"
   /// - 725,000,000.34 → "725M"
   /// - 725,230,000.00 → "725.23M"
   /// - 12,345,678,900,000 → "12.35T"
   pub fn format_abbreviated(&self) -> String {
      let n = self.f64;
      // less than a million just return it as it is
      if n < 1_000_000.0 {
         return self.formatted().clone();
      }
      

      let suffixes = ["", "K", "M", "B", "T"];
      let magnitude = (n.log10() / 3.0).floor() as usize;
      let magnitude = magnitude.min(suffixes.len() - 1);
      let divisor = 1000.0f64.powi(magnitude as i32);
      let scaled = n / divisor;
      let formatted = format!("{:.2}", scaled)
         .trim_end_matches('0')
         .trim_end_matches('.')
         .to_string();
      format!("{}{}", formatted, suffixes[magnitude])
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::utils::parse_ether;

   #[test]
   fn format_abbreviated() {
      // 725,000,000.34 → "725M"
      let amount = parse_ether("725000000.34").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.format_abbreviated(), "725M");

      // 725,230,000.00 → "725.23M"
      let amount = parse_ether("725230000.00").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.format_abbreviated(), "725.23M");

      // 1,234.56 → "1,234.56"
      let amount = parse_ether("1234.56").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.format_abbreviated(), "1,234.56");

      // 12,345,678,900,000 → "12.35T"
      let amount = parse_ether("12345678900000").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.format_abbreviated(), "12.35T");
   }

   #[test]
   fn test_low_amount_value() {
      let amount = parse_ether("0.001834247995202872").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.f64, 0.001834247995202872);
      assert_eq!(value.formatted, "0.001834");
   }

   #[test]
   fn test_calc_slippage() {
      let mut value = NumericValue::parse_to_wei("1", 18);
      value.calc_slippage(10.0, 18);
      assert_eq!(value.wei2(), U256::from(900000000000000000u128));
      assert_eq!(value.f64, 0.9);
      assert_eq!(value.formatted, "0.9");
   }

   #[test]
   fn test_parse_to_wei() {
      // 1 ETH
      let amount = "1";
      let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
      assert_eq!(value.wei().unwrap(), U256::from(1000000000000000000u128));
      assert_eq!(value.f64, 1.0);
      assert_eq!(value.formatted, "1");

      // 0.001294885 ETH
      let amount = "0.001294885";
      let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
      assert_eq!(value.wei().unwrap(), U256::from(1294885000000000u128));
      assert_eq!(value.f64, 0.001294885);
      assert_eq!(value.formatted, "0.001294");
   }

   #[test]
   fn test_parse_to_wei_low_amount() {
      let amount = "0.000001";
      let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
      assert_eq!(value.wei().unwrap(), U256::from(1000000000000u128));
      assert_eq!(value.f64, 0.000001);
      assert_eq!(value.formatted, "0.000001");
   }

   #[test]
   fn test_parse_to_gwei() {
      let amount = "1";
      let value = NumericValue::parse_to_gwei(&amount.to_string());
      assert_eq!(value.wei().unwrap(), U256::from(1000000000u128));
      assert_eq!(value.f64, 1.0);
      assert_eq!(value.formatted, "1");

      let amount = "0.000000070";
      let value = NumericValue::parse_to_gwei(&amount.to_string());
      assert_eq!(value.wei().unwrap(), U256::from(70u128));
      assert_eq!(value.f64, 0.000000070);
      assert_eq!(value.formatted, "0");
   }

   #[test]
   fn test_format_to_gwei() {
      let amount = U256::from(1000000000u128);
      let value = NumericValue::format_to_gwei(amount);
      assert_eq!(value.wei().unwrap(), U256::from(1000000000u128));
      assert_eq!(value.f64, 1.0);
      assert_eq!(value.formatted, "1");
   }

   #[test]
   fn test_high_amount_value() {
      let amount = parse_ether("2133.073141862605681577").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.f64, 2133.073141862605681577);
      assert_eq!(value.formatted, "2,133.07");
      assert_eq!(value.flatten(), "2133.07");
   }

   #[test]
   fn test_value() {
      let amount = 0.421;
      let price = 2345.33;
      let value = NumericValue::value(amount, price);
      assert_eq!(value.f64, price * amount);
      assert_eq!(value.formatted, "987.38");
   }

   #[test]
   fn test_low_price() {
      let price = 0.001834247995202872;
      let value = NumericValue::currency_price(price);
      assert_eq!(value.f64, 0.001834247995202872);
      assert_eq!(value.formatted, "0.0018");
   }
}
