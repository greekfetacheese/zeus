pub mod nft_position;
pub mod v2;
pub mod v3;
pub mod v4;

use crate::permit::Permit2;
use alloy_primitives::{Address, Bytes, U256, aliases::U24};
use alloy_sol_types::{SolValue, sol};

// https://docs.uniswap.org/contracts/universal-router/technical-reference
sol! {

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
        uint256 amount;
    }
}

pub fn encode_wrap_eth(recipient: Address, amount: U256) -> Bytes {
   let data = WrapEth { recipient, amount }.abi_encode_params();

   data.into()
}

pub fn encode_unwrap_weth(recipient: Address, amount: U256) -> Bytes {
   let data = UnwrapWeth { recipient, amount }.abi_encode_params();

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
      path.extend_from_slice(&token.to_vec());
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
