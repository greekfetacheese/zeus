use alloy_primitives::{Address, Signature, Bytes, U256, aliases::{U48, U160}};
use alloy_sol_types::{SolCall, SolValue, sol};

use super::Permit2;
use super::v4::actions::*;
pub use IV4Router::{PathKey, PoolKey};

sol! {
    type Currency is address;

    contract UniversalRouter {
        function execute(bytes calldata commands, bytes[] calldata inputs) public payable;

        function execute(bytes calldata commands, bytes[] calldata inputs, uint256 deadline)
        external
        payable;
    }

   interface IHooks {}

    interface IV4Router {

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
   }

   struct Permit2TransferFrom {
        address token;
        address recipient;
        uint160 amount;
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

pub fn encode_permit2_transfer_from(
   token: Address,
   recipient: Address,
   amount: U256,
) -> Bytes {
   let data = Permit2TransferFrom {
      token,
      recipient,
      amount: U160::from(amount),
   }
   .abi_encode_params()
   .into();
   data
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
   let encoded_args = (permit_single, sig_bytes).abi_encode_params();

   encoded_args.into()
}