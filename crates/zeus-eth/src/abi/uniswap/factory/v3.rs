use alloy_sol_types::{sol, SolCall};

use alloy_contract::private::Network;
use alloy_primitives::{Address, Bytes, Uint};
use alloy_provider::Provider;
use alloy_transport::Transport;


sol! {
#[sol(rpc)]
contract IUniswapV3Factory {
    event OwnerChanged(address indexed oldOwner, address indexed newOwner);
    event FeeAmountEnabled(uint24 indexed fee, int24 indexed tickSpacing);
    event PoolCreated(
        address indexed token0,
        address indexed token1,
        uint24 indexed fee,
        int24 tickSpacing,
        address pool
    );

    function owner() external view returns (address);
    function feeAmountTickSpacing(uint24 fee) external view returns (int24);
    function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address pool);
    function setOwner(address _owner) external;
    function enableFeeAmount(uint24 fee, int24 tickSpacing) external;

    function createPool(
        address tokenA,
        address tokenB,
        uint24 fee
    ) external returns (address pool);
}
}


pub async fn owner<T, P, N>(
    client: P,
    factory: Address,
) -> Result<Address, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let factory = IUniswapV3Factory::new(factory, client);
    let owner = factory.owner().call().await?;
    Ok(owner._0)
}

pub async fn fee_amount_tick_spacing<T, P, N>(
    client: P,
    factory: Address,
    fee: u32
) -> Result<i32, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let factory = IUniswapV3Factory::new(factory, client);
    let tick_spacing = factory.feeAmountTickSpacing(Uint::from(fee)).call().await?;
    let tick_spacing = tick_spacing._0.bits();
    Ok(tick_spacing as i32)
}

pub async fn get_pool<T, P, N>(
    client: P,
    factory: Address,
    token0: Address,
    token1: Address,
    fee: u32
) -> Result<Address, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let factory = IUniswapV3Factory::new(factory, client);
    let pool = factory.getPool(token0, token1, Uint::from(fee)).call().await?;
    Ok(pool.pool)
}


pub fn encode_create_pool(token0: Address, token1: Address, fee: u32) -> Bytes {
    let abi = IUniswapV3Factory::createPoolCall {
        tokenA: token0,
        tokenB: token1,
        fee: Uint::from(fee),
    };
    Bytes::from(abi.abi_encode())
}

pub fn decode_create_pool(data: &Bytes) -> Result<Address, anyhow::Error> {
    let abi = IUniswapV3Factory::createPoolCall::abi_decode_returns(data, true)?;
    Ok(abi.pool)
}