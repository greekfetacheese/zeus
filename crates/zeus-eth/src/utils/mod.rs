pub mod address_book;
pub mod batch;
pub mod block;
pub mod client;
pub mod price_feed;

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

use anyhow::anyhow;
use std::sync::Arc;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};

pub fn generate_permit2_json_value(
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
   target_address: Vec<Address>,
   events: impl IntoIterator<Item = impl AsRef<[u8]>>,
   from_block: u64,
   concurrency: usize,
   block_range: u64,
) -> Result<Vec<Log>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
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

   if latest_block - from_block > block_range {
      let mut start_block = from_block;

      while start_block <= latest_block {
         let end_block = std::cmp::min(start_block + block_range, latest_block);
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

fn get_decimal_position(x: f64) -> usize {
   let sci = format!("{:e}", x);
   if let Some(exp_str) = sci.split('e').nth(1) {
      if let Ok(exp) = exp_str.parse::<i32>() {
         if exp < 0 {
            return (-exp) as usize;
         }
      }
   }
   1
}

pub fn leading_zeros_after_decimal(x: f64) -> usize {
   let position = get_decimal_position(x);
   position.saturating_sub(1)
}

fn add_comma_separators(number: &str) -> String {
   let mut parts = number.splitn(2, '.');
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
      let s = format_dynamic_precision(n, zeros);
      remove_trailing_zeros(s)

      // From 10k start adding commas
   } else if n > 9999.0 {
      let s = format!("{:.2}", n);
      add_comma_separators(&s)
   } else {
      format!("{:.2}", n)
   }
}

fn _format_abbreviated(n: f64) -> Option<String> {
   if n < 1_000_000.0 {
      return None;
   }

   let one_sextillion = 1_000_000_000_000_000_000_000.0;

   // Just return unlimited for now, these numbers doesn't make sense anyway
   if n > one_sextillion {
     return Some(format!("Unlimited"));
   }

   // Up to sextillion 10^21
   let suffixes = ["", "K", "M", "B", "T", "Q", "Q", "S"];
   let magnitude = (n.log10() / 3.0).floor() as usize;
   let magnitude = magnitude.min(suffixes.len() - 1);
   let divisor = 1000.0f64.powi(magnitude as i32);
   let scaled = n / divisor;
   let formatted = format!("{:.2}", scaled)
      .trim_end_matches('0')
      .trim_end_matches('.')
      .to_string();
   let s = format!("{}{}", formatted, suffixes[magnitude]);
   Some(s)
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
      let units_formated = format_units(wei, decimals).unwrap_or("0".to_string());
      let f64 = units_formated.parse().unwrap_or(0.0);
      let formatted = format_number(f64);
      let abbreviated = _format_abbreviated(f64);

      Self {
         wei: Some(wei),
         f64,
         formatted,
         abbreviated,
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
   /// ```
   pub fn parse_to_wei(amount: &str, currency_decimals: u8) -> Self {
      let wei = if let Ok(units) = parse_units(amount, currency_decimals) {
         units.get_absolute()
      } else {
         U256::ZERO
      };

      let formatted = format_units(wei, currency_decimals).unwrap_or("0".to_string());
      let f64 = formatted.parse().unwrap_or(0.0);
      let formatted = format_number(f64);
      let abbreviated = _format_abbreviated(f64);

      Self {
         wei: Some(wei),
         f64,
         formatted,
         abbreviated,
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
   /// ```
   pub fn format_to_gwei(amount: U256) -> Self {
      let formatted = format_units(amount, 9).unwrap_or("0".to_string());
      let f64 = formatted.parse().unwrap_or(0.0);
      let formatted = format_number(f64);
      let abbreviated = _format_abbreviated(f64);

      Self {
         wei: Some(amount),
         f64,
         formatted,
         abbreviated,
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
   /// ```
   pub fn parse_to_gwei(amount: &str) -> Self {
      let wei = if let Ok(units) = parse_units(amount, 9) {
         units.get_absolute()
      } else {
         U256::ZERO
      };

      let formatted = format_units(wei, 9).unwrap_or("0".to_string());
      let f64 = formatted.parse().unwrap_or(0.0);
      let formatted = format_number(f64);
      let abbreviated = _format_abbreviated(f64);

      Self {
         wei: Some(wei),
         f64,
         formatted,
         abbreviated,
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
      let formatted = format_units(balance, currency_decimals).unwrap_or("0".to_string());
      let f64 = formatted.parse().unwrap_or(0.0);
      let formatted = format_number(f64);
      let abbreviated = _format_abbreviated(f64);
      Self {
         wei: Some(balance),
         f64,
         formatted,
         abbreviated,
      }
   }

   /// Create a new NumericValue to represent a currency price
   pub fn currency_price(price: f64) -> Self {
      let formatted = format_number(price);
      let abbreviated = _format_abbreviated(price);
      Self {
         wei: None,
         f64: price,
         formatted,
         abbreviated,
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

      let formatted = format_number(value);
      let abbreviated = _format_abbreviated(value);

      Self {
         wei: None,
         f64: value,
         formatted,
         abbreviated,
      }
   }

   pub fn from_f64(float: f64) -> Self {
      let formatted = format_number(float);
      let abbreviated = _format_abbreviated(float);
      Self {
         wei: None,
         f64: float,
         formatted,
         abbreviated,
      }
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

   pub fn formatted(&self) -> String {
      self.formatted.clone()
   }

   pub fn abbreviated(&self) -> String {
      match self.abbreviated {
         Some(ref s) => s.clone(),
         None => self.formatted(),
      }
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
}
