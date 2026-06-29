//! Railgun contract definitions using alloy's sol! macro.
//!
//! Defines the important events and types for scanning Railgun activity.

use alloy_primitives::{Address, U256, address};
use alloy_sol_types::sol;

// Known Railgun contract addresses per chain.

/// Current relay contract (proxy that calls the implementation)
pub const ETHEREUM_MAINNET_RELAY: Address =
   address!("0xFA7093CDD9EE6932B4eb2c9e1cde7CE00B1FA4b9");

/// Current implementation contract
pub const ETHEREUM_MAINNET_IMPLEMENTATION: Address =
   address!("0xB4F2d77bD12c6b548Ae398244d7FAD4ABCE4D89b");

// TODO: Add more chains

sol! {
    #[derive(Debug, PartialEq, Eq)]
    contract RailgunSmartWallet {
        // Core events for the scanner

        event Shield(
            uint256 indexed treeNumber,
            uint256 startPosition,
            Commitment[] commitments
        );

        event Transact(
            uint256 indexed treeNumber,
            uint256 startPosition,
            bytes32[] hash
        );

        event GeneratedCommitmentBatch(
            uint256 indexed treeNumber,
            uint256 startPosition,
            Commitment[] commitments
        );

        event CommitmentBatch(
            uint256 indexed treeNumber,
            uint256 startPosition,
            uint256[] hash
        );

        event Nullifiers(
            uint256 indexed treeNumber,
            uint256[] nullifier
        );

        event Unshield(
            address to,
            TokenData token,
            uint256 amount
        );

        struct TokenData {
            address tokenAddress;
            uint8 tokenType;
            uint256 tokenSubID;
        }

        struct Commitment {
            uint256 npk;
            uint256 token;
            uint256 value;
        }
    }
}

/// Get Railgun contract address for a chain id.
pub fn railgun_address(chain_id: u64) -> Option<Address> {
   match chain_id {
      1 => Some(ETHEREUM_MAINNET_RELAY),
      _ => None,
   }
}

/// High-level decoded Railgun events useful for the scanner.
#[derive(Debug, Clone)]
pub enum RailgunEvent {
   Shield {
      tree_number: U256,
      start_position: U256,
      commitments: Vec<RailgunSmartWallet::Commitment>,
   },
   Transact {
      tree_number: U256,
      start_position: U256,
      hashes: Vec<[u8; 32]>,
   },
   Nullifiers {
      tree_number: U256,
      nullifiers: Vec<U256>,
   },
   Unshield {
      to: Address,
      token: RailgunSmartWallet::TokenData,
      amount: U256,
   },
}

// Re-export generated types
pub use RailgunSmartWallet::{Commitment, TokenData};
