use alloy_primitives::{
   Address, Bytes, U256,
};
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


pub fn encode_z_swap(commands: Bytes, inputs: Vec<Bytes>, currency_out: Address, amount_min: U256) -> Bytes {
   let data = ZeusDelegate::zSwapCall {
      params: ZeusDelegate::ZParams {
         commands,
         inputs,
         currencyOut: currency_out,
         amountMin: amount_min,
      }
   }
   .abi_encode();
   data.into()
}