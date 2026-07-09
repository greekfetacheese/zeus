pub mod abi;
pub mod account;
pub mod caip;
pub mod circuit;
pub mod crypto;
pub mod database;
pub mod merkle_tree;
pub mod note;
pub mod poi;
pub mod transact;
pub mod types;
pub mod indexer;

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> MaybeSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}
