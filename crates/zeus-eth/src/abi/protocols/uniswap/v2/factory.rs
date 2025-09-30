use alloy_primitives::Address;
use alloy_sol_types::sol;

use alloy_contract::private::{Network, Provider};

sol! {
    #[sol(rpc)]
    contract IUniswapV2Factory {
        event PairCreated(address indexed token0, address indexed token1, address pair, uint);

        function feeTo() external view returns (address);
        function feeToSetter() external view returns (address);
        function getPair(address tokenA, address tokenB) external view returns (address pair);
        function allPairs(uint256 index) external view returns (address pair);
        function allPairsLength() external view returns (uint256 length);
        function createPair(address tokenA, address tokenB) external returns (address pair);
        function setFeeTo(address) external;
        function setFeeToSetter(address) external;
    }
}

pub async fn get_pair<P, N>(
   client: P,
   factory: Address,
   token0: Address,
   token1: Address,
) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let factory = IUniswapV2Factory::new(factory, client);
   let pair = factory.getPair(token0, token1).call().await?;
   Ok(pair)
}
