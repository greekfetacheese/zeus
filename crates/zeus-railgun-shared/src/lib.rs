use secure_types::SecureArray;

pub mod address;
pub mod keys;
pub mod crypto;

pub type Key32 = SecureArray<u8, 32>;
pub type ChainCode = SecureArray<u8, 32>;
pub type Key64 = SecureArray<u8, 64>;

pub use address::{RailgunAddress, encode_address};
pub use keys::RailgunKeys;
pub use crypto::poseidon_hash;


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Chain {
   pub type_: u8,
   pub id: u64,
}

impl From<u64> for Chain {
   fn from(id: u64) -> Self {
      Self { type_: 0, id }
   }
}

impl Chain {
   pub const ETHEREUM_MAINNET: Self = Self { type_: 0, id: 1 };
   pub const POLYGON_MAINNET: Self = Self { type_: 0, id: 137 };
}