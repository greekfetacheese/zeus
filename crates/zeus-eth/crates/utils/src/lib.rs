pub mod address;
pub mod price_feed;
pub mod batch_request;
pub mod client;

use alloy_primitives::Address;
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log};

use alloy_contract::private::{Network, Provider};

use types::BlockTime;
use std::sync::Arc;
use tokio::{sync::{Mutex, Semaphore}, task::JoinHandle};
use tracing::trace;

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
    N: Network
{
    let latest_block = client.get_block_number().await?;
    let from_block = block_time.go_back(chain_id, latest_block)?;

    trace!("Fetching logs from block {} to {}", from_block, latest_block);

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

            trace!("Quering Logs for block range: {} - {}", start_block, end_block);

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