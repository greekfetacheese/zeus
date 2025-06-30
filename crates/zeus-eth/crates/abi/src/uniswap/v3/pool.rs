use IUniswapV3Pool::{Burn, Collect, Mint, Swap};
use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, Bytes, FixedBytes, LogData, U256};
use alloy_rpc_types::BlockId;
use alloy_sol_types::{SolCall, SolEvent, sol};

use anyhow::Context;

sol! {
    #[sol(rpc)]
    contract IUniswapV3Pool {

        // * EVENTS *

        event Initialize(uint160 sqrtPriceX96, int24 tick);
        event Mint(
            address sender,
            address indexed owner,
            int24 indexed tickLower,
            int24 indexed tickUpper,
            uint128 amount,
            uint256 amount0,
            uint256 amount1
        );
        event Collect(
            address indexed owner,
            address recipient,
            int24 indexed tickLower,
            int24 indexed tickUpper,
            uint128 amount0,
            uint128 amount1
        );
        event Burn(
            address indexed owner,
            int24 indexed tickLower,
            int24 indexed tickUpper,
            uint128 amount,
            uint256 amount0,
            uint256 amount1
        );
        event Swap(
            address indexed sender,
            address indexed recipient,
            int256 amount0,
            int256 amount1,
            uint160 sqrtPriceX96,
            uint128 liquidity,
            int24 tick
        );
        event Flash(
            address indexed sender,
            address indexed recipient,
            uint256 amount0,
            uint256 amount1,
            uint256 paid0,
            uint256 paid1
        );
        event IncreaseObservationCardinalityNext(
            uint16 observationCardinalityNextOld,
            uint16 observationCardinalityNextNew
        );
        event SetFeeProtocol(uint8 feeProtocol0Old, uint8 feeProtocol1Old, uint8 feeProtocol0New, uint8 feeProtocol1New);
        event CollectProtocol(address indexed sender, address indexed recipient, uint128 amount0, uint128 amount1);

        // * VIEW FUNCTIONS *

        function factory() external view returns (address);
        function fee() external view returns (uint24);
        function feeGrowthGlobal0X128() external view returns (uint256);
        function feeGrowthGlobal1X128() external view returns (uint256);
        function liquidity() external view returns (uint128);
        function maxLiquidityPerTick() external view returns (uint128);
        function observations(uint256) external view returns (uint32 blockTimestamp, int56 tickCumulative, uint160 secondsPerLiquidityCumulativeX128, bool initialized);
        function observe(uint32[] secondsAgos) external view returns (int56[] tickCumulatives, uint160[] secondsPerLiquidityCumulativeX128s);
        function positions(bytes32) external view returns (uint128 liquidity, uint256 feeGrowthInside0LastX128, uint256 feeGrowthInside1LastX128, uint128 tokensOwed0, uint128 tokensOwed1);
        function protocolFees() external view returns (uint128 token0, uint128 token1);
        function slot0() external view returns (uint160, int24, uint16, uint16, uint16, uint8, bool);
        function snapshotCumulativeInside(int24 tickLower, int24 tickUpper) external view returns (int56 tickCumulativeInside, uint160 secondsPerLiquidityInsideX128, uint32 secondsInside);
        function tickBitmap(int16 wordPosition) external view returns (uint256);
        function tickSpacing() external view returns (int24);
        function ticks(int24 tick) external view returns (uint128, int128, uint256, uint256, int56, uint160, uint32, bool);
        function token0() external view returns (address);
        function token1() external view returns (address);

        // * WRITE FUNCTIONS *

        function burn(int24 tickLower, int24 tickUpper, uint128 amount) external;
        function collect(address recipient, int24 tickLower, int24 tickUpper, uint128 amount0Requested, uint128 amount1Requested) external;
        function collectProtocol(address recipient, uint128 amount0Requested, uint128 amount1Requested) external;
        function flash(
            address recipient,
            uint256 amount0,
            uint256 amount1,
            bytes data
        ) external;
        function increaseObservationCardinalityNext(uint16 observationCardinalityNext) external;
        function initialize(uint160 sqrtPriceX96) external;
        function mint(address recipient, int24 tickLower, int24 tickUpper, uint128 amount, bytes data) external;
        function setFeeProtocol(uint8 feeProtocol0, uint8 feeProtocol1) external;
        function swap(
            address recipient,
            bool zeroForOne,
            int256 amountSpecified,
            bytes data
        ) external;
    }
}

pub fn swap_signature() -> &'static str {
   IUniswapV3Pool::swapCall::SIGNATURE
}

pub fn swap_selector() -> [u8; 4] {
   IUniswapV3Pool::swapCall::SELECTOR
}

/// Return the factory address that created this pool
pub async fn factory<P, N>(pool_address: Address, client: P) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IUniswapV3Pool::new(pool_address, client);
   let factory = contract.factory().call().await?;
   Ok(factory)
}

/// Return the fee of this pool
pub async fn fee<P, N>(pool_address: Address, client: P) -> Result<u32, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IUniswapV3Pool::new(pool_address, client);
   let fee = contract.fee().call().await?;
   let fee: u32 = fee.to_string().parse().context("Failed to parse fee")?;
   Ok(fee)
}

/// Returns the information about a position by the position's key
pub async fn positions<P, N>(
   pool_address: Address,
   key: FixedBytes<32>,
   client: P,
   block_id: Option<BlockId>,
) -> Result<(u128, U256, U256, u128, u128), anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block_id.unwrap_or(BlockId::latest());

   let contract = IUniswapV3Pool::new(pool_address, client);
   let positions = contract.positions(key).block(block).call().await?;

   Ok((
      positions.liquidity,
      positions.feeGrowthInside0LastX128,
      positions.feeGrowthInside1LastX128,
      positions.tokensOwed0,
      positions.tokensOwed1,
   ))
}

/// Return the token0 of this pool
pub async fn token0<P, N>(pool_address: Address, client: P) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IUniswapV3Pool::new(pool_address, client);
   let token0 = contract.token0().call().await?;
   Ok(token0)
}

/// Return the token1 of this pool
pub async fn token1<P, N>(pool_address: Address, client: P) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IUniswapV3Pool::new(pool_address, client);
   let token1 = contract.token1().call().await?;
   Ok(token1)
}

pub fn decode_swap_log(log: &LogData) -> Result<Swap, anyhow::Error> {
   let b = IUniswapV3Pool::Swap::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_mint_log(log: &LogData) -> Result<Mint, anyhow::Error> {
   let b = IUniswapV3Pool::Mint::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_burn_log(log: &LogData) -> Result<Burn, anyhow::Error> {
   let b = IUniswapV3Pool::Burn::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_collect_log(log: &LogData) -> Result<Collect, anyhow::Error> {
   let b = IUniswapV3Pool::Collect::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

pub fn decode_positions(data: &Bytes) -> Result<(u128, U256, U256, u128, u128), anyhow::Error> {
   let abi = IUniswapV3Pool::positionsCall::abi_decode_returns(data)?;
   Ok((
      abi.liquidity,
      abi.feeGrowthInside0LastX128,
      abi.feeGrowthInside1LastX128,
      abi.tokensOwed0,
      abi.tokensOwed1,
   ))
}
