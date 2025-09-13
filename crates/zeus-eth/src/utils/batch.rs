use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, FixedBytes};
use alloy_rpc_types::BlockId;

use super::address_book::zeus_stateview;
use crate::{abi::zeus::ZeusStateView, utils::address_book};

/// Query the ETH balance for the given addresses
pub async fn get_eth_balances<P, N>(
   client: P,
   chain: u64,
   block: Option<BlockId>,
   addresses: Vec<Address>,
) -> Result<Vec<ZeusStateView::ETHBalance>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let address = zeus_stateview(chain)?;
   let contract = ZeusStateView::new(address, client);
   let balance = contract
      .getETHBalance(addresses)
      .call()
      .block(block)
      .await?;
   Ok(balance)
}

/// Query the balance of multiple ERC20 tokens for the given owner
pub async fn get_erc20_balances<P, N>(
   client: P,
   chain: u64,
   block: Option<BlockId>,
   owner: Address,
   tokens: Vec<Address>,
) -> Result<Vec<ZeusStateView::ERC20Balance>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let address = zeus_stateview(chain)?;
   let contract = ZeusStateView::new(address, client);
   let balance = contract
      .getERC20Balance(tokens, owner)
      .call()
      .block(block)
      .await?;
   Ok(balance)
}

/// Query the ERC20 token info for the given token
pub async fn get_erc20_info<P, N>(
   client: P,
   chain: u64,
   token: Address,
) -> Result<ZeusStateView::ERC20Info, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview(chain)?;
   let contract = ZeusStateView::new(address, client);
   let info = contract.getERC20Info(token).call().await?;
   Ok(info)
}

/// Query the ERC20 token info for the given tokens
pub async fn get_erc20_tokens<P, N>(
   client: P,
   chain: u64,
   tokens: Vec<Address>,
) -> Result<Vec<ZeusStateView::ERC20Info>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview(chain)?;
   let contract = ZeusStateView::new(address, client);
   let info = contract.getERC20InfoBatch(tokens).call().await?;
   Ok(info)
}

/// Get all possible V3 pools based on token pair
pub async fn get_v3_pools<P, N>(
   client: P,
   chain: u64,
   factory: Address,
   token_a: Address,
   token_b: Address,
) -> Result<Vec<ZeusStateView::V3Pool>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview(chain)?;
   let contract = ZeusStateView::new(address, client);
   let pools = contract
      .getV3Pools(factory, token_a, token_b)
      .call()
      .await?;
   Ok(pools)
}

/// Validate the given V4 pools
pub async fn validate_v4_pools<P, N>(
   client: P,
   chain: u64,
   pools: Vec<FixedBytes<32>>,
) -> Result<Vec<FixedBytes<32>>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview(chain)?;
   let stateview = address_book::uniswap_v4_stateview(chain)?;
   let contract = ZeusStateView::new(address, client);
   let pools = contract.validateV4Pools(stateview, pools).call().await?;
   Ok(pools)
}

/// Query the reserves for the given v2 pools
pub async fn get_v2_reserves<P, N>(
   client: P,
   chain: u64,
   pools: Vec<Address>,
) -> Result<Vec<ZeusStateView::V2PoolReserves>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview(chain)?;
   let contract = ZeusStateView::new(address, client);
   let reserves = contract.getV2Reserves(pools).call().await?;
   Ok(reserves)
}

/// Query the state of multiple V3 pools
pub async fn get_v3_state<P, N>(
   client: P,
   chain: u64,
   pools: Vec<ZeusStateView::V3Pool>,
) -> Result<Vec<ZeusStateView::V3PoolData>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview(chain)?;
   let contract = ZeusStateView::new(address, client);
   let state = contract.getV3PoolState(pools).call().await?;
   Ok(state)
}

/// Query the state of multiple V4 pools
pub async fn get_v4_pool_state<P, N>(
   client: P,
   chain: u64,
   pools: Vec<ZeusStateView::V4Pool>,
) -> Result<Vec<ZeusStateView::V4PoolData>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview(chain)?;
   let stateview = address_book::uniswap_v4_stateview(chain)?;
   let contract = ZeusStateView::new(address, client);
   let state = contract.getV4PoolState(pools, stateview).call().await?;
   Ok(state)
}
