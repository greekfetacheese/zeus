use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, FixedBytes};
use alloy_rpc_types::BlockId;

use super::address_book::zeus_stateview_v2;
use crate::{
   abi::zeus::ZeusStateViewV2::{self, *},
   utils::address_book,
};

/// Query the ETH balance for the given addresses
pub async fn get_eth_balances<P, N>(
   client: P,
   chain: u64,
   block: Option<BlockId>,
   addresses: Vec<Address>,
) -> Result<Vec<ETHBalance>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
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
) -> Result<Vec<ERC20Balance>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let balance = contract
      .getERC20Balance(tokens, owner)
      .call()
      .block(block)
      .await?;
   Ok(balance)
}

/// Query the ERC20 token info for the given token
pub async fn get_erc20_info<P, N>(client: P, chain: u64, token: Address) -> Result<ERC20Info, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let info = contract.getERC20Info(token).call().await?;
   Ok(info)
}

/// Query the ERC20 token info for the given tokens
pub async fn get_erc20_tokens<P, N>(
   client: P,
   chain: u64,
   tokens: Vec<Address>,
) -> Result<Vec<ERC20Info>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let info = contract.getERC20InfoBatch(tokens).call().await?;
   Ok(info)
}

/// Get all possible pools based on the token pairs and fee tiers
pub async fn get_pools<P, N>(
   client: P,
   chain: u64,
   v2_factory: Address,
   v3_factory: Address,
   state_view: Address,
   v4_pools: Vec<FixedBytes<32>>,
   base_tokens: Vec<Address>,
   quote_token: Address,
) -> Result<Pools, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let pools = contract
      .getPools(
         v2_factory,
         v3_factory,
         state_view,
         v4_pools,
         base_tokens,
         quote_token,
      )
      .call()
      .await?;
   Ok(pools)
}

/// Get the pools state for the given pools
pub async fn get_pools_state<P, N>(
   client: P,
   chain: u64,
   v2_pools: Vec<Address>,
   v3_pools: Vec<V3Pool>,
   v4_pools: Vec<V4Pool>,
   state_view: Address,
) -> Result<PoolsState, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let pools_state = contract
      .getPoolsState(v2_pools, v3_pools, v4_pools, state_view)
      .call()
      .await?;
   Ok(pools_state)
}

/// Get all possible V3 pools based on token pair
pub async fn get_v3_pools<P, N>(
   client: P,
   chain: u64,
   factory: Address,
   token_a: Address,
   token_b: Address,
) -> Result<Vec<V3Pool>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
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
   let address = zeus_stateview_v2(chain)?;
   let stateview = address_book::uniswap_v4_stateview(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let pools = contract.validateV4Pools(stateview, pools).call().await?;
   Ok(pools)
}

/// Query the reserves for the given v2 pools
pub async fn get_v2_reserves<P, N>(
   client: P,
   chain: u64,
   pools: Vec<Address>,
) -> Result<Vec<V2PoolReserves>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let reserves = contract.getV2Reserves(pools).call().await?;
   Ok(reserves)
}

/// Query the state of multiple V3 pools
pub async fn get_v3_state<P, N>(client: P, chain: u64, pools: Vec<V3Pool>) -> Result<Vec<V3PoolData>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let state = contract.getV3PoolState(pools).call().await?;
   Ok(state)
}

/// Query the state of multiple V4 pools
pub async fn get_v4_pool_state<P, N>(
   client: P,
   chain: u64,
   pools: Vec<V4Pool>,
) -> Result<Vec<V4PoolData>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let address = zeus_stateview_v2(chain)?;
   let stateview = address_book::uniswap_v4_stateview(chain)?;
   let contract = ZeusStateViewV2::new(address, client);
   let state = contract.getV4PoolState(pools, stateview).call().await?;
   Ok(state)
}
