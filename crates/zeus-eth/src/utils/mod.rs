pub mod address_book;
pub mod batch;
pub mod block;
pub mod client;
pub mod price_feed;
pub mod secure_signer;

pub use secure_signer::*;

use alloy_contract::private::{Network, Provider};
use alloy_dyn_abi::{Eip712Domain, Eip712Types, Resolver, TypedData};
use alloy_primitives::{
   Address, U256,
   aliases::U48,
   utils::{format_units, parse_units},
};
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::abi::permit::Permit2::PermitDetails;

use anyhow::anyhow;
use std::sync::Arc;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};



pub fn generate_permit2_single_value(
   chain_id: u64,
   token: Address,
   spender: Address,
   amount: U256,
   permit2: Address,
   expiration: U256,
   sig_deadline: U256,
   nonce: U48,
) -> Value {
   let value = serde_json::json!({
       "types": {
           "PermitSingle": [
               {"name": "details", "type": "PermitDetails"},
               {"name": "spender", "type": "address"},
               {"name": "sigDeadline", "type": "uint256"}
           ],
           "PermitDetails": [
               {"name": "token", "type": "address"},
               {"name": "amount", "type": "uint160"},
               {"name": "expiration", "type": "uint48"},
               {"name": "nonce", "type": "uint48"}
           ],
           "EIP712Domain": [
               {"name": "name", "type": "string"},
               {"name": "chainId", "type": "uint256"},
               {"name": "verifyingContract", "type": "address"}
           ]
       },
       "domain": {
           "name": "Permit2",
           "chainId": chain_id.to_string(),
           "verifyingContract": permit2.to_string()
       },
       "primaryType": "PermitSingle",
       "message": {
           "details": {
               "token": token.to_string(),
               "amount": amount.to_string(),
               "expiration": expiration.to_string(),
               "nonce": nonce.to_string()
           },
           "spender": spender.to_string(),
           "sigDeadline": sig_deadline.to_string()
       }
   });

   value
}

pub fn generate_permit2_batch_value(
   chain_id: u64,
   details: Vec<PermitDetails>,
   spender: Address,
   permit2: Address,
   sig_deadline: U256,
) -> Value {
   let details_json: Vec<Value> = details
      .iter()
      .map(
         |PermitDetails {
             token,
             amount,
             expiration,
             nonce,
          }| {
            serde_json::json!({
                "token": token.to_string(),
                "amount": amount.to_string(),
                "expiration": expiration.to_string(),
                "nonce": nonce.to_string()
            })
         },
      )
      .collect();

   let value = serde_json::json!({
       "types": {
           "PermitBatch": [
               {"name": "details", "type": "PermitDetails[]"},
               {"name": "spender", "type": "address"},
               {"name": "sigDeadline", "type": "uint256"}
           ],
           "PermitDetails": [
               {"name": "token", "type": "address"},
               {"name": "amount", "type": "uint160"},
               {"name": "expiration", "type": "uint48"},
               {"name": "nonce", "type": "uint48"}
           ],
           "EIP712Domain": [
               {"name": "name", "type": "string"},
               {"name": "chainId", "type": "uint256"},
               {"name": "verifyingContract", "type": "address"}
           ]
       },
       "domain": {
           "name": "Permit2",
           "chainId": chain_id.to_string(),
           "verifyingContract": permit2.to_string()
       },
       "primaryType": "PermitBatch",
       "message": {
           "details": details_json,
           "spender": spender.to_string(),
           "sigDeadline": sig_deadline.to_string()
       }
   });

   value
}

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
   let weth = address_book::weth(chain).is_ok_and(|weth| weth == token);
   let wbnb = address_book::wbnb(chain).is_ok_and(|wbnb| wbnb == token);
   let usdc = address_book::usdc(chain).is_ok_and(|usdc| usdc == token);
   let usdt = address_book::usdt(chain).is_ok_and(|usdt| usdt == token);
   let dai = address_book::dai(chain).is_ok_and(|dai| dai == token);

   weth || wbnb || usdc || usdt || dai
}

/// Get logs for a given target address and events
///
/// - `block_time` The block time to go back from the latest block (eg. 1 day etc..)
///
/// - `concurrency` The number of concurrent requests to make to the RPC, set 1 for no concurrency
pub async fn get_logs_for<P, N>(
   client: P,
   _chain_id: u64,
   target_address: Vec<Address>,
   events: impl IntoIterator<Item = impl AsRef<[u8]>>,
   from_block: u64,
   concurrency: usize,
) -> Result<Vec<Log>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   // Could do more but every provider has its own limits
   const BLOCK_RANGE: u64 = 50_000;

   let latest_block = client.get_block_number().await?;

   tracing::debug!(target: "zeus_eth::utils::lib",
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

   if latest_block - from_block > BLOCK_RANGE {
      let mut start_block = from_block;

      while start_block <= latest_block {
         let end_block = std::cmp::min(start_block + BLOCK_RANGE, latest_block);
         let client = client.clone();
         let logs_clone = Arc::clone(&logs);
         let filter_clone = filter.clone();
         let semaphore = semaphore.clone();

         let task = tokio::spawn(async move {
            let _permit = semaphore.acquire_owned().await?;
            tracing::debug!(target: "zeus_eth::utils::lib",
               "Quering Logs for block range: {} - {}",
               start_block, end_block
            );

            let local_filter = filter_clone
               .from_block(BlockNumberOrTag::Number(start_block))
               .to_block(BlockNumberOrTag::Number(end_block));

            let log_chunk = client.get_logs(&local_filter).await?;
            let mut logs_lock = logs_clone.lock().await;
            logs_lock.extend(log_chunk);
            Ok(())
         });

         tasks.push(task);
         start_block = end_block + 1;
      }

      for task in tasks {
         match task.await {
            Ok(_) => {}
            Err(e) => {
               tracing::error!(target: "zeus_eth::utils::lib", "Error fetching logs: {:?}", e);
            }
         }
      }

      return Ok(Arc::try_unwrap(logs).unwrap().into_inner());
   }

   let logs = client.get_logs(&filter).await?;
   Ok(logs)
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
   let parts: Vec<&str> = amount_str.split('.').collect();
   let integer_part = parts[0];
   let decimal_part = if parts.len() > 1 { parts[1] } else { "0" };

   let formatted_integer = add_thousands_separators(integer_part);

   let effective_decimal_places = if integer_part == "0" {
      6
   } else {
      decimal_places
   };

   if effective_decimal_places == 0 {
      formatted_integer
   } else {
      let decimal_to_show = if decimal_part.len() < effective_decimal_places {
         format!(
            "{:0<width$}",
            decimal_part,
            width = effective_decimal_places
         )
      } else {
         decimal_part[..effective_decimal_places].to_string()
      };

      let mut result = format!("{}.{}", formatted_integer, decimal_to_show);

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
   for (count, c) in integer_part.chars().rev().enumerate() {
      if count > 0 && count % 3 == 0 {
         formatted_integer.push(',');
      }
      formatted_integer.push(c);
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
   pub fn calc_slippage(&self, slippage: f64, decimals: u8) -> Self {
      let wei = self.wei();
      let slippage_bps = (slippage * 100.0) as u64;
      let denominator = 10000u64;
      let factor_num = denominator - slippage_bps;
      let wei = (wei * U256::from(factor_num)) / U256::from(denominator);
      let value = NumericValue::format_wei(wei, decimals);
      NumericValue {
         wei: Some(wei),
         f64: value.f64(),
         formatted: value.formatted,
      }
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
      if let Some(wei) = self.wei {
         return wei == U256::ZERO;
      }

      self.f64 == 0.0 || self.formatted.as_str() == "0"
   }

   /// Panics if [Self::wei] is None
   pub fn wei(&self) -> U256 {
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
   fn format_zero() {
      let value = NumericValue::format_wei(U256::ZERO, 18);
      assert_eq!(value.wei(), U256::ZERO);
      assert_eq!(value.formatted(), "0");
      assert_eq!(value.f64(), 0.0);
   }

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
      let value = NumericValue::parse_to_wei("1", 18);
      let value_after_slippage = value.calc_slippage(10.0, 18);
      assert_eq!(value_after_slippage.wei(), U256::from(900000000000000000u128));
      assert_eq!(value_after_slippage.f64, 0.9);
      assert_eq!(value_after_slippage.formatted, "0.9");
   }

   #[test]
   fn test_parse_to_wei() {
      // 1 ETH
      let amount = "1";
      let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
      assert_eq!(value.wei(), U256::from(1000000000000000000u128));
      assert_eq!(value.f64, 1.0);
      assert_eq!(value.formatted, "1");

      // 0.001294885 ETH
      let amount = "0.001294885";
      let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
      assert_eq!(value.wei(), U256::from(1294885000000000u128));
      assert_eq!(value.f64, 0.001294885);
      assert_eq!(value.formatted, "0.001294");
   }

   #[test]
   fn test_parse_to_wei_low_amount() {
      let amount = "0.000001";
      let value = NumericValue::parse_to_wei(&amount.to_string(), 18);
      assert_eq!(value.wei(), U256::from(1000000000000u128));
      assert_eq!(value.f64, 0.000001);
      assert_eq!(value.formatted, "0.000001");
   }

   #[test]
   fn test_parse_to_gwei() {
      let amount = "1";
      let value = NumericValue::parse_to_gwei(&amount.to_string());
      assert_eq!(value.wei(), U256::from(1000000000u128));
      assert_eq!(value.f64, 1.0);
      assert_eq!(value.formatted, "1");

      let amount = "0.000000070";
      let value = NumericValue::parse_to_gwei(&amount.to_string());
      assert_eq!(value.wei(), U256::from(70u128));
      assert_eq!(value.f64, 0.000000070);
      assert_eq!(value.formatted, "0");
   }

   #[test]
   fn test_format_to_gwei() {
      let amount = U256::from(1000000000u128);
      let value = NumericValue::format_to_gwei(amount);
      assert_eq!(value.wei(), U256::from(1000000000u128));
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
