use alloy_primitives::{Address, Signature, Bytes, U256, aliases::{U48, U160}};
use alloy_sol_types::{SolCall, SolValue, sol};
use super::permit::Permit2::{PermitSingle, PermitDetails};

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
    contract ZeusRouter {

    function zSwap(ZParams calldata params) public payable;

   address public immutable WETH;
   address public immutable PERMIT2;
   address public immutable V4_POOL_MANAGER;
   address public immutable UNISWAP_V3_FACTORY;
   address public immutable PANCAKE_SWAP_V3_FACTORY;


    struct DeployParams {
        address weth;
        address permit2;
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

    struct Permit2Permit {
        PermitSingle permitSingle;
        bytes signature;
    }

    struct V2V3SwapParams {
        uint256 amountIn;
        address tokenIn;
        address tokenOut;
        address pool;
        uint poolVariant;
        address recipient;
        uint24 fee;
        bool permit2;
    }

    struct V4SwapParams {
        address currencyIn;
        address currencyOut;
        uint256 amountIn;
        uint24 fee;
        int24 tickSpacing;
        bool zeroForOne;
        address hooks;
        bytes hookData;
        address recipient;
        bool permit2;
    }

    struct WrapETH {
    address recipient;
    uint256 amount;
    }

    struct WrapAllETH {
    address recipient;
    }

    struct UnwrapWETH {
    address recipient;
    }

    struct Sweep {
    address currency;
    address recipient;
    }

    }

}

sol! {
    #[sol(rpc)]
    contract ZeusStateViewV2 {

        function getETHBalance(address[] memory owners) external view returns (ETHBalance[] memory) {}
        function getERC20Balance(address[] memory tokens, address owner) external view returns (ERC20Balance[] memory) {}
        function getERC20Info(address token) external view returns (ERC20Info memory) {}
        function getERC20InfoBatch(address[] memory tokens) external view returns (ERC20Info[] memory) {}
        function getV3Pools(address factory, address tokenA, address tokenB) external view returns (V3Pool[] memory) {}
        function validateV4Pools(address stateView, bytes32[] memory pools) external view returns (bytes32[] memory) {}
        function getV2Reserves(address[] memory pools) external view returns (V2PoolReserves[] memory) {}
        function getV3PoolState(V3Pool[] memory pools) external view returns (V3PoolData[] memory) {}
        function getV4PoolState(V4Pool[] memory pools, address stateView) external view returns (V4PoolData[] memory) {}
        function getPoolsState(
        address[] memory v2pools,
        V3Pool[] memory v3pools,
        V4Pool[] memory v4pools,
        address stateView
    ) external view returns (PoolsState memory) {}
        function getPools(
        address v2factory,
        address v3factory,
        address stateView,
        bytes32[] memory v4pools,
        address[] memory baseTokens,
        address quoteToken
    ) external view returns (Pools memory) {}

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
    #[derive(Debug)]
    struct V3Pool {
        address addr;
        address tokenA;
        address tokenB;
        uint24 fee;
    }

    /// Response of the `getV2Pools` function
    #[derive(Debug)]
    struct V2Pool {
        address addr;
        address tokenA;
        address tokenB;
    }

    /// Response of the `getV2Reserves` function
    #[derive(Default, Debug)]
    struct V2PoolReserves {
        address pool;
        uint112 reserve0;
        uint112 reserve1;
        uint32 blockTimestampLast;
    }

    /// Response of the `getV3PoolState` function
    #[derive(Default, Debug)]
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
    #[derive(Default, Debug)]
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

    /// Response of the `getPoolsState` function
    #[derive(Default, Debug)]
    struct PoolsState {
        V2PoolReserves[] v2Reserves;
        V3PoolData[] v3PoolsData;
        V4PoolData[] v4PoolsData;
    }

    /// Response of the `getPools` function
    #[derive(Debug)]
    struct Pools {
        V2Pool[] v2Pools;
        V3Pool[] v3Pools;
        bytes32[] v4Pools;
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

pub fn encode_permit2_permit(
   token: Address,
   amount: U256,
   expiration: U256,
   nonce: U48,
   spender: Address,
   sig_deadline: U256,
   signature: Signature,
) -> Bytes {
   let amount = U160::from(amount);
   let expiration = U48::from(expiration);

   let permit_details = PermitDetails {
      token,
      amount,
      expiration,
      nonce,
   };

   let permit_single = PermitSingle {
      details: permit_details,
      spender,
      sigDeadline: sig_deadline,
   };

   let sig_bytes = Bytes::from(signature.as_bytes());
   let encoded_args = (permit_single, sig_bytes).abi_encode();

   encoded_args.into()
}