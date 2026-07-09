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

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> MaybeSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}
