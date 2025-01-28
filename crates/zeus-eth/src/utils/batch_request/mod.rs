use alloy_sol_types::{sol, SolValue};
use alloy_primitives::Address;
use alloy_rpc_types::BlockId;

use alloy_contract::private::Network;
use alloy_provider::Provider;
use alloy_transport::Transport;
use crate::defi::currency::erc20::ERC20Token;
use anyhow::anyhow;

sol! {
    #[sol(rpc)]
    IGetErc20Balance,
    "src/utils/batch_request/abi/GetErc20Balance.json",
}

sol! {
    #[sol(rpc)]
    IGetERC20,
    "src/utils/batch_request/abi/GetERC20.json",
}

sol! {
    #[sol(rpc)]
    IGetV2PoolsReserves,
    "src/utils/batch_request/abi/GetV2PoolsReserves.json",
}

sol! {
    #[sol(rpc)]
    IGetV3Pools,
    "src/utils/batch_request/abi/GetV3Pools.json",
}

sol! {
    #[sol(rpc)]
    IGetUniswapV3PoolData,
    "src/utils/batch_request/abi/GetUniswapV3PoolData.json",
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
        uint24 fee;
        address pool;
    }
}


sol! {
    #[derive(Debug)]
    struct PoolData {
        address pool;
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
pub async fn get_erc20_balance<T, P, N>(
    client: P,
    block: Option<BlockId>,
    owner: Address,
    tokens: Vec<Address>
)
    -> Result<Vec<TokenBalance>, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let block = block.unwrap_or(BlockId::latest());
    let deployer = IGetErc20Balance::deploy_builder(client, tokens, owner).block(block);
    let res = deployer.call_raw().await?;

    let data = <Vec<TokenBalance> as SolValue>::abi_decode(&res, false).map_err(|e| anyhow!("Failed to decode token balances: {:?}", e))?;
    Ok(data)
}

/// Get the ERC20 token info
pub async fn get_erc20_info<T, P, N>(
    client: P,
    token: Address,
    chain_id: u64
)
    -> Result<ERC20Token, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let deployer = IGetERC20::deploy_builder(client, token);
    let res = deployer.call_raw().await?;

    let data = <ERC20Info as SolValue>::abi_decode(&res, false).map_err(|e| anyhow!("Failed to decode token info: {:?}", e))?;

    Ok(ERC20Token {
        address: token,
        chain_id,
        symbol: data.symbol,
        name: data.name,
        decimals: data.decimals,
        total_supply: data.totalSupply,
        icon: None
    })
}




/// Retrieve all V3 pools for tokenA and tokenB based on the fee tiers (if they exist)
/// 
/// For any possible pool that does not exist the values will be 0
/// 
/// If no pools exists it will still return a vector with zero values
pub async fn get_v3_pools<T, P, N>(
    client: P,
    token_a: Address,
    token_b: Address,
    factory: Address
)
    -> Result<Vec<V3Pool>, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let deployer = IGetV3Pools::deploy_builder(client, factory, token_a, token_b);
    let res = deployer.call_raw().await?;

    let data = <Vec<V3Pool> as SolValue>::abi_decode(&res, false).map_err(|e| anyhow!("Failed to decode V3 pools: {:?}", e))?;
    
    Ok(data)
}

/// Get the reserves for the given v2 pools, if block is None, then the latest block is used
pub async fn get_v2_pool_reserves<T, P, N>(
    client: P,
    block: Option<BlockId>,
    pools: Vec<Address>
)
    -> Result<Vec<V2PoolReserves>, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let block = block.unwrap_or(BlockId::latest());
    let deployer = IGetV2PoolsReserves::deploy_builder(client, pools).block(block);
    let res = deployer.call_raw().await?;

    let data = <Vec<V2PoolReserves> as SolValue>::abi_decode(&res, false).map_err(|e| anyhow!("Failed to decode V2 pool reserves: {:?}", e))?;
    
    Ok(data)
}

/// Retrieve the pool data for the given pools, if block is None, then the latest block is used
pub async fn get_v3_pool_data<T, P, N>(
    client: P,
    block: Option<BlockId>,
    pools: Vec<Address>
)
    -> Result<Vec<PoolData>, anyhow::Error>
    where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
{
    let block = block.unwrap_or(BlockId::latest());
    let deployer = IGetUniswapV3PoolData::deploy_builder(client, pools).block(block);
    let res = deployer.call_raw().await?;

    let data = <Vec<PoolData> as SolValue>::abi_decode(&res, false).map_err(|e| anyhow!("Failed to decode V3 pool data: {:?}", e))?;
    
    Ok(data)
}

#[cfg(test)]
mod tests {
    use crate::prelude::{ ERC20Token, usdc, weth };
    use alloy_primitives::address;
    use alloy_provider::{ ProviderBuilder, WsConnect };
    use super::*;
    use alloy_signer_local::PrivateKeySigner;

    #[tokio::test]
    async fn test_erc20_balance() {
        let url = "wss://eth.merkle.io";
        let ws_connect = WsConnect::new(url);
        let client = ProviderBuilder::new().on_ws(ws_connect).await.unwrap();

        let weth = weth(1).unwrap();
        let usdc = usdc(1).unwrap();

        let owner = PrivateKeySigner::random();

        let tokens = vec![weth, usdc];

        let balances = get_erc20_balance(client, None, owner.address(), tokens).await.unwrap();

        assert_eq!(balances.len(), 2);
    }

    #[tokio::test]
    async fn test_erc20_info() {
        let url = "wss://eth.merkle.io";
        let ws_connect = WsConnect::new(url);
        let client = ProviderBuilder::new().on_ws(ws_connect).await.unwrap();

        let weth = ERC20Token::weth();
        let usdc = ERC20Token::usdc();

        let weth_info = get_erc20_info(client.clone(), weth.address, weth.chain_id).await.unwrap();
        let usdc_info = get_erc20_info(client.clone(), usdc.address, usdc.chain_id).await.unwrap();

        assert_eq!(weth_info.symbol, weth.symbol);
        assert_eq!(usdc_info.symbol, usdc.symbol);

        assert_eq!(weth_info.name, weth.name);
        assert_eq!(usdc_info.name, usdc.name);

        assert_eq!(weth_info.decimals, weth.decimals);
        assert_eq!(usdc_info.decimals, usdc.decimals);
    }

    #[tokio::test]
    async fn test_v3_pools() {
        let url = "wss://eth.merkle.io";
        let ws_connect = WsConnect::new(url);
        let client = ProviderBuilder::new().on_ws(ws_connect).await.unwrap();

        let weth = weth(1).unwrap();
        let usdc = usdc(1).unwrap();

        let factory = address!("1F98431c8aD98523631AE4a59f267346ea31F984");

        let pairs = get_v3_pools(client, weth, usdc, factory).await.unwrap();

        assert_eq!(pairs.len(), 4);

        println!("=== V3 Pairs Test ===");
        for pair in pairs {
            println!("Pair: {:?}, Fee: {}", pair.pool, pair.fee);
        }
    }

    #[tokio::test]
    async fn test_v2_pool_reserves() {
        let url = "wss://eth.merkle.io";
        let ws_connect = WsConnect::new(url);
        let client = ProviderBuilder::new().on_ws(ws_connect).await.unwrap();

        let pool = address!("0d4a11d5EEaaC28EC3F61d100daF4d40471f1852");

        let reserves = get_v2_pool_reserves(client, None, vec![pool]).await.unwrap();

        assert_eq!(reserves.len(), 1);

        println!("=== V2 Pool Reserves Test ===");
        for reserve in reserves {
            println!("Pool: {:?}, Reserves: {}, {}", reserve.pool, reserve.reserve0, reserve.reserve1);
        }
    }

    #[tokio::test]
    async fn test_v3_pool_data() {
        let url = "wss://eth.merkle.io";
        let ws_connect = WsConnect::new(url);
        let client = ProviderBuilder::new().on_ws(ws_connect).await.unwrap();

        let pool = address!("88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640");

        let data = get_v3_pool_data(client, None, vec![pool]).await.unwrap();

        assert_eq!(data.len(), 1);

        println!("=== V3 Pool Data Test ===");
        for pool in data {
            println!("Pool Data: {:?}", pool);
        }
    }
}
