pub mod address;
pub mod batch_request;
pub mod block;
pub mod client;
pub mod price_feed;

use alloy_primitives::{Address, U256, utils::format_units};
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log};
use alloy_contract::private::{Network, Provider};
use serde::{Serialize, Deserialize};

use std::sync::Arc;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};
use tracing::trace;
use types::BlockTime;

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
   P: Provider<(), N> + Clone + 'static,
   N: Network,
{
   let latest_block = client.get_block_number().await?;
   let from_block = block_time.go_back(chain_id, latest_block)?;

   trace!(
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

         trace!(
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
               trace!("Error fetching logs: {:?}", e);
            }
         }
      }

      return Ok(Arc::try_unwrap(logs).unwrap().into_inner());
   }

   let log_chunk = client.get_logs(&filter).await?;
   Ok(log_chunk)
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
      4
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumericValue {
   pub uint: Option<U256>,
   pub float: f64,
   pub formatted: String,
}

impl Default for NumericValue {
   fn default() -> Self {
      Self {
         uint: None,
         float: 0.0,
         formatted: "0".to_string(),
      }
   }
}

impl NumericValue {
   pub fn new(uint: Option<U256>, float: f64, formatted: String) -> Self {
      Self {
         uint,
         float,
         formatted,
      }
   }

   /// Create a new NumericValue to represent a currency balance
   pub fn currency_balance(balance: U256, currency_decimals: u8) -> Self {
      let value_string = format_units(balance, currency_decimals).unwrap_or("0".to_string());
      let float = value_string.parse().unwrap_or(0.0);
      let formatted = format_number(&value_string, 2, true);
      Self {
         uint: Some(balance),
         float,
         formatted,
      }
   }

   /// Create a new NumericValue to represent a currency price
   pub fn currency_price(price: f64) -> Self {
      let formatted = format_price(price);
      Self {
         uint: None,
         float: price,
         formatted,
      }
   }

   /// Create a new NumericValue to represent a currency value
   pub fn currency_value(amount: f64, price: f64) -> Self {
      let value = if amount == 0.0 || price == 0.0 {
         0.0
      } else {
         amount * price
      };
      let formatted = format_number(&value.to_string(), 2, true);
      Self {
         uint: None,
         float: value,
         formatted,
      }
   }

   pub fn uint(&self) -> Option<U256> {
      self.uint
   }

   pub fn float(&self) -> f64 {
      self.float
   }

   pub fn formatted(&self) -> &String {
      &self.formatted
   }
}

mod tests {
   #[allow(unused_imports)]
   use super::*;
   #[allow(unused_imports)]
   use alloy_primitives::utils::parse_ether;

   #[test]
   fn test_low_amount_value() {
      let amount = parse_ether("0.001834247995202872").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.float, 0.001834247995202872);
      assert_eq!(value.formatted, "0.0018");
   }

   #[test]
   fn test_high_amount_value() {
      let amount = parse_ether("2133.073141862605681577").unwrap();
      let value = NumericValue::currency_balance(amount, 18);
      assert_eq!(value.float, 2133.073141862605681577);
      assert_eq!(value.formatted, "2,133.07");
   }

   #[test]
   fn test_currency_value() {
      let amount = 0.421;
      let price = 2345.33;
      let value = NumericValue::currency_value(amount, price);
      assert_eq!(value.float, price * amount);
      assert_eq!(value.formatted, "987.38");
   }

   #[test]
   fn test_low_price() {
      let price = 0.001834247995202872;
      let value = NumericValue::currency_price(price);
      assert_eq!(value.float, 0.001834247995202872);
      assert_eq!(value.formatted, "0.0018");
   }
}