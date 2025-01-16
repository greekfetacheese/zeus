use alloy_primitives::Address;
use alloy_rpc_types::{BlockNumberOrTag, Filter, Log};

use alloy_contract::private::Network;
use alloy_provider::Provider;
use alloy_transport::Transport;

use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;

use super::super::BlockTime;
use tracing::trace;

/// Get logs based on a filter
///
/// ## Arguments
///
/// * `client` - The provider client
/// * `chain_id` - The chain id
/// * `target_address` - The addresses you want to get logs for
/// * `events` - The events you want to get logs for
/// * `block_time` - The time range you want to get logs for
pub async fn get_logs_for<T, P, N>(
    client: P,
    chain_id: u64,
    target_address: Vec<Address>,
    events: impl IntoIterator<Item = impl AsRef<[u8]>>,
    block_time: BlockTime,
) -> Result<Vec<Log>, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone + 'static,
    N: Network,
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
    let semaphore = Arc::new(Semaphore::new(5));

    let mut tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

    if latest_block - from_block > 100_000 {
        let mut start_block = from_block;

        while start_block <= latest_block {
            let end_block = std::cmp::min(start_block + 100_000, latest_block);
            let client_clone = client.clone();
            let logs_clone = Arc::clone(&logs);
            let filter_clone = filter.clone();
            let permit = Arc::clone(&semaphore).acquire_owned().await?;

            trace!("Quering Logs for block range: {} - {}", start_block, end_block);

            let task = tokio::spawn(async move {
                let local_filter = filter_clone
                    .from_block(BlockNumberOrTag::Number(start_block))
                    .to_block(BlockNumberOrTag::Number(end_block));

                let log_chunk = client_clone.get_logs(&local_filter).await?;
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