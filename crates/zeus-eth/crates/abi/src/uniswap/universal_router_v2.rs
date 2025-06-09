use super::v4::{PathKey, PoolKey};
use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{SolCall, SolValue, sol};

pub use IV4Router::{ExactInputParams, ExactInputSingleParams, ExactOutputParams, ExactOutputSingleParams};

sol! {

    contract UniversalRouter {
        function execute(bytes calldata commands, bytes[] calldata inputs) public payable;

        function execute(bytes calldata commands, bytes[] calldata inputs, uint256 deadline)
        external
        payable;
    }

    interface IV4Router {

         type Currency is address;

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
        }
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
