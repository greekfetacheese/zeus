//! Railgun contract definitions using alloy's sol! macro.
//!
//! Accurate definitions based on current Railgun contracts (RailgunLogic.sol + Globals.sol + RailgunSmartWallet.sol).
//! Source pulled from Etherscan verified contracts + deployments repo.

use alloy_primitives::{Address, U256, address};
use alloy_sol_types::sol;

// Railgun contract address (proxy)
pub const ETHEREUM_MAINNET_RELAY: Address = address!("0xFA7093CDD9EE6932B4eb2c9e1cde7CE00B1FA4b9");

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

        // --- Transact / gas-sponsored support (from Globals.sol) ---
        enum UnshieldType {
            NONE,
            NORMAL,
            REDIRECT
        }

        struct G1Point {
            uint256 x;
            uint256 y;
        }

        struct G2Point {
            uint256[2] x;
            uint256[2] y;
        }

        struct SnarkProof {
            G1Point a;
            G2Point b;
            G1Point c;
        }

        struct BoundParams {
            uint16 treeNumber;
            uint72 minGasPrice; // Only for type 0 transactions
            UnshieldType unshield;
            uint64 chainID;
            address adaptContract;
            bytes32 adaptParams;
            // For unshields do not include an element in ciphertext array
            // Ciphertext array length = commitments - unshields
            CommitmentCiphertext[] commitmentCiphertext;
        }

        struct Transaction {
            SnarkProof proof;
            bytes32 merkleRoot;
            bytes32[] nullifiers;
            bytes32[] commitments;
            BoundParams boundParams;
            CommitmentPreimage unshieldPreimage;
        }

        /// Main entry point for private actions + unshield (used heavily for broadcaster-sponsored gas abstraction)
        function transact(Transaction[] calldata _transactions) external;
    }
}

/// Get Railgun contract address (the proxy users interact with) for a chain id.
pub fn railgun_address(chain_id: u64) -> Option<Address> {
   match chain_id {
      1 => Some(ETHEREUM_MAINNET_RELAY),
      _ => None,
   }
}

/// The deployment block of the implementation contract
pub fn deployment_block(chain_id: u64) -> Option<u64> {
   match chain_id {
      1 => Some(15964145),
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
   BoundParams, CommitmentCiphertext, CommitmentPreimage, ShieldCiphertext, SnarkProof, TokenData,
   Transaction, UnshieldType,
};
