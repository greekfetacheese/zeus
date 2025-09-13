use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{SolCall, sol};

sol! {
    contract ZeusDelegate {

    function zSwap(ZParams calldata params) public payable;

   address public immutable WETH;
   address public immutable V4_POOL_MANAGER;
   address public immutable UNISWAP_V3_FACTORY;
   address public immutable PANCAKE_SWAP_V3_FACTORY;


    struct DeployParams {
    address weth;
    address v4PoolManager;
    address uniswapV3Factory;
    address pancakeSwapV3Factory;
}


    struct ZParams {
    bytes commands;
    bytes[] inputs;
    address currencyOut;
    uint256 amountMin;
    uint256 deadline;
}

    struct V2V3SwapParams {
        uint256 amountIn;
        address tokenIn;
        address tokenOut;
        address pool;
        bytes1 poolVariant;
        uint24 fee;
    }

    struct V4SwapArgs {
        address currencyIn;
        address currencyOut;
        uint256 amountIn;
        uint24 fee;
        int24 tickSpacing;
        bool zeroForOne;
        address hooks;
        bytes hookData;
        address recipient;
    }

    struct WrapETH {
        uint256 amountMin;
    }

    struct WrapETHNoCheck {
        uint256 amount;
    }

    struct UnwrapWETH {
        uint256 amountMin;
    }
    }

}

sol! {
    #[sol(rpc)]
    contract ZeusStateView {

        function getETHBalance(address[] memory owners) external view returns (ETHBalance[] memory) {}
        function getERC20Balance(address[] memory tokens, address owner) external view returns (ERC20Balance[] memory) {}
        function getERC20Info(address token) external view returns (ERC20Info memory) {}
        function getERC20InfoBatch(address[] memory tokens) external view returns (ERC20Info[] memory) {}
        function getV3Pools(address factory, address tokenA, address tokenB) external view returns (V3Pool[] memory) {}
        function validateV4Pools(address stateView, bytes32[] memory pools) external view returns (bytes32[] memory) {}
        function getV2Reserves(address[] memory pools) external view returns (V2PoolReserves[] memory) {}
        function getV3PoolState(V3Pool[] memory pools) external view returns (V3PoolData[] memory) {}
        function getV4PoolState(V4Pool[] memory pools, address stateView) external view returns (V4PoolData[] memory) {}


    /// Response of the `getETHBalance` function
    struct ETHBalance {
        address owner;
        uint256 balance;
    }

    /// Response of the `getERC20Balance` function
    struct ERC20Balance {
        address token;
        uint256 balance;
    }

    /// Response of the `getERC20Info` function
    struct ERC20Info {
        address addr;
        string symbol;
        string name;
        uint256 totalSupply;
        uint8 decimals;
    }

    /// Response of the `getV3Pools` function
    struct V3Pool {
        address addr;
        address tokenA;
        address tokenB;
        uint24 fee;
    }

    /// Response of the `getV2Reserves` function
    #[derive(Debug)]
    struct V2PoolReserves {
        address pool;
        uint112 reserve0;
        uint112 reserve1;
        uint32 blockTimestampLast;
    }

    /// Response of the `getV3PoolState` function
    #[derive(Debug)]
    struct V3PoolData {
        address pool;
        uint256 tokenABalance;
        uint256 tokenBBalance;
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

    /// Argument for the `getV4PoolState` function
    struct V4Pool {
        bytes32 pool;
        int24 tickSpacing;
    }

    /// Response of the `getV4PoolState` function
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
    }
}

pub fn encode_z_swap(
   commands: Bytes,
   inputs: Vec<Bytes>,
   currency_out: Address,
   amount_min: U256,
   deadline: U256,
) -> Bytes {
   let data = ZeusDelegate::zSwapCall {
      params: ZeusDelegate::ZParams {
         commands,
         inputs,
         currencyOut: currency_out,
         amountMin: amount_min,
         deadline,
      },
   }
   .abi_encode();
   data.into()
}