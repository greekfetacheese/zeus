//! ABI for [EIP-7708](https://eips.ethereum.org/EIPS/eip-7708): every ETH transfer emits a log.
//!
//! The log is a `LOG3` from the system address whose topic0 is ERC-20's `Transfer` signature, with
//! the amount in wei as its data. The emitter is therefore the only thing that tells it apart from
//! a token transfer — an ERC-20 decoder accepts it as-is — which is why every decode path has to
//! ask [`is_eth_transfer_log`] before it reaches for a token.

use alloy_primitives::{Address, Log, LogData, U256};
use alloy_sol_types::{SolEvent, sol};

/// The address EIP-7708 emits ETH transfer logs from ([EIP-4788]'s `SYSTEM_ADDRESS`).
///
/// Re-exported from revm rather than written out here: revm is what the simulation emits from, so
/// the two cannot drift apart.
///
/// [EIP-4788]: https://eips.ethereum.org/EIPS/eip-4788
pub use revm::primitives::eip7708::ETH_TRANSFER_LOG_ADDRESS;

sol! {
    /// The log EIP-7708 issues for every value transfer that reaches a different account — the
    /// transaction's own transfer first, then each `CALL`/`CREATE`/`SELFDESTRUCT`. Zero-value
    /// transfers and reverts emit nothing.
    ///
    /// Its signature is deliberately ERC-20's `Transfer`, so existing indexers read it unchanged.
    contract EIP7708 {
        event Transfer(address indexed from, address indexed to, uint256 value);
    }
}

/// Whether `log` is an EIP-7708 ETH transfer log.
///
/// The emitter decides it, since `topics[0]` is shared with ERC-20.
pub fn is_eth_transfer_log(log: &Log) -> bool {
   log.address == ETH_TRANSFER_LOG_ADDRESS
}

/// Decode an EIP-7708 ETH transfer log into `(from, to, amount in wei)`.
pub fn decode_eth_transfer_log(log: &LogData) -> Result<(Address, Address, U256), anyhow::Error> {
   let transfer = EIP7708::Transfer::decode_raw_log(log.topics(), &log.data)?;
   Ok((transfer.from, transfer.to, transfer.value))
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::abi::erc20;
   use alloy_primitives::address;

   const SENDER: Address = address!("A88D2cb271E3dB59CF8CD4c84Ea7d83eEF1c94b0");
   const RECIPIENT: Address = address!("80Dbe0E147739CE637E8f6f87a912b43Aee17543");

   fn eth_transfer_log(value: U256) -> Log {
      Log {
         address: ETH_TRANSFER_LOG_ADDRESS,
         data: EIP7708::Transfer {
            from: SENDER,
            to: RECIPIENT,
            value,
         }
         .encode_log_data(),
      }
   }

   #[test]
   fn decodes_the_amount_the_emitter_and_both_endpoints() {
      // 0.002713297724087723 ETH, as a Sepolia ETH transfer reports it.
      let value = U256::from(2_713_297_724_087_723u64);
      let (from, to, amount) = decode_eth_transfer_log(&eth_transfer_log(value).data).unwrap();

      assert_eq!((from, to, amount), (SENDER, RECIPIENT, value));
   }

   /// The ERC-20 decoder takes this log too — same topic0, same shape — so the emitter is the only
   /// thing that separates an ETH transfer from a token one.
   #[test]
   fn shares_its_shape_with_an_erc20_transfer() {
      let log = eth_transfer_log(U256::from(1));
      let as_token = erc20::decode_transfer_log(&log.data).unwrap();

      assert_eq!((as_token.from, as_token.to), (SENDER, RECIPIENT));
      assert!(is_eth_transfer_log(&log));
      assert_eq!(
         erc20::IERC20::Transfer::SIGNATURE,
         EIP7708::Transfer::SIGNATURE
      );
   }

   /// A token's own `Transfer` is not an ETH transfer, however identical its topics are.
   #[test]
   fn a_token_transfer_is_not_an_eth_transfer() {
      let log = Log {
         address: address!("7b79995e5f793A07Bc00c21412e50Ecae098E7f9"),
         data: eth_transfer_log(U256::from(1)).data,
      };

      assert!(!is_eth_transfer_log(&log));
   }
}
