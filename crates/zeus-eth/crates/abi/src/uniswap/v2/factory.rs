use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{SolCall, sol};

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

pub async fn fee_to<P, N>(client: P, factory: Address) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let factory = IUniswapV2Factory::new(factory, client);
   let fee_to = factory.feeTo().call().await?;
   Ok(fee_to)
}

pub async fn fee_to_setter<P, N>(client: P, factory: Address) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let factory = IUniswapV2Factory::new(factory, client);
   let fee_to_setter = factory.feeToSetter().call().await?;
   Ok(fee_to_setter)
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

pub async fn all_pairs<P, N>(client: P, factory: Address, index: U256) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let factory = IUniswapV2Factory::new(factory, client);
   let pair = factory.allPairs(index).call().await?;
   Ok(pair)
}

pub async fn all_pairs_length<P, N>(client: P, factory: Address) -> Result<U256, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let factory = IUniswapV2Factory::new(factory, client);
   let length = factory.allPairsLength().call().await?;
   Ok(length)
}

pub fn encode_create_pair(token0: Address, token1: Address) -> Bytes {
   let abi = IUniswapV2Factory::createPairCall {
      tokenA: token0,
      tokenB: token1,
   };
   Bytes::from(abi.abi_encode())
}

pub fn decode_create_pair(data: &Bytes) -> Result<Address, anyhow::Error> {
   let abi = IUniswapV2Factory::createPairCall::abi_decode_returns(data)?;
   Ok(abi)
}
