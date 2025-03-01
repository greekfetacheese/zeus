use alloy_primitives::Address;
use alloy_rpc_types::BlockId;
use alloy_sol_types::{SolValue, sol};

use alloy_contract::private::{Network, Provider};

pub use crate::batch_request::GetV3State::V3Pool2;
use anyhow::anyhow;

sol! {
    #[sol(rpc)]
    IGetErc20Balance,
    "src/batch_request/abi/GetErc20Balance.json",
}

sol! {
    #[sol(rpc)]
    IGetERC20,
    "src/batch_request/abi/GetERC20.json",
}

sol! {
    #[sol(rpc)]
    IGetV2PoolsReserves,
    "src/batch_request/abi/GetV2PoolsReserves.json",
}

sol! {
    #[sol(rpc)]
    IGetV3Pools,
    "src/batch_request/abi/GetV3Pools.json",
}

sol! {
    #[sol(rpc)]
    IGetV3State,
    "src/batch_request/abi/GetV3State.json",
}

sol! {
    #[derive(Debug)]
    struct TokenBalance {
        address token;
        uint256 balance;
    }
}

sol! {
    #[derive(Debug)]
    struct ERC20Info {
        string symbol;
        string name;
        uint256 totalSupply;
        uint8 decimals;
    }
}

sol! {
    #[derive(Debug)]
    struct V2PoolReserves {
        address pool;
        uint112 reserve0;
        uint112 reserve1;
        uint32 blockTimestampLast;
    }
}

sol! {
    #[derive(Debug)]
    struct V3Pool {
        address addr;
        address token0;
        address token1;
        uint24 fee;
    }
}

sol! {
    #[derive(Debug)]
    struct V3PoolData {
        address pool;
        uint256 base_token_liquidity;
        uint128 liquidity;
        uint160 sqrtPrice;
        int24 tick;
        int24 tickSpacing;
        uint256 tickBitmap;
        int16 wordPos;
        int128 liquidityNet;
        uint128 liquidityGross;
        bool initialized;
    }
}

/// Get the balance for the given ERC20 tokens for the owner at the given block, if block is None the latest block is used
pub async fn get_erc20_balance<P, N>(
   client: P,
   block: Option<BlockId>,
   owner: Address,
   tokens: Vec<Address>,
) -> Result<Vec<TokenBalance>, anyhow::Error>
where
   P: Provider<(), N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = IGetErc20Balance::deploy_builder(client, tokens, owner).block(block);
   let res = deployer.call_raw().await?;

   let data = <Vec<TokenBalance> as SolValue>::abi_decode(&res, false)
      .map_err(|e| anyhow!("Failed to decode token balances: {:?}", e))?;
   Ok(data)
}

/// Get the ERC20 token info
pub async fn get_erc20_info<P, N>(client: P, token: Address) -> Result<ERC20Info, anyhow::Error>
where
   P: Provider<(), N> + Clone + 'static,
   N: Network,
{
   let deployer = IGetERC20::deploy_builder(client, token);
   let res = deployer.call_raw().await?;

   let data =
      <ERC20Info as SolValue>::abi_decode(&res, false).map_err(|e| anyhow!("Failed to decode token info: {:?}", e))?;

   Ok(data)
}

/// Retrieve all V3 pools for tokenA and tokenB based on the fee tiers (if they exist)
///
/// For any possible pool that does not exist the values will be 0
///
/// If no pools exists it will still return a vector with zero values
pub async fn get_v3_pools<P, N>(
   client: P,
   token_a: Address,
   token_b: Address,
   factory: Address,
) -> Result<Vec<V3Pool>, anyhow::Error>
where
   P: Provider<(), N> + Clone + 'static,
   N: Network,
{
   let deployer = IGetV3Pools::deploy_builder(client, factory, token_a, token_b);
   let res = deployer.call_raw().await?;

   let data =
      <Vec<V3Pool> as SolValue>::abi_decode(&res, false).map_err(|e| anyhow!("Failed to decode V3 pools: {:?}", e))?;

   Ok(data)
}

/// Get the reserves for the given v2 pools, if block is None, then the latest block is used
///
/// To avoid the `CreateContractSizeLimit` EVM error, a safe limit of 100 pools should work for all chains
pub async fn get_v2_pool_reserves<P, N>(
   client: P,
   block: Option<BlockId>,
   pools: Vec<Address>,
) -> Result<Vec<V2PoolReserves>, anyhow::Error>
where
   P: Provider<(), N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = IGetV2PoolsReserves::deploy_builder(client, pools).block(block);
   let res = deployer.call_raw().await?;

   let data = <Vec<V2PoolReserves> as SolValue>::abi_decode(&res, false)
      .map_err(|e| anyhow!("Failed to decode V2 pool reserves: {:?}", e))?;

   Ok(data)
}

/// Retrieve the state for the  given v3 pools, if block is None, then the latest block is used
///
/// To avoid the `CreateContractSizeLimit` EVM error, a safe limit of 10 pools should work for all chains
pub async fn get_v3_state<P, N>(
   client: P,
   block: Option<BlockId>,
   pools: Vec<V3Pool2>,
) -> Result<Vec<V3PoolData>, anyhow::Error>
where
   P: Provider<(), N> + Clone + 'static,
   N: Network,
{
   let block = block.unwrap_or(BlockId::latest());
   let deployer = IGetV3State::deploy_builder(client, pools).block(block);
   let res = deployer.call_raw().await?;

   let data = <Vec<V3PoolData> as SolValue>::abi_decode(&res, false)
      .map_err(|e| anyhow!("Failed to decode V3 pool data: {:?}", e))?;

   Ok(data)
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::address;
   use alloy_primitives::address;
   use alloy_provider::ProviderBuilder;
   use url::Url;

   #[tokio::test]
   async fn test_erc20_balance() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let weth = address::weth(1).unwrap();
      let usdc = address::usdc(1).unwrap();

      let owner = Address::ZERO;

      let tokens = vec![weth, usdc];

      let balances = get_erc20_balance(client, None, owner, tokens)
         .await
         .unwrap();

      assert_eq!(balances.len(), 2);
   }

   #[tokio::test]
   async fn test_erc20_balance_limit() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let weth = address::weth(1).unwrap();
      let owner = Address::ZERO;

      let mut tokens = Vec::new();
      for _ in 0..200 {
         tokens.push(weth);
      }

      let balances = get_erc20_balance(client, None, owner, tokens)
         .await
         .unwrap();

      assert_eq!(balances.len(), 200);
   }

   #[tokio::test]
   async fn test_erc20_info() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let weth = address::weth(1).unwrap();
      let usdc = address::usdc(1).unwrap();

      let weth_info = get_erc20_info(client.clone(), weth).await.unwrap();
      let usdc_info = get_erc20_info(client.clone(), usdc).await.unwrap();

      assert_eq!(&weth_info.symbol, "WETH");
      assert_eq!(&usdc_info.symbol, "USDC");

      assert_eq!(weth_info.decimals, 18);
      assert_eq!(usdc_info.decimals, 6);
   }

   #[tokio::test]
   async fn test_v3_pools() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let weth = address::weth(1).unwrap();
      let usdc = address::usdc(1).unwrap();

      let factory = address!("1F98431c8aD98523631AE4a59f267346ea31F984");

      let pairs = get_v3_pools(client, weth, usdc, factory).await.unwrap();

      assert_eq!(pairs.len(), 4);

      println!("=== V3 Pairs Test ===");
      for pair in pairs {
         println!("Pair: {:?}, Fee: {}", pair.addr, pair.fee);
      }
   }

   #[tokio::test]
   async fn test_v2_pool_reserves() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

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
   async fn test_v3_state() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let pool = address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640");
      let base_token = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
      let pool2 = V3Pool2 { pool, base_token };

      let data = get_v3_state(client.clone(), None, vec![pool2])
         .await
         .unwrap();

      assert_eq!(data.len(), 1);

      println!("=== V3 Pool Data Test ===");
      for pool in data {
         println!("Pool Data: {:?}", pool);
      }
   }

   /*
       #[tokio::test]
       async fn test_v3_state_limit() {
           use std::str::FromStr;
           let url = Url::parse("https://eth.merkle.io").unwrap();
           let client = ProviderBuilder::new().on_http(url);

           let pools = vec![
               Address::from_str("0x937e2afab37237a83f47b9ec0ecb18a5aaef353a").unwrap(),
               Address::from_str("0x99585040df4bbce1883a462bf5684774676fbeac").unwrap(),
               Address::from_str("0xe9ac65ca67cf1cd5124d030bbbcb2f9f2b1cf72e").unwrap(),
               Address::from_str("0xef3c16317f9907ef9181c99247cd6d9458550a4d").unwrap(),
               Address::from_str("0x9178ea3eb764c0e8728ae5ab0823101a975a0a12").unwrap(),
               Address::from_str("0x2a4d547ea2c35d03a501a8bb5d12d81d0a222dc5").unwrap(),
               Address::from_str("0x6b4265e16e7cf4bc160062de8219438a3d08518c").unwrap(),
               Address::from_str("0xe6e14be906c1f1b438da2010b38beca14b387231").unwrap(),
               Address::from_str("0x177622e79acece98c39f6e12fa78ac7fc8a8bf62").unwrap(),
               Address::from_str("0xa6118c413a5816e25b08c02c7b43b94ddd88bf35").unwrap(),
           ];

           let data = get_v3_state(client, None, pools).await.unwrap();

           assert_eq!(data.len(), 10);
   }
           */
}
