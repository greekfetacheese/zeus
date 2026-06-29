//! Railgun contract definitions using alloy's sol! macro.
//!
//! Accurate definitions based on current Railgun contracts (RailgunLogic.sol + Globals.sol + RailgunSmartWallet.sol).
//! Source pulled from Etherscan verified contracts + deployments repo.

use alloy_primitives::{Address, U256, address};
use alloy_sol_types::sol;

// Railgun contract address (proxy)
pub const ETHEREUM_MAINNET_RELAY: Address = address!("0xFA7093CDD9EE6932B4eb2c9e1cde7CE00B1FA4b9");

// Polygon example (common)
pub const POLYGON_MAINNET_RELAY: Address = address!("0x19cA1dF4a6A8aC8B0f9C6e3E2a2a2a2a2a2a2a2a"); // TODO: replace with real Railgun proxy on Polygon (from deployments)

sol! {
    #[derive(Debug, PartialEq, Eq)]
    contract RailgunSmartWallet {
        // Core transaction events (from RailgunLogic / SmartWallet)
        event Shield(
            uint256 treeNumber,
            uint256 startPosition,
            CommitmentPreimage[] commitments,
            ShieldCiphertext[] shieldCiphertext,
            uint256[] fees
        );

        event Transact(
            uint256 treeNumber,
            uint256 startPosition,
            bytes32[] hash,
            CommitmentCiphertext[] ciphertext
        );

        event Unshield(
            address to,
            TokenData token,
            uint256 amount,
            uint256 fee
        );

        event Nullified(
            uint16 treeNumber,
            bytes32[] nullifier
        );

        // Supporting structs (from Globals.sol)
        struct TokenData {
            uint8 tokenType;      // 0=ERC20, 1=ERC721, 2=ERC1155
            address tokenAddress;
            uint256 tokenSubID;
        }

        struct CommitmentPreimage {
            bytes32 npk;          // note public key
            TokenData token;
            uint120 value;
        }

        struct ShieldCiphertext {
            bytes32[3] encryptedBundle;
            bytes32 shieldKey;
        }

        struct CommitmentCiphertext {
            bytes32[4] ciphertext;
            bytes32 blindedSenderViewingKey;
            bytes32 blindedReceiverViewingKey;
            bytes annotationData;
            bytes memo;
        }
    }
}

/// Get Railgun contract address (the proxy users interact with) for a chain id.
pub fn railgun_address(chain_id: u64) -> Option<Address> {
   match chain_id {
      1 => Some(ETHEREUM_MAINNET_RELAY),
      137 => Some(POLYGON_MAINNET_RELAY), // update with real address
      _ => None,
   }
}

/// High-level decoded Railgun events for the scanner.
#[derive(Debug, Clone)]
pub enum RailgunEvent {
   Shield {
      tree_number: U256,
      start_position: U256,
      commitments: Vec<RailgunSmartWallet::CommitmentPreimage>,
      shield_ciphertext: Vec<RailgunSmartWallet::ShieldCiphertext>,
      fees: Vec<U256>,
   },
   Transact {
      tree_number: U256,
      start_position: U256,
      /// The actual leaf hashes inserted into the Merkle tree
      hashes: Vec<[u8; 32]>,
      ciphertexts: Vec<RailgunSmartWallet::CommitmentCiphertext>,
   },
   Nullified {
      tree_number: u16,
      nullifiers: Vec<[u8; 32]>,
   },
   Unshield {
      to: Address,
      token: RailgunSmartWallet::TokenData,
      amount: U256,
      fee: U256,
   },
}

// Re-exports for convenience
pub use RailgunSmartWallet::{
   CommitmentCiphertext, CommitmentPreimage, ShieldCiphertext, TokenData,
};
