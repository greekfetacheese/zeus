//! Witness types for Railgun circuits.
//!
//! These types are derived from the official Railgun engine implementation:
//!   engine/src/models/prover-types.ts
//!   engine/src/prover/prover.ts  → formatRailgunInputs()
//!
//! The main type `FormattedCircuitInputsRailgun` is what gets passed
//! to snarkjs.groth16.fullProve (or the native prover).

use serde::{Deserialize, Serialize};

/// The exact shape expected by the Railgun Groth16 circuit (Railgun v2/v3 Poseidon Merkle).
///
/// All fields are big integers (serialized as strings in JSON for safety with large numbers).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormattedCircuitInputsRailgun {
   pub merkle_root: String,
   pub bound_params_hash: String,
   pub nullifiers: Vec<String>,
   pub commitments_out: Vec<String>,

   /// Token address (as bigint string)
   pub token: String,

   /// Spending public key [x, y] (Baby Jubjub)
   pub public_key: Vec<String>,

   /// Signature [R8x, R8y, S] or similar — depends on exact circuit
   pub signature: Vec<String>,

   /// Randomness for each input note
   pub random_in: Vec<String>,

   /// Values for each input note
   pub value_in: Vec<String>,

   /// Flattened Merkle path elements (pathElements.flat(2) in TS)
   pub path_elements: Vec<String>,

   /// Leaf indices in the tree for each input
   pub leaves_indices: Vec<String>,

   /// Nullifying key (viewing key scalar)
   pub nullifying_key: String,

   /// npk (note public key) for each output note
   pub npk_out: Vec<String>,

   /// Values for each output note
   pub value_out: Vec<String>,
}

/// Public inputs that go into the proof (visible on-chain).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicInputsRailgun {
   pub merkle_root: String,
   pub bound_params_hash: String,
   pub nullifiers: Vec<String>,
   pub commitments_out: Vec<String>,
}

/// Private inputs (witness only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateInputsRailgun {
   pub token_address: String,
   pub public_key: Vec<String>,
   pub random_in: Vec<String>,
   pub value_in: Vec<String>,
   pub path_elements: Vec<Vec<String>>,
   pub leaves_indices: Vec<String>,
   pub nullifying_key: String,
   pub npk_out: Vec<String>,
   pub value_out: Vec<String>,
}

/// Full request sent from Rust to the sidecar for proof generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofRequest {
   pub public_inputs: PublicInputsRailgun,
   pub private_inputs: PrivateInputsRailgun,
   pub signature: Vec<String>,
   pub circuit_variant: String, // e.g. "01x02"
}

/// Response containing the proof (or error).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofResponse {
   pub success: bool,
   #[serde(default)]
   pub proof: Option<serde_json::Value>, // Will be mapped to SnarkProof later
   #[serde(default)]
   pub error: Option<String>,
}

impl FormattedCircuitInputsRailgun {
   /// Converts our structured inputs into the flat format the circuit expects.
   /// Mirrors exactly what `formatRailgunInputs` does in the TS engine.
   pub fn from_parts(
      public: &PublicInputsRailgun,
      private: &PrivateInputsRailgun,
      signature: &[String],
   ) -> Self {
      let path_elements_flat: Vec<String> =
         private.path_elements.iter().flat_map(|path| path.iter().cloned()).collect();

      Self {
         merkle_root: public.merkle_root.clone(),
         bound_params_hash: public.bound_params_hash.clone(),
         nullifiers: public.nullifiers.clone(),
         commitments_out: public.commitments_out.clone(),
         token: private.token_address.clone(),
         public_key: private.public_key.clone(),
         signature: signature.to_vec(),
         random_in: private.random_in.clone(),
         value_in: private.value_in.clone(),
         path_elements: path_elements_flat,
         leaves_indices: private.leaves_indices.clone(),
         nullifying_key: private.nullifying_key.clone(),
         npk_out: private.npk_out.clone(),
         value_out: private.value_out.clone(),
      }
   }
}
