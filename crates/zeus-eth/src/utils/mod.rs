pub mod address_book;
pub mod batch;
pub mod block;
pub mod client;
pub mod ens;
pub mod interoperable_name;
pub mod numeric_value;
pub mod price_feed;
pub mod safe;
pub mod smart_accounts;

pub use numeric_value::NumericValue;

use alloy_contract::private::{Network, Provider};
use alloy_primitives::Address;
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log};

use std::sync::Arc;
use std::time::Duration;
use tokio::{
   sync::{Mutex, Semaphore},
   task::JoinHandle,
};

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
   let usdg = address_book::usdg(chain).is_ok_and(|usdg| usdg == token);

   weth || wbnb || usdc || usdt || dai || usdg
}

/// Base delay between `eth_getLogs` retries; multiplied by the attempt number (linear backoff).
const GET_LOGS_RETRY_BASE_DELAY_MS: u64 = 500;

/// Default retry budget for a log range before the fetch fails.
pub const GET_LOGS_MAX_RETRIES: usize = 5;

/// Get logs for a given target address and events
///
/// - `from_block` The first block of the range (inclusive)
///
/// - `to_block` The last block of the range (inclusive)
///
/// - `concurrency` The number of concurrent requests to make to the RPC, set 1 for no concurrency
///
/// - `max_retries` The number of retries after the first attempt for each request, with linear
///   backoff; a request that still fails fails the whole call
pub async fn get_logs_for<P, N>(
   client: P,
   target_address: Vec<Address>,
   events: impl IntoIterator<Item = impl AsRef<[u8]>>,
   from_block: u64,
   to_block: u64,
   concurrency: usize,
   block_range: u64,
   max_retries: usize,
) -> Result<Vec<Log>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   tracing::debug!(target: "zeus_eth::utils::lib",
      "Fetching logs from block {} to {}",
      from_block, to_block
   );

   let filter = Filter::new()
      .address(target_address)
      .events(events)
      .from_block(BlockNumberOrTag::Number(from_block))
      .to_block(BlockNumberOrTag::Number(to_block));

   let logs = Arc::new(Mutex::new(Vec::new()));
   let semaphore = Arc::new(Semaphore::new(concurrency));

   let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

   if to_block.saturating_sub(from_block) > block_range {
      let mut start_block = from_block;

      while start_block <= to_block {
         let end_block = std::cmp::min(start_block + block_range, to_block);
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

            let log_chunk = get_logs_with_retry(
               &client,
               &local_filter,
               start_block,
               end_block,
               max_retries,
            )
            .await?;

            let mut logs_lock = logs_clone.lock().await;
            logs_lock.extend(log_chunk);
            Ok(())
         });

         tasks.push(task);
         start_block = end_block + 1;
      }

      // Fail the whole fetch if any chunk exhausted its retries: partial logs
      // would silently miss events for the requested range.
      for task in tasks {
         match task.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(e) => return Err(e.into()),
         }
      }

      return Ok(Arc::try_unwrap(logs).unwrap().into_inner());
   }

   get_logs_with_retry(
      &client,
      &filter,
      from_block,
      to_block,
      max_retries,
   )
   .await
}

/// `eth_getLogs` for a single filter with linear backoff retries.
///
/// `max_retries` counts the retries after the first attempt (`0` = a single attempt).
/// Exhausting them returns the last RPC error.
async fn get_logs_with_retry<P, N>(
   client: &P,
   filter: &Filter,
   from_block: u64,
   to_block: u64,
   max_retries: usize,
) -> Result<Vec<Log>, anyhow::Error>
where
   P: Provider<N>,
   N: Network,
{
   let mut attempt = 0usize;

   loop {
      match client.get_logs(filter).await {
         Ok(logs) => return Ok(logs),
         Err(e) => {
            attempt += 1;

            if attempt > max_retries {
               tracing::error!(target: "zeus_eth::utils::lib",
                  "get_logs failed for blocks {}-{} after {} retries: {}",
                  from_block, to_block, max_retries, e
               );
               return Err(e.into());
            }

            let delay_ms = GET_LOGS_RETRY_BASE_DELAY_MS.saturating_mul(attempt as u64);
            tracing::debug!(target: "zeus_eth::utils::lib",
               "get_logs failed for blocks {}-{} (attempt {}/{}): {} - retrying in {}ms",
               from_block, to_block, attempt, max_retries, e, delay_ms
            );
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
         }
      }
   }
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
      format!(
         "{}...{}",
         &s[..prefix_len],
         &s[s.len() - suffix_len..]
      )
   }
}
