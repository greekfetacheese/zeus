use super::{poseidon_hash, poseidon3};
use ruint::aliases::U256;
use serde::{Deserialize, Serialize, Serializer};

use crate::{merkle_tree::RailgunMerkleTree, transact::proved_transaction::ProvedOperation};

/// TxID uniquely identifies a Railgun Operation (`RailgunSmartWallet::Transaction`).
/// Each TxID corresponds to a set of UTXO notes from a single Operation.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Txid(U256);

impl Txid {
   pub fn new(nullifiers: &[U256], commitments: &[U256], bound_params_hash: U256) -> Self {
      let max_nullifiers = 13; // Max circuit inputs
      let max_commitments = 13; // Max circuit outputs

      // This is deeply unfortunate given the performance implications
      let mut nullifiers_padded = [RailgunMerkleTree::zero(); 13];
      let mut commitments_padded = [RailgunMerkleTree::zero(); 13];

      for (i, &nullifier) in nullifiers.iter().take(max_nullifiers).enumerate() {
         nullifiers_padded[i] = nullifier;
      }
      for (i, &commitment) in commitments.iter().take(max_commitments).enumerate() {
         commitments_padded[i] = commitment;
      }

      let nullifiers_hash = poseidon_hash(&nullifiers_padded)
         .expect("13 padded inputs are a supported poseidon arity");
      let commitments_hash = poseidon_hash(&commitments_padded)
         .expect("13 padded inputs are a supported poseidon arity");

      poseidon3(
         nullifiers_hash,
         commitments_hash,
         bound_params_hash,
      )
      .into()
   }

   pub fn from_operation(op: &ProvedOperation) -> Self {
      let nullifiers: Vec<_> = op.inner.in_notes().iter().map(|n| n.nullifier).collect();

      let commitments: Vec<_> = op.inner.out_notes().iter().map(|n| n.hash().into()).collect();

      Self::new(
         &nullifiers,
         &commitments,
         op.circuit_inputs.bound_params_hash,
      )
   }
}

impl From<U256> for Txid {
   fn from(value: U256) -> Self {
      Txid(value)
   }
}

impl From<Txid> for U256 {
   fn from(val: Txid) -> Self {
      val.0
   }
}

impl Serialize for Txid {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: Serializer,
   {
      serializer.serialize_str(&format!("{:064x}", self.0))
   }
}

impl<'de> Deserialize<'de> for Txid {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      let s = String::deserialize(deserializer)?;
      let s = s.strip_prefix("0x").unwrap_or(&s);
      let value = U256::from_str_radix(s, 16).map_err(serde::de::Error::custom)?;
      Ok(Txid(value))
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn txid_wire_form_is_lowercase_hex_and_accepts_an_0x_prefix() {
      let txid = Txid::from(U256::from(0xdead_beefu64));

      assert_eq!(
         serde_json::to_string(&txid).unwrap(),
         "\"00000000000000000000000000000000000000000000000000000000deadbeef\""
      );

      assert_eq!(
         serde_json::from_str::<Txid>(
            "\"00000000000000000000000000000000000000000000000000000000deadbeef\""
         )
         .unwrap(),
         txid
      );
      assert_eq!(
         serde_json::from_str::<Txid>(
            "\"0x00000000000000000000000000000000000000000000000000000000deadbeef\""
         )
         .unwrap(),
         txid
      );

      let round_tripped: U256 = txid.into();
      assert_eq!(round_tripped, U256::from(0xdead_beefu64));
   }
}
