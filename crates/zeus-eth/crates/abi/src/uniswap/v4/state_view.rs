use alloy_contract::private::{Network, Provider};
use alloy_primitives::{
   Address, B256, U256,
   aliases::I24,
};
use alloy_rpc_types::BlockId;
use alloy_sol_types::sol;

sol! {
    #[sol(rpc)]
    contract StateView {

        type PoolId is bytes32;

        function getSlot0(PoolId poolId)
        external
        view
        returns (uint160 sqrtPriceX96, int24 tick, uint24 protocolFee, uint24 lpFee);

        function getTickInfo(PoolId poolId, int24 tick)
        external
        view
        returns (
            uint128 liquidityGross,
            int128 liquidityNet,
            uint256 feeGrowthOutside0X128,
            uint256 feeGrowthOutside1X128
        );

        function getTickLiquidity(PoolId poolId, int24 tick)
        external
        view
        returns (uint128 liquidityGross, int128 liquidityNet);

        function getTickFeeGrowthOutside(PoolId poolId, int24 tick)
        external
        view
        returns (uint256 feeGrowthOutside0X128, uint256 feeGrowthOutside1X128);

        function getFeeGrowthGlobals(PoolId poolId)
        external
        view
        returns (uint256 feeGrowthGlobal0, uint256 feeGrowthGlobal1);

        function getLiquidity(PoolId poolId) external view returns (uint128 liquidity);

        function getTickBitmap(PoolId poolId, int16 tick) external view returns (uint256 tickBitmap);

        function getPositionInfo(PoolId poolId, bytes32 positionId)
        external
        view
        returns (uint128 liquidity, uint256 feeGrowthInside0LastX128, uint256 feeGrowthInside1LastX128);

        function getPositionLiquidity(PoolId poolId, bytes32 positionId) external view returns (uint128 liquidity);

        function getFeeGrowthInside(PoolId poolId, int24 tickLower, int24 tickUpper);
    }
}

pub async fn get_slot0<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   block: Option<BlockId>,
) -> Result<StateView::getSlot0Return, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract.getSlot0(pool_id).block(block).call().await?;
   Ok(res)
}

pub async fn get_tick_info<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   tick: i32,
   block: Option<BlockId>,
) -> Result<StateView::getTickInfoReturn, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract
      .getTickInfo(pool_id, tick.try_into()?)
      .block(block)
      .call()
      .await?;
   Ok(res)
}

pub async fn get_tick_liquidity<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   tick: I24,
   block: Option<BlockId>,
) -> Result<StateView::getTickLiquidityReturn, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract
      .getTickLiquidity(pool_id, tick)
      .block(block)
      .call()
      .await?;
   Ok(res)
}

pub async fn get_tick_fee_growth_outside<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   tick: i32,
   block: Option<BlockId>,
) -> Result<StateView::getTickFeeGrowthOutsideReturn, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract
      .getTickFeeGrowthOutside(pool_id, tick.try_into()?)
      .block(block)
      .call()
      .await?;
   Ok(res)
}

pub async fn get_fee_growth_globals<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   block: Option<BlockId>,
) -> Result<StateView::getFeeGrowthGlobalsReturn, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract
      .getFeeGrowthGlobals(pool_id)
      .block(block)
      .call()
      .await?;
   Ok(res)
}

pub async fn get_liquidity<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   block: Option<BlockId>,
) -> Result<u128, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract.getLiquidity(pool_id).block(block).call().await?;
   Ok(res)
}

pub async fn get_tick_bitmap<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   tick: i16,
   block: Option<BlockId>,
) -> Result<U256, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract
      .getTickBitmap(pool_id, tick)
      .block(block)
      .call()
      .await?;
   Ok(res)
}

pub async fn get_position_info<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   position_id: B256,
   block: Option<BlockId>,
) -> Result<StateView::getPositionInfoReturn, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract
      .getPositionInfo(pool_id, position_id)
      .block(block)
      .call()
      .await?;
   Ok(res)
}

pub async fn get_position_liquidity<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   position_id: B256,
   block: Option<BlockId>,
) -> Result<u128, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract
      .getPositionLiquidity(pool_id, position_id)
      .block(block)
      .call()
      .await?;
   Ok(res)
}

pub async fn get_fee_growth_inside<P, N>(
   client: P,
   contract: Address,
   pool_id: B256,
   tick_lower: i32,
   tick_upper: i32,
   block: Option<BlockId>,
) -> Result<StateView::getFeeGrowthInsideReturn, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let contract = StateView::new(contract, client);
   let res = contract
      .getFeeGrowthInside(pool_id, tick_lower.try_into()?, tick_upper.try_into()?)
      .block(block)
      .call()
      .await?;
   Ok(res)
}
