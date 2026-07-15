pub mod config;
pub mod hex_u256;
pub mod proof;
pub mod tree;
pub mod txid_tree;
pub mod utxo_tree;

use alloy_sol_types::SolCall;
pub use config::{MerkleConfig, RailgunMerkleConfig, TOTAL_LEAVES, TREE_DEPTH};
pub use proof::{MerkleRoot, RailgunMerkleProof};
pub use tree::{MerkleTree, MerkleTreeError, RailgunMerkleTree, RailgunMerkleTreeState};
pub use txid_tree::{TxidLeafHash, TxidMerkleTree, UtxoTreeIndex};
pub use utxo_tree::{UtxoLeafHash, UtxoMerkleTree};

use alloy_primitives::Address;
use alloy_provider::{DynProvider, Provider, network::Ethereum};
use alloy_rpc_types::{BlockId, TransactionRequest};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::abi::railgun::RailgunSmartWallet;

/// Validates a Merkle root against an external authority (e.g. on-chain or a POI node).
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait MerkleTreeVerifier: crate::MaybeSend {
   async fn verify_root(
      &self,
      tree_number: u32,
      index: u32,
      root: MerkleRoot,
      block_id: Option<BlockId>,
   ) -> Result<bool, Box<dyn std::error::Error + Send + Sync + 'static>>;

   /// Replaces the underlying RPC provider at runtime.
   ///
   /// Object-safe: takes a type-erased [`DynProvider`] so it can be called through
   /// `Arc<dyn MerkleTreeVerifier>`.
   async fn set_provider(&self, provider: DynProvider<Ethereum>);
}

/// A Merkle root verifier that uses a Json RPC client
pub struct RootVerifier {
   railgun_address: Address,
   /// Stored behind a shared `Mutex` so the swap works through `Arc<dyn MerkleTreeVerifier>`.
   provider: Arc<Mutex<DynProvider<Ethereum>>>,
}

impl RootVerifier {
   pub fn new(provider: impl Provider<Ethereum> + 'static, railgun_address: Address) -> Self {
      Self {
         railgun_address,
         provider: Arc::new(Mutex::new(DynProvider::new(provider))),
      }
   }

   pub async fn set_provider(&self, provider: DynProvider<Ethereum>) {
      let cell = self.provider.clone();
      *cell.lock().await = provider;
   }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl MerkleTreeVerifier for RootVerifier {
   async fn verify_root(
      &self,
      tree_number: u32,
      _index: u32,
      root: MerkleRoot,
      block_id: Option<BlockId>,
   ) -> Result<bool, Box<dyn std::error::Error + Send + Sync + 'static>> {
      let block_id = block_id.unwrap_or(BlockId::latest());

      let tree_number = alloy_primitives::U256::from(tree_number);
      let root_u: alloy_primitives::U256 = root.into();

      let call = RailgunSmartWallet::rootHistoryCall {
         treeNumber: tree_number,
         root: root_u.into(),
      };

      let tx = TransactionRequest::default()
         .to(self.railgun_address)
         .input(call.abi_encode().into());

      let client = self.provider.lock().await.clone();
      let data = client.call(tx).block(block_id).await?;

      // Explicit bool decode is more robust across alloy versions.
      let exists = <bool as alloy_sol_types::SolValue>::abi_decode(&data)?;

      Ok(exists)
   }

   async fn set_provider(&self, provider: DynProvider<Ethereum>) {
      self.set_provider(provider).await;
   }
}
