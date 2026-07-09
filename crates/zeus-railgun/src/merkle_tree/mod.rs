pub mod config;
pub mod hex_u256;
pub mod proof;
pub mod tree;
pub mod txid_tree;
pub mod utxo_tree;

pub use config::{RailgunMerkleConfig, MerkleConfig, TREE_DEPTH, TOTAL_LEAVES};
pub use proof::{RailgunMerkleProof, MerkleRoot};
pub use tree::{RailgunMerkleTree, RailgunMerkleTreeState, MerkleTree, MerkleTreeError};
pub use txid_tree::{TxidLeafHash, TxidMerkleTree, UtxoTreeIndex};
pub use utxo_tree::{UtxoLeafHash, UtxoMerkleTree};

/// Validates a Merkle root against an external authority (e.g. on-chain or a POI node).
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait MerkleTreeVerifier: crate::MaybeSend {
    async fn verify_root(
        &self,
        tree_number: u32,
        tree_index: u32,
        root: MerkleRoot,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync + 'static>>;
}