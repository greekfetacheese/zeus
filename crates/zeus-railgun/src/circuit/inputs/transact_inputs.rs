use std::collections::HashMap;

use ruint::aliases::U256;

use crate::{
   account::signer::RailgunSigner,
   caip::AssetId,
   crypto::poseidon_hash,
   merkle_tree::{MerkleRoot, MerkleTreeError, RailgunMerkleProof, UtxoMerkleTree},
   note::{OutputNote, utxo::UtxoNote},
};

#[derive(Debug, Clone)]
pub struct TransactCircuitInputs {
   // Public Inputs
   pub merkleroot: MerkleRoot,
   pub bound_params_hash: U256,
   pub nullifiers: Vec<U256>,
   pub commitments_out: Vec<U256>,

   // Private Inputs
   token: U256,
   public_key: [U256; 2],
   signature: [U256; 3],
   random_in: Vec<U256>,
   value_in: Vec<U256>,
   path_elements: Vec<Vec<U256>>,
   leaves_indices: Vec<U256>,
   nullifying_key: U256,
   npk_out: Vec<U256>,
   value_out: Vec<U256>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum TransactCircuitInputsError {
   EmptyInputNotes,
   MerkleTree(MerkleTreeError),
   Signing(anyhow::Error),
   InvalidMerkleProof,
   UnsupportedPoseidonArity(usize),
}

impl std::fmt::Display for TransactCircuitInputsError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::EmptyInputNotes => f.write_str("Empty input notes"),
         Self::MerkleTree(err) => write!(f, "Merkle tree error: {err}"),
         Self::Signing(err) => write!(f, "Signing error: {err}"),
         Self::InvalidMerkleProof => f.write_str("Merkle proof does not match note or tree root"),
         Self::UnsupportedPoseidonArity(arity) => {
            write!(
               f,
               "Unsupported Poseidon arity for {arity} transact inputs"
            )
         }
      }
   }
}

impl std::error::Error for TransactCircuitInputsError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::MerkleTree(err) => Some(err),
         // `anyhow::Error` is not itself a `std::error::Error`, so route through its own
         // `as_ref()` instead of borrowing the wrapper.
         Self::Signing(err) => Some(err.as_ref()),
         Self::EmptyInputNotes | Self::InvalidMerkleProof | Self::UnsupportedPoseidonArity(_) => {
            None
         }
      }
   }
}

impl From<MerkleTreeError> for TransactCircuitInputsError {
   fn from(err: MerkleTreeError) -> Self {
      Self::MerkleTree(err)
   }
}

impl From<anyhow::Error> for TransactCircuitInputsError {
   fn from(err: anyhow::Error) -> Self {
      Self::Signing(err)
   }
}

impl TransactCircuitInputs {
   pub fn from_inputs(
      merkle_tree: &UtxoMerkleTree,
      bound_params_hash: U256,
      signer: &RailgunSigner,
      asset: AssetId,
      notes_in: &[UtxoNote],
      notes_out: &[OutputNote],
   ) -> Result<Self, TransactCircuitInputsError> {
      if notes_in.is_empty() || notes_out.is_empty() {
         return Err(TransactCircuitInputsError::EmptyInputNotes);
      }

      let merkle_proofs: Vec<_> = notes_in
         .iter()
         .map(|note| merkle_tree.generate_proof(note.hash()))
         .collect::<Result<_, _>>()?;

      Self::from_notes_and_proofs(
         merkle_tree.root(),
         bound_params_hash,
         signer,
         asset,
         notes_in,
         notes_out,
         &merkle_proofs,
      )
   }

   pub fn from_notes_and_proofs(
      merkleroot: MerkleRoot,
      bound_params_hash: U256,
      signer: &RailgunSigner,
      asset: AssetId,
      notes_in: &[UtxoNote],
      notes_out: &[OutputNote],
      merkle_proofs: &[RailgunMerkleProof],
   ) -> Result<Self, TransactCircuitInputsError> {
      if notes_in.is_empty() || notes_out.is_empty() {
         return Err(TransactCircuitInputsError::EmptyInputNotes);
      }
      if merkle_proofs.len() != notes_in.len() {
         return Err(TransactCircuitInputsError::InvalidMerkleProof);
      }

      for (note, proof) in notes_in.iter().zip(merkle_proofs) {
         let leaf: U256 = note.hash().into();
         if proof.element != leaf || proof.root != merkleroot || !proof.verify() {
            return Err(TransactCircuitInputsError::InvalidMerkleProof);
         }
      }

      let nullifiers: Vec<U256> = notes_in.iter().map(|note| note.nullifier).collect();
      let commitments: Vec<U256> = notes_out.iter().map(|note| note.hash().into()).collect();

      let token = asset.hash();
      let public_key = signer.keys().spending_public_key;
      let public_key = [public_key.x_u256(), public_key.y_u256()];

      let mut unsigned = vec![merkleroot.into(), bound_params_hash];
      unsigned.extend_from_slice(&nullifiers);
      unsigned.extend_from_slice(&commitments);
      let unsigned_hash = poseidon_hash(&unsigned)
         .map_err(|_| TransactCircuitInputsError::UnsupportedPoseidonArity(unsigned.len()))?;
      let signature = signer.sign(unsigned_hash)?;
      let signature = [signature.r8_x, signature.r8_y, signature.s];

      let random_in = notes_in.iter().map(|note| U256::from_be_slice(&note.random)).collect();

      let value_in = notes_in.iter().map(|note| U256::from(note.value())).collect();

      let path_elements = merkle_proofs.iter().map(|p| p.elements.clone()).collect();

      let leaves_indices = merkle_proofs.iter().map(|p| U256::from(p.indices)).collect();

      let nullifying_key = signer.keys().nullifying_key.to_u256();
      let npk_out = notes_out.iter().map(|note| note.note_public_key()).collect();
      let value_out = notes_out.iter().map(|note| U256::from(note.value())).collect();

      Ok(TransactCircuitInputs {
         merkleroot,
         bound_params_hash,
         nullifiers,
         commitments_out: commitments,
         token,
         public_key,
         signature,
         random_in,
         value_in,
         path_elements,
         leaves_indices,
         nullifying_key,
         npk_out,
         value_out,
      })
   }

   pub fn to_circuit_signals(&self) -> HashMap<String, Vec<U256>> {
      let mut m = HashMap::with_capacity(14);
      m.insert("merkleRoot".into(), vec![self.merkleroot.into()]);
      m.insert(
         "boundParamsHash".into(),
         vec![self.bound_params_hash],
      );
      m.insert("nullifiers".into(), self.nullifiers.clone());
      m.insert(
         "commitmentsOut".into(),
         self.commitments_out.clone(),
      );
      m.insert("token".into(), vec![self.token]);
      m.insert("publicKey".into(), self.public_key.to_vec());
      m.insert("signature".into(), self.signature.to_vec());
      m.insert("randomIn".into(), self.random_in.clone());
      m.insert("valueIn".into(), self.value_in.clone());
      m.insert(
         "pathElements".into(),
         self.path_elements.iter().flatten().copied().collect(),
      );
      m.insert(
         "leavesIndices".into(),
         self.leaves_indices.clone(),
      );
      m.insert("nullifyingKey".into(), vec![self.nullifying_key]);
      m.insert("npkOut".into(), self.npk_out.clone());
      m.insert("valueOut".into(), self.value_out.clone());
      m
   }
}

#[cfg(test)]
mod error_tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_messages() {
      assert_eq!(
         TransactCircuitInputsError::EmptyInputNotes.to_string(),
         "Empty input notes"
      );
      assert_eq!(
         TransactCircuitInputsError::MerkleTree(MerkleTreeError::InvalidProof).to_string(),
         "Merkle tree error: Invalid proof"
      );
      assert_eq!(
         TransactCircuitInputsError::Signing(anyhow::anyhow!("boom")).to_string(),
         "Signing error: boom"
      );
      assert_eq!(
         TransactCircuitInputsError::InvalidMerkleProof.to_string(),
         "Merkle proof does not match note or tree root"
      );
      assert_eq!(
         TransactCircuitInputsError::UnsupportedPoseidonArity(4).to_string(),
         "Unsupported Poseidon arity for 4 transact inputs"
      );
   }

   #[test]
   fn the_merkle_tree_variant_delegates_its_source() {
      let inner = MerkleTreeError::ElementNotFound("leaf".to_owned());
      let rendered = inner.to_string();
      assert_eq!(
         TransactCircuitInputsError::MerkleTree(inner).source().map(ToString::to_string),
         Some(rendered)
      );
   }

   #[test]
   fn the_signing_variant_delegates_its_anyhow_source() {
      let inner = anyhow::anyhow!("boom");
      let rendered = inner.to_string();
      assert_eq!(
         TransactCircuitInputsError::Signing(inner).source().map(ToString::to_string),
         Some(rendered)
      );
   }

   #[test]
   fn the_payloadless_variants_have_no_source() {
      assert!(TransactCircuitInputsError::EmptyInputNotes.source().is_none());
      assert!(TransactCircuitInputsError::InvalidMerkleProof.source().is_none());
      assert!(TransactCircuitInputsError::UnsupportedPoseidonArity(4).source().is_none());
   }
}
