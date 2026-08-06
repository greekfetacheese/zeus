pub mod address_book;
pub mod batch;
pub mod block;
pub mod client;
pub mod numeric_value;
pub mod price_feed;

pub use numeric_value::NumericValue;

use alloy_contract::private::{Network, Provider};
use alloy_primitives::Address;
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log};

use std::sync::Arc;
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

   if latest_block.saturating_sub(from_block) > block_range {
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
