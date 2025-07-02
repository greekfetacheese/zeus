use alloy_primitives::{Address, B256, aliases::I24};
use alloy_rpc_types::BlockId;
use alloy_sol_types::{SolValue, sol};

use alloy_contract::private::{Network, Provider};
use anyhow::anyhow;

pub use Get_V3PoolState::V3Pool;
pub use Get_V4PoolState::V4Pool;

sol! {
    #[sol(rpc)]
    ERC20GetBalance,
    "src/abi/ERC20GetBalance.json",
}

sol! {
    #[sol(rpc)]
    GetERC20,
    "src/abi/GetERC20.json",
}

sol! {
    #[sol(rpc)]
    GetERC20Batch,
    "src/abi/GetERC20Batch.json",
}

sol! {
    #[sol(rpc)]
    GetV2PoolsReserves,
    "src/abi/GetV2PoolReserves.json",
}

sol! {
    #[sol(rpc)]
    GetV3Pools,
    "src/abi/GetV3Pools.json",
}

sol! {
    #[sol(rpc)]
    GetV3PoolState,
    "src/abi/Get_V3PoolState.json",
}

sol! {
    #[sol(rpc)]
    GetV4PoolState,
    "src/abi/Get_V4PoolState.json",
}

sol! {
   #[sol(rpc)]
   TickDataAggregatorV4,
   "src/abi/TickDataAggregatorV4.json",
}

sol! {
   #[sol(rpc)]
   TickBitmapFetchV3,
   "src/abi/TickBitmapFetchV3.json",
}

sol! {
   #[sol(rpc)]
   TickDataFetchV3,
   "src/abi/TickDataFetchV3.json",
}

sol! {

    #[derive(Debug)]
    struct TokenBalance {
        address token;
        uint256 balance;
    }

    #[derive(Debug)]
    struct ERC20Info {
        address addr;
        string symbol;
        string name;
        uint256 totalSupply;
        uint8 decimals;
    }

    #[derive(Debug)]
        struct ERC20 {
        string symbol;
        string name;
        uint256 totalSupply;
        uint8 decimals;
    }

    #[derive(Debug)]
    struct V2PoolReserves {
        address pool;
        uint112 reserve0;
        uint112 reserve1;
        uint32 blockTimestampLast;
    }

    #[derive(Debug)]
    struct V3PoolInfo {
        address addr;
        address token0;
        address token1;
        uint24 fee;
    }

    #[derive(Debug)]
    struct V3PoolData {
        address pool;
        uint256 token0Balance;
        uint256 token1Balance;
        uint256 feeGrowthGlobal0X128;
        uint256 feeGrowthGlobal1X128;
        uint256 feeGrowthOutside0X128;
        uint256 feeGrowthOutside1X128;
        uint128 liquidity;
        uint160 sqrtPriceX96;
        int24 tick;
        uint256 tickBitmap;
        int16 wordPos;
        int128 liquidityNet;
        uint128 liquidityGross;
        bool initialized;
    }

    #[derive(Debug)]
        struct V4PoolData {
        bytes32 pool;
        uint256 feeGrowthGlobal0;
        uint256 feeGrowthGlobal1;
        uint256 feeGrowthOutside0X128;
        uint256 feeGrowthOutside1X128;
        uint128 liquidity;
        uint160 sqrtPriceX96;
        int24 tick;
        uint256 tickBitmap;
        int16 wordPos;
        int128 liquidityNet;
        uint128 liquidityGross;
    }

    #[derive(Debug)]
    struct PoolInfo {
      address pool;
      int24 tickSpacing;
      int16 minWord;
      int16 maxWord;
  }

    #[derive(Debug)]
      struct TickData {
        int24 actualTick;
        uint128 liquidityGross;
        int128 liquidityNet;
    }

    #[derive(Debug)]
      struct TickInfo {
        int24 tick;
        uint128 liquidityGross;
        int128 liquidityNet;
        uint256 feeGrowthOutside0X128;
        uint256 feeGrowthOutside1X128;
        bool initialized;
  }

  #[derive(Debug)]
   struct TickBitMap {
      int16 wordPos;
      uint256 bitmap;
    }

  #[derive(Debug)]
      struct AggregatedTicks {
      TickData[] allTicksInfo;
      uint256[] populatedBitmapWords;
      int16[] correspondingWordPositions;
  }

}

/// Query the balance of multiple ERC20 tokens for the given owner
///
/// If `block` is None, the latest block is used
pub async fn get_erc20_balances<P, N>(
   client: P,
   block: Option<BlockId>,
   owner: Address,
   tokens: Vec<Address>,
) -> Result<Vec<TokenBalance>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = ERC20GetBalance::deploy_builder(client, tokens, owner).block(block);
   let res = deployer.call_raw().await?;

   let data = <Vec<TokenBalance> as SolValue>::abi_decode(&res)
      .map_err(|e| anyhow!("Failed to decode token balances: {:?}", e))?;
   Ok(data)
}

/// Query the ERC20 token info for the given token
pub async fn get_erc20_info<P, N>(client: P, token: Address) -> Result<ERC20, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let deployer = GetERC20::deploy_builder(client, token);
   let res = deployer.call_raw().await?;

   let data = <ERC20 as SolValue>::abi_decode(&res).map_err(|e| anyhow!("Failed to decode token info: {:?}", e))?;

   Ok(data)
}

/// Query the ERC20 token info for the given tokens
pub async fn get_erc20_tokens<P, N>(client: P, tokens: Vec<Address>) -> Result<Vec<ERC20Info>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let deployer = GetERC20Batch::deploy_builder(client, tokens);
   let res = deployer.call_raw().await?;

   let data =
      <Vec<ERC20Info> as SolValue>::abi_decode(&res).map_err(|e| anyhow!("Failed to decode token info: {:?}", e))?;

   Ok(data)
}

/// Query the reserves for the given v2 pools
///
/// If `block` is None, the latest block is used
pub async fn get_v2_pool_reserves<P, N>(
   client: P,
   block: Option<BlockId>,
   pools: Vec<Address>,
) -> Result<Vec<V2PoolReserves>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = GetV2PoolsReserves::deploy_builder(client, pools).block(block);
   let res = deployer.call_raw().await?;

   let data = <Vec<V2PoolReserves> as SolValue>::abi_decode(&res)
      .map_err(|e| anyhow!("Failed to decode V2 pool reserves: {:?}", e))?;

   Ok(data)
}

/// Retrieve all V3 pools for tokenA and tokenB based on the fee tiers
///
/// If no pools exists it will return an empty vector
pub async fn get_v3_pools<P, N>(
   client: P,
   token_a: Address,
   token_b: Address,
   factory: Address,
) -> Result<Vec<V3PoolInfo>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let deployer = GetV3Pools::deploy_builder(client, factory, token_a, token_b);
   let res = deployer.call_raw().await?;

   let data =
      <Vec<V3PoolInfo> as SolValue>::abi_decode(&res).map_err(|e| anyhow!("Failed to decode V3 pools: {:?}", e))?;

   Ok(data)
}

/// Query the state of multiple V3 pools
///
/// If `block` is `None`, the latest block is used.
pub async fn get_v3_state<P, N>(
   client: P,
   block: Option<BlockId>,
   pools: Vec<V3Pool>,
) -> Result<Vec<V3PoolData>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = GetV3PoolState::deploy_builder(client, pools).block(block);
   let res = deployer.call_raw().await?;

   let data =
      <Vec<V3PoolData> as SolValue>::abi_decode(&res).map_err(|e| anyhow!("Failed to decode V3 pool data: {:?}", e))?;

   Ok(data)
}

/// Get the given ticks for a V3 pool
/// 
/// If `block` is `None`, the latest block is used.
pub async fn get_v3_pool_ticks<P, N>(
   client: P,
   pool: TickDataFetchV3::PoolInfo,
   block: Option<BlockId>,
) -> Result<Vec<TickInfo>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = TickDataFetchV3::deploy_builder(client, pool).block(block);
   let res = deployer.call_raw().await?;

   let data =
      <Vec<TickInfo> as SolValue>::abi_decode(&res).map_err(|e| anyhow!("Failed to decode V3 pool tick data: {:?}", e))?;

   Ok(data)
}

/// Get the tickBitmaps of a v3 pool for the given word positions
/// 
/// If `block` is `None`, the latest block is used.
pub async fn get_v3_pool_tick_bitmaps<P, N>(
   client: P,
   pool: TickBitmapFetchV3::PoolInfo,
   block: Option<BlockId>,
) -> Result<Vec<TickBitMap>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = TickBitmapFetchV3::deploy_builder(client, pool).block(block);
   let res = deployer.call_raw().await?;

   let data =
      <Vec<TickBitMap> as SolValue>::abi_decode(&res).map_err(|e| anyhow!("Failed to decode V3 pool tick data: {:?}", e))?;

   Ok(data)
}

/// Query the state of multiple V4 pools
///
/// If `block` is `None`, the latest block is used.
pub async fn get_v4_pool_state<P, N>(
   client: P,
   pools: Vec<V4Pool>,
   state_view: Address,
   block: Option<BlockId>,
) -> Result<Vec<V4PoolData>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = GetV4PoolState::deploy_builder(client, pools, state_view).block(block);
   let res = deployer.call_raw().await?;

   let data =
      <Vec<V4PoolData> as SolValue>::abi_decode(&res).map_err(|e| anyhow!("Failed to decode V4 pool data: {:?}", e))?;

   Ok(data)
}

/// Query the tick data for the given minWord and maxWord
///
/// If `block` is None, the latest block is used
pub async fn get_v4_pool_tick_data<P, N>(
   client: P,
   pool_id: B256,
   state_view: Address,
   min_word: i16,
   max_word: i16,
   tick_spacing: I24,
   block: Option<BlockId>,
) -> Result<AggregatedTicks, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = TickDataAggregatorV4::deploy_builder(
      client,
      pool_id,
      state_view,
      min_word,
      max_word,
      tick_spacing,
   )
   .block(block);
   let res = deployer.call_raw().await?;

   let data =
      <AggregatedTicks as SolValue>::abi_decode(&res).map_err(|e| anyhow!("Failed to decode V4 pool data: {:?}", e))?;
   Ok(data)
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::address_book;
   use alloy_primitives::address;
   use alloy_provider::ProviderBuilder;
   use url::Url;

   #[tokio::test]
   async fn test_erc20_balance() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let weth = address_book::weth(1).unwrap();
      let usdc = address_book::usdc(1).unwrap();

      let owner = Address::ZERO;

      let tokens = vec![weth, usdc];

      let balances = get_erc20_balances(client, None, owner, tokens)
         .await
         .unwrap();

      assert_eq!(balances.len(), 2);
   }

   #[tokio::test]
   async fn test_erc20_info() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let weth = address_book::weth(1).unwrap();

      let weth_info = get_erc20_info(client.clone(), weth).await.unwrap();

      assert_eq!(&weth_info.symbol, "WETH");
      assert_eq!(&weth_info.name, "Wrapped Ether");
      assert_eq!(weth_info.decimals, 18);
   }

   #[tokio::test]
   async fn test_v2_pool_reserves() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let pool = address!("0d4a11d5EEaaC28EC3F61d100daF4d40471f1852");

      let reserves = get_v2_pool_reserves(client, None, vec![pool])
         .await
         .unwrap();

      assert_eq!(reserves.len(), 1);

      println!("=== V2 Pool Reserves Test ===");
      for reserve in reserves {
         println!(
            "Pool: {:?}, Reserves: {}, {}",
            reserve.pool, reserve.reserve0, reserve.reserve1
         );
      }
   }

   #[tokio::test]
   async fn test_v3_pools() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let weth = address_book::weth(1).unwrap();
      let usdc = address_book::usdc(1).unwrap();

      let factory = address!("1F98431c8aD98523631AE4a59f267346ea31F984");

      let pools = get_v3_pools(client, weth, usdc, factory).await.unwrap();

      assert_eq!(pools.len(), 4);

      println!("=== V3 Pairs Test ===");
      for pool in pools {
         println!("Pair: {:?}, Fee: {}", pool.addr, pool.fee);
      }
   }
}
