use alloy_contract::private::Network;
use alloy_primitives::{Address, Bytes, U256};
use alloy_provider::Provider;
use alloy_rpc_types::BlockId;
use alloy_sol_types::{SolCall, sol};
use alloy_transport::Transport;


sol! {

    #[sol(rpc)]
    contract IUniswapV2Pair {

        // * EVENTS *

        event Approval(address indexed owner, address indexed spender, uint value);
        event Transfer(address indexed from, address indexed to, uint value);
        event Mint(address indexed sender, uint amount0, uint amount1);
        event Burn(address indexed sender, uint amount0, uint amount1, address indexed to);
        event Swap(
            address indexed sender,
            uint amount0In,
            uint amount1In,
            uint amount0Out,
            uint amount1Out,
            address indexed to
        );
        event Sync(uint112 reserve0, uint112 reserve1);

        // * VIEW FUNCTIONS *

        function factory() external view returns (address);
        function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
        function kLast() external view returns (uint256);
        function name() external view returns (string memory);
        function price0CumulativeLast() external view returns (uint256);
        function price1CumulativeLast() external view returns (uint256);
        function token0() external view returns (address);
        function token1() external view returns (address);

        // * WRITE FUNCTIONS *

        function approve(address spender, uint value) external returns (bool);
        function burn(address to) external;
        function initialize(address token0, address token1) external;
        function mint(address to) external;
        function permit(
            address owner,
            address spender,
            uint value,
            uint deadline,
            uint8 v,
            bytes32 r,
            bytes32 s
        ) external;
        function skim(address to) external;
        function swap(
            uint amount0Out,
            uint amount1Out,
            address to,
            bytes calldata data
        ) external;
        function sync() external;

    }
}

/// Return the factory address that created this pair
pub async fn factory<T, P, N>(pair_address: Address, client: P) -> Result<Address, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let contract = IUniswapV2Pair::new(pair_address, client);
    let factory = contract.factory().call().await?;
    Ok(factory._0)
}

/// Return the reserves of this pair (reserve0, reserve1, blockTimestampLast)
///
/// # Arguments
///
/// * `client` - The provided client
///
/// * `block_id` - The block id to query the reserves
/// If None, the latest block will be used
pub async fn get_reserves<T, P, N>(
    pair_address: Address,
    client: P,
    block_id: Option<BlockId>,
) -> Result<(U256, U256, u32), anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let block = block_id.unwrap_or(BlockId::latest());
    let contract = IUniswapV2Pair::new(pair_address, client);
    let reserves = contract.getReserves().call().block(block).await?;
    let reserve0 = U256::from(reserves.reserve0);
    let reserve1 = U256::from(reserves.reserve1);
    Ok((
        reserve0,
        reserve1,
        reserves.blockTimestampLast,
    ))
}

/// Return the last k value of this pair
pub async fn k_last<T, P, N>(
    pair_address: Address,
    client: P,
    block_id: Option<BlockId>,
) -> Result<U256, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let block = block_id.unwrap_or(BlockId::latest());
    let contract = IUniswapV2Pair::new(pair_address, client);
    let k_last = contract.kLast().call().block(block).await?;
    Ok(k_last._0)
}

/// Return the price0CumulativeLast of this pair
pub async fn price0_cumulative_last<T, P, N>(
    pair_address: Address,
    client: P,
    block_id: Option<BlockId>,
) -> Result<U256, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let block = block_id.unwrap_or(BlockId::latest());
    let contract = IUniswapV2Pair::new(pair_address, client);
    let price0_cumulative_last = contract
        .price0CumulativeLast()
        .call()
        .block(block)
        .await?;
    Ok(price0_cumulative_last._0)
}

/// Return the price1CumulativeLast of this pair
pub async fn price1_cumulative_last<T, P, N>(
    pair_address: Address,
    client: P,
    block_id: Option<BlockId>,
) -> Result<U256, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let block = block_id.unwrap_or(BlockId::latest());
    let contract = IUniswapV2Pair::new(pair_address, client);
    let price1_cumulative_last = contract
        .price1CumulativeLast()
        .call()
        .block(block)
        .await?;
    Ok(price1_cumulative_last._0)
}

/// Return the address of token0
pub async fn token0<T, P, N>(pair_address: Address, client: P) -> Result<Address, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let contract = IUniswapV2Pair::new(pair_address, client);
    let token0 = contract.token0().call().await?;
    Ok(token0._0)
}

/// Return the address of token1
pub async fn token1<T, P, N>(pair_address: Address, client: P) -> Result<Address, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let contract = IUniswapV2Pair::new(pair_address, client);
    let token1 = contract.token1().call().await?;
    Ok(token1._0)
}

/// Make a burn call to this pair
pub async fn burn<T, P, N>(
    pair_address: Address,
    to: Address,
    client: P,
) -> Result<(), anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let contract = IUniswapV2Pair::new(pair_address, client);
    contract.burn(to).call().await?;
    Ok(())
}

// * ABI Encode the functions

/// Encode the function with signature `factory()` and selector `0xc45a0155`
pub fn encode_factory() -> Bytes {
    let abi = IUniswapV2Pair::factoryCall {};
    Bytes::from(abi.abi_encode())
}

/// Encode the function with signature `getReserves()` and selector `0x0902f1ac`
pub fn encode_get_reserves() -> Bytes {
    let abi = IUniswapV2Pair::getReservesCall {};
    Bytes::from(abi.abi_encode())
}

/// Encode the function with signature `kLast()` and selector `0x7464fc3d`
pub fn encode_k_last() -> Bytes {
    let abi = IUniswapV2Pair::kLastCall {};
    Bytes::from(abi.abi_encode())
}

/// Encode the function with signature `price0CumulativeLast()` and selector `0x5909c0d5`
pub fn encode_price0_cumulative_last() -> Bytes {
    let abi =
        IUniswapV2Pair::price0CumulativeLastCall {};
    Bytes::from(abi.abi_encode())
}

/// Encode the function with signature `price1CumulativeLast()` and selector `0x5a3d5493`
pub fn encode_price1_cumulative_last() -> Bytes {
    let abi =
        IUniswapV2Pair::price1CumulativeLastCall {};
    Bytes::from(abi.abi_encode())
}

/// Encode the function with signature `token0()` and selector `0x0dfe1681`
pub fn encode_token0() -> Bytes {
    let abi = IUniswapV2Pair::token0Call {};
    Bytes::from(abi.abi_encode())
}

/// Encode the function with signature `token1()` and selector `0xd21220a7`
pub fn encode_token1() -> Bytes {
    let abi = IUniswapV2Pair::token1Call {};
    Bytes::from(abi.abi_encode())
}