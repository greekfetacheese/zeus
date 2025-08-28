use alloy_primitives::{
   Address, Bytes, Signature, U256,
   aliases::{U48, U160},
};
use alloy_sol_types::{SolCall, SolValue, sol};

use crate::abi::permit::Permit2;

sol! {
    contract ZeusRouter {
       function execute(bytes calldata commands, bytes[] calldata inputs) public payable;

       address public immutable WETH;
       address public immutable PERMIT2;
       address public immutable V4_POOL_MANAGER;
       address public immutable UNISWAP_V3_FACTORY;
       address public immutable PANCAKE_SWAP_V3_FACTORY;
    }

        struct DeployParams {
        address weth;
        address permit2;
        address v4PoolManager;
        address uniswapV3Factory;
        address pancakeSwapV3Factory;
    }

        struct Permit2Permit {
        Permit2.PermitSingle permitSingle;
        bytes signature;
    }

    /// Parameters for a V2/V3 swap
    /// 
    /// `amountIn` The amount of tokenIn to swap
    /// 
    /// `tokenIn` The input token
    /// 
    /// `tokenOut` The output token
    /// 
    /// `pool` The pool to swap on
    /// 
    /// `poolVariant` 0 for V2, 1 for V3
    /// 
    /// `recipient` The recipient of the tokenOut
    /// 
    /// `fee` The pool fee in hundredths of bips (eg. 3000 for 0.3%)
    /// 
    /// `permit2` Whether the funds should come from permit or are already in the router
    struct V2V3SwapParams {
        uint256 amountIn;
        uint256 amountOutMin;
        address tokenIn;
        address tokenOut;
        address pool;
        uint poolVariant;
        address recipient;
        uint24 fee;
        bool permit2;
    }

    /// Parameters for a V4 swap
    /// 
    /// `currencyIn` The input currency (address(0) for ETH)
    /// 
    /// `currencyOut` The output currency (address(0) for ETH)
    /// 
    /// `amountIn` The amount of currencyIn to swap
    /// 
    /// `amountOutMin` The minimum amount of currencyOut to receive
    /// 
    /// `fee` The pool fee in hundredths of bips (eg. 3000 for 0.3%)
    /// 
    /// `tickSpacing` The tick spacing of the pool
    /// 
    /// `zeroForOne` Whether the swap is from currencyIn to currencyOut
    /// 
    /// `hooks` The hooks to use for the swap
    /// 
    /// `hookData` The data to pass to the hooks
    /// 
    /// `recipient` The recipient of the currencyOut
    /// 
    /// `permit2` Whether the funds should come from permit or are already in the router
    struct V4SwapParams {
        address currencyIn;
        address currencyOut;
        uint256 amountIn;
        uint256 amountOutMin;
        uint24 fee;
        int24 tickSpacing;
        bool zeroForOne;
        address hooks;
        bytes hookData;
        address recipient;
        bool permit2;
    }

        /// `recipient` The recipient of the wrapped ETH
        /// 
        /// `amount` The amount of ETH to wrap
        struct WrapETH {
        address recipient;
        uint256 amount;
    }

        /// `recipient` The recipient of the unwrapped ETH
        /// 
        /// `amountMin` The minimum amount of ETH to unwrap
        struct UnwrapWETH {
        address recipient;
        uint256 amountMin;
    }

        /// `currency` The currency to sweep, Use address(0) for ETH
        /// 
        /// `recipient` The recipient of the tokens
        /// 
        /// `amountMin` The minimum amount of tokens to sweep
        struct Sweep {
        address currency;
        address recipient;
        uint256 amountMin;
    }
}

pub fn encode_execute(commands: Bytes, inputs: Vec<Bytes>) -> Bytes {
   let data = ZeusRouter::executeCall { commands, inputs }.abi_encode();
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

   let permit_details = Permit2::PermitDetails {
      token,
      amount,
      expiration,
      nonce,
   };

   let permit_single = Permit2::PermitSingle {
      details: permit_details,
      spender,
      sigDeadline: sig_deadline,
   };

   let sig_bytes = Bytes::from(signature.as_bytes());
   let input = Permit2Permit {
      permitSingle: permit_single,
      signature: sig_bytes,
   };

   input.abi_encode().into()
}
