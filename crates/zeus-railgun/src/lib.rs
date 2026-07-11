pub mod abi;
pub mod account;
pub mod adapter_data;
pub mod caip;
pub mod chain_config;
pub mod circuit;
pub mod crypto;
pub mod database;
pub mod indexer;
pub mod merkle_tree;
pub mod note;
pub mod poi;
pub mod provider;
pub mod transact;
pub mod types;

pub use provider::{RailgunProvider, BalanceEntry};
pub use database::RedbDatabase;
pub use account::{address::RailgunAddress, signer::RailgunSigner};
pub use chain_config::ChainConfig;
pub use merkle_tree::RootVerifier;
pub use indexer::{utxo_indexer::UtxoIndexer, syncer::RpcSyncer, syncer::{UtxoSyncer, subsquid::SubsquidSyncer}};
pub use circuit::groth16_prover::Groth16Prover;

pub use rand;

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> MaybeSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}
