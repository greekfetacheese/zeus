use alloy_primitives::{Address, Uint};
use alloy_sol_types::sol;

use alloy_contract::private::{Network, Provider};

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

pub async fn get_pool<P, N>(
   client: P,
   factory: Address,
   token0: Address,
   token1: Address,
   fee: u32,
) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let factory = IUniswapV3Factory::new(factory, client);
   let pool = factory
      .getPool(token0, token1, Uint::from(fee))
      .call()
      .await?;
   Ok(pool)
}