use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{SolCall, SolValue, sol};

pub use IV4Router::{PoolKey, PathKey, ExactInputParams, ExactInputSingleParams, ExactOutputParams, ExactOutputSingleParams};

sol! {

    contract UniversalRouter {
        function execute(bytes calldata commands, bytes[] calldata inputs) public payable;

        function execute(bytes calldata commands, bytes[] calldata inputs, uint256 deadline)
        external
        payable;
    }

   interface IHooks {}

    interface IV4Router {

         type Currency is address;
         

         #[derive(Debug)]
         struct Sweep {
            address token;
            address recipient;
            uint256 amountMin;
         }

        #[derive(Debug, Default, PartialEq, Eq)]
        /// @notice Parameters for a single-hop exact-input swap
        struct ExactInputSingleParams {
            PoolKey poolKey;
            bool zeroForOne;
            uint128 amountIn;
            uint128 amountOutMinimum;
            bytes hookData;
        }

        #[derive(Debug, Default, PartialEq, Eq)]
        /// @notice Parameters for a multi-hop exact-input swap
        struct ExactInputParams {
            Currency currencyIn;
            PathKey[] path;
            uint128 amountIn;
            uint128 amountOutMinimum;
        }

        #[derive(Debug, Default, PartialEq, Eq)]
        /// @notice Parameters for a single-hop exact-output swap
        struct ExactOutputSingleParams {
            PoolKey poolKey;
            bool zeroForOne;
            uint128 amountOut;
            uint128 amountInMaximum;
            bytes hookData;
        }

        #[derive(Debug, Default, PartialEq, Eq)]
        /// @notice Parameters for a multi-hop exact-output swap
        struct ExactOutputParams {
            address currencyOut;
            PathKey[] path;
            uint128 amountOut;
            uint128 amountInMaximum;
        }

        #[derive(Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct PoolKey {
        address currency0;
        address currency1;
        uint24 fee;
        int24 tickSpacing;
        address hooks;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct PathKey {
        address intermediateCurrency;
        uint24 fee;
        int24 tickSpacing;
        IHooks hooks;
        bytes hookData;
    }


    #[derive(Debug, Default, PartialEq, Eq)]
    struct IncreaseLiquidityParams {
        uint256 tokenId;
        uint256 liquidity;
        uint128 amount0Max;
        uint128 amount1Max;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct DecreaseLiquidityParams {
        uint256 tokenId;
        uint256 liquidity;
        uint128 amount0Min;
        uint128 amount1Min;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct MintPositionParams {
        PoolKey poolKey;
        int24 tickLower;
        int24 tickUpper;
        uint256 liquidity;
        uint128 amount0Max;
        uint128 amount1Max;
        address owner;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct BurnPositionParams {
        uint256 tokenId;
        uint128 amount0Min;
        uint128 amount1Min;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SwapExactInSingleParams {
        PoolKey poolKey;
        bool zeroForOne;
        uint128 amountIn;
        uint128 amountOutMinimum;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SwapExactInParams {
        address currencyIn;
        PathKey[] path;
        uint128 amountIn;
        uint128 amountOutMinimum;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SwapExactOutSingleParams {
        PoolKey poolKey;
        bool zeroForOne;
        uint128 amountOut;
        uint128 amountInMaximum;
        bytes hookData;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SwapExactOutParams {
        address currencyOut;
        PathKey[] path;
        uint128 amountOut;
        uint128 amountInMaximum;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SettleParams {
        address currency;
        uint256 amount;
        bool payerIsUser;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SettleAllParams {
        address currency;
        uint256 maxAmount;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SettlePairParams {
        address currency0;
        address currency1;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct TakeParams {
        address currency;
        address recipient;
        uint256 amount;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct TakeAllParams {
        address currency;
        uint256 minAmount;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct TakePortionParams {
        address currency;
        address recipient;
        uint256 bips;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct TakePairParams {
        address currency0;
        address currency1;
        address recipient;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SettleTakePairParams {
        address settleCurrency;
        address takeCurrency;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct CloseCurrencyParams {
        address currency;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct SweepParams {
        address currency;
        address recipient;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct ActionsParams {
        bytes actions;
        bytes[] params;
    }
   }
}

pub fn execute_call_selector() -> [u8; 4] {
    UniversalRouter::execute_0Call::SELECTOR
}

pub fn execute_with_deadline_call_selector() -> [u8; 4] {
   UniversalRouter::execute_1Call::SELECTOR
}

pub fn encode_execute_with_deadline(commands: Bytes, inputs: Vec<Bytes>, deadline: U256) -> Bytes {
   let data = UniversalRouter::execute_1Call {
      commands,
      inputs,
      deadline,
   }
   .abi_encode();
   data.into()
}

pub fn encode_execute(commands: Bytes, inputs: Vec<Bytes>) -> Bytes {
   let data = UniversalRouter::execute_0Call { commands, inputs }.abi_encode();
   data.into()
}

pub fn encode_exact_input_single_params(
   pool_key: PoolKey,
   zero_for_one: bool,
   amount_in: U256,
   amount_out_minimum: U256,
   hook_data: Bytes,
) -> Result<Bytes, anyhow::Error> {
   let data = ExactInputSingleParams {
      poolKey: pool_key,
      zeroForOne: zero_for_one,
      amountIn: amount_in.try_into()?,
      amountOutMinimum: amount_out_minimum.try_into()?,
      hookData: hook_data,
   }
   .abi_encode_params()
   .into();
   Ok(data)
}

pub fn encode_exact_input(
   currency_in: Address,
   path: Vec<PathKey>,
   amount_in: U256,
   amount_out_minimum: U256,
) -> Result<Bytes, anyhow::Error> {
   let data = ExactInputParams {
      currencyIn: currency_in,
      path,
      amountIn: amount_in.try_into()?,
      amountOutMinimum: amount_out_minimum.try_into()?,
   }
   .abi_encode_params()
   .into();
   Ok(data)
}

pub fn encode_exact_output_single_params(
   pool_key: PoolKey,
   zero_for_one: bool,
   amount_out: U256,
   amount_in_maximum: U256,
   hook_data: Bytes,
) -> Result<Bytes, anyhow::Error> {
   let data = ExactOutputSingleParams {
      poolKey: pool_key,
      zeroForOne: zero_for_one,
      amountOut: amount_out.try_into()?,
      amountInMaximum: amount_in_maximum.try_into()?,
      hookData: hook_data,
   }
   .abi_encode_params()
   .into();
   Ok(data)
}

pub fn encode_exact_output(
   currency_out: Address,
   path: Vec<PathKey>,
   amount_out: U256,
   amount_in_maximum: U256,
) -> Result<Bytes, anyhow::Error> {
   let data = ExactOutputParams {
      currencyOut: currency_out,
      path,
      amountOut: amount_out.try_into()?,
      amountInMaximum: amount_in_maximum.try_into()?,
   }
   .abi_encode_params()
   .into();
   Ok(data)
}
