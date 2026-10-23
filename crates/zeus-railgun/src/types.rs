use alloy_primitives::{Address, Bytes, U256};
use secure_types::SecureArray;
use serde::{Deserialize, Serialize};

pub type Key32 = SecureArray<u8, 32>;
pub type Key64 = SecureArray<u8, 64>;

/// The HMAC chain code of one BIP32-style derivation step.
///
/// A newtype rather than `pub type ChainCode = Key32`: the two are the same 32-byte secret and a
/// derivation step threads both through the same tuple, so without a distinct type a transposed
/// `(key, chain_code)` compiles and silently derives the wrong key.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChainCode(Key32);

impl ChainCode {
   pub fn new(key: Key32) -> Self {
      Self(key)
   }
}

impl std::ops::Deref for ChainCode {
   type Target = Key32;
   fn deref(&self) -> &Key32 {
      &self.0
   }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TxData {
   pub to: Address,
   pub data: Bytes,
   pub value: U256,
}

impl TxData {
   pub fn new(to: Address, data: Bytes, value: U256) -> Self {
      TxData { to, data, value }
   }
}

/// Deliberately exhaustive: a new variant must break every `match` in the consuming app, so the
/// compiler — not a silent wildcard — forces the update. The crate owns that breakage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TxidVersion {
   #[serde(rename = "V2_PoseidonMerkle")]
   V2PoseidonMerkle,
   #[serde(rename = "V3_PoseidonMerkle")]
   V3PoseidonMerkle,
}

impl std::fmt::Display for TxidVersion {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         TxidVersion::V2PoseidonMerkle => f.write_str("V2_PoseidonMerkle"),
         TxidVersion::V3PoseidonMerkle => f.write_str("V3_PoseidonMerkle"),
      }
   }
}

/// Deliberately exhaustive: a new variant must break every `match` in the consuming app, so the
/// compiler — not a silent wildcard — forces the update. The crate owns that breakage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
   use super::TxidVersion;

   /// The `Display` form must stay identical to the `#[serde(rename)]` wire form.
   #[test]
   fn txid_version_to_string_matches_the_wire_form() {
      assert_eq!(
         TxidVersion::V2PoseidonMerkle.to_string(),
         "V2_PoseidonMerkle"
      );
      assert_eq!(
         TxidVersion::V3PoseidonMerkle.to_string(),
         "V3_PoseidonMerkle"
      );
   }
}
