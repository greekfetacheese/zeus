use alloy_primitives::{
   Address, Bytes, Signature, U256,
   aliases::{U48, U24, U160},
};
use alloy_sol_types::{SolCall, SolValue, sol};

use crate::abi::permit::Permit2;
use super::v4::actions::*;
pub use IV4Router::{PathKey, PoolKey};

// https://docs.uniswap.org/contracts/universal-router/technical-reference
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

    #[derive(Debug, Default, PartialEq, Eq)]
    struct V3SwapExactIn {
        // The recipient of the output
        address recipient;
        // The amount of input token to send
        uint256 amountIn;
        // The minimum amount of output token that must be received
        uint256 amountOutMinimum;
        // The encoded path to trade
        bytes path;
         // Whether the funds should come from msg.sender or are already in the router
        bool permit2;
    }

   #[derive(Debug, Default, PartialEq, Eq)]
    struct V3SwapExactOut {
        // The recipient of the output
        address recipient;
        // The amount of output token to receive
        uint256 amountOut;
        // The maximum amount of input token to send
        uint256 amountInMaximum;
        // The encoded path to trade
        bytes path;
        // Whether the funds should come from msg.sender or are already in the router
        bool permit2;
    }

    /// Grants router permission to operate on a user’s v3 NFT
    #[derive(Debug, Default, PartialEq, Eq)]
    struct V3PositionManagerPermit {
      address spender;
      uint256 tokenId;
      uint256 deadline;
      uint8 v;
      bytes32 r;
      bytes32 s;
    }

    // V3_POSITION_MANAGER_CALL
    // bytes callData
    //  Executes v3 NFT ops like burn, collect, decreaseLiquidity

    #[derive(Debug, Default, PartialEq, Eq)]
    struct Permit2TransferFrom {
        address token;
        address recipient;
        uint160 amount;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct Permit2Permit {
        // A PermitSingle struct outlining all of the Permit2 permits to execute.
        Permit2.PermitSingle permitSingle;
        // the signature to provide to permit2
        bytes signature;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct Permit2PermitBatch {
        // A PermitBatch struct outlining all of the Permit2 permits to execute.
        Permit2.PermitBatch permitBatch;
        // the signature to provide to permit2
        bytes signature;
    }

        #[derive(Debug, Default, PartialEq, Eq)]
    struct V2SwapExactIn {
        // the recipient of the output
        address recipient;
        // the amount of input token to send
        uint256 amountIn;
        // the minimum amount of output token that must be received
        uint256 amountOutMinimum;
        // the token path to trade
        address[] path;
        // whether the funds should come from msg.sender or are already in the router
        bool permit2;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct V2SwapExactOut {
        // the recipient of the output
        address recipient;
        // the amount of output token to receive
        uint256 amountOut;
        // the maximum amount of input token to send
        uint256 amountInMaximum;
        // the token path to trade
        address[] path;
        // whether the funds should come from msg.sender or are already in the router
        bool permit2;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct WrapEth {
        // the recipient of the wrapped eth
        address recipient;
        // the amount of eth to wrap
        uint256 amount;
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct UnwrapWeth {
        // the recipient of the eth
        address recipient;
        // the minimum amount of eth to receive
        uint256 amountMin;
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

pub fn encode_wrap_eth(recipient: Address, amount: U256) -> Bytes {
   let data = WrapEth { recipient, amount }.abi_encode_params();

   data.into()
}

pub fn encode_unwrap_weth(recipient: Address, amount_min: U256) -> Bytes {
   let data = UnwrapWeth {
      recipient,
      amountMin: amount_min,
   }
   .abi_encode_params();

   data.into()
}

pub fn encode_v3_swap_exact_in(
   recipient: Address,
   amount_in: U256,
   amount_out_minimum: U256,
   path: Vec<Address>,
   fees: Vec<U24>,
   permit2: bool,
) -> Result<Bytes, anyhow::Error> {
   let path = encode_v3_path(path, fees);
   let data = V3SwapExactIn {
      recipient,
      amountIn: amount_in,
      amountOutMinimum: amount_out_minimum,
      path,
      permit2,
   }
   .abi_encode_params()
   .into();
   Ok(data)
}

fn encode_v3_path(tokens: Vec<Address>, fees: Vec<U24>) -> Bytes {
   // Ensure the number of tokens is one more than the number of fees
   assert_eq!(
      tokens.len(),
      fees.len() + 1,
      "Invalid path: tokens.len() must be fees.len() + 1"
   );

   let mut path = Vec::new();
   for (i, token) in tokens.iter().enumerate() {
      path.extend_from_slice(token.as_ref());
      if i < fees.len() {
         let fee = fees[i];
         let fee_bytes: [u8; 3] = fee.to_be_bytes();
         path.extend_from_slice(&fee_bytes);
      }
   }
   Bytes::from(path)
}

pub fn encode_v2_swap_exact_in(
   recipient: Address,
   amount_in: U256,
   amount_out_minimum: U256,
   path: Vec<Address>,
   permit2: bool,
) -> Result<Bytes, anyhow::Error> {
   let data = V2SwapExactIn {
      recipient,
      amountIn: amount_in,
      amountOutMinimum: amount_out_minimum,
      path,
      permit2,
   }
   .abi_encode_params()
   .into();
   Ok(data)
}

pub fn encode_permit2_transfer_from(token: Address, recipient: Address, amount: U256) -> Bytes {
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
