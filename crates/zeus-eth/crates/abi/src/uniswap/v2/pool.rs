use alloy_primitives::{Address, Bytes, LogData, U256};
use alloy_rpc_types::BlockId;
use alloy_sol_types::{SolCall, SolEvent, sol};

use IUniswapV2Pair::Swap;
use alloy_contract::private::{Network, Provider};

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

pub fn swap_signature() -> &'static str {
   IUniswapV2Pair::swapCall::SIGNATURE
}

pub fn swap_selector() -> [u8; 4] {
   IUniswapV2Pair::swapCall::SELECTOR
}

/// Return the factory address that created this pair
pub async fn factory<P, N>(pair_address: Address, client: P) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IUniswapV2Pair::new(pair_address, client);
   let factory = contract.factory().call().await?;
   Ok(factory)
}

/// * `block_id` - The block id to query the reserves
/// If None, the latest block will be used
pub async fn get_reserves<P, N>(
   pair_address: Address,
   client: P,
   block_id: Option<BlockId>,
) -> Result<(U256, U256, u32), anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block = block_id.unwrap_or(BlockId::latest());
   let contract = IUniswapV2Pair::new(pair_address, client);
   let reserves = contract.getReserves().call().block(block).await?;
   let reserve0 = U256::from(reserves.reserve0);
   let reserve1 = U256::from(reserves.reserve1);
   Ok((reserve0, reserve1, reserves.blockTimestampLast))
}


/// Return the address of token0
pub async fn token0<P, N>(pair_address: Address, client: P) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IUniswapV2Pair::new(pair_address, client);
   let token0 = contract.token0().call().await?;
   Ok(token0)
}

/// Return the address of token1
pub async fn token1<P, N>(pair_address: Address, client: P) -> Result<Address, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IUniswapV2Pair::new(pair_address, client);
   let token1 = contract.token1().call().await?;
   Ok(token1)
}


pub fn decode_swap_log(log: &LogData) -> Result<Swap, anyhow::Error> {
   let b = IUniswapV2Pair::Swap::decode_raw_log(log.topics(), &log.data)?;
   Ok(b)
}

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