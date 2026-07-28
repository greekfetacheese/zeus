use std::{collections::HashMap, path::PathBuf};

use alloy_primitives::U256;
use ark_bn254::{Bn254, Fr};
use ark_circom::CircomReduction;
use ark_ff::BigInt;
use ark_groth16::{Groth16, prepare_verifying_key};
use ark_relations::gr1cs::SynthesisError;
use ark_std::rand::random;
use thiserror::Error;
use tracing::debug;

use crate::circuit::{
   proof::Proof,
   remote_artifact_loader::{
      ARTIFACT_MAX_INPUTS, AvailableCircuits, PrefetchReport, RemoteArtifactLoader,
      RemoteArtifactLoaderError,
   },
   witness::{CalculateWitnessError, calculate_witness},
};

#[derive(Clone)]
pub struct Groth16Prover {
   artifact_loader: RemoteArtifactLoader,
}

#[derive(Debug, Error)]
pub enum Groth16ProverError {
   #[error("Artifact loader error: {0}")]
   ArtifactLoaderError(#[from] RemoteArtifactLoaderError),
   #[error("Witness calculator error: {0}")]
   WitnessCalculatorError(#[from] CalculateWitnessError),
   #[error("Synthesis Error")]
   SynthesisError(#[from] SynthesisError),
   #[error("Proof verification failed")]
   InvalidProof,
}

impl Groth16Prover {
   pub fn new(cache_dir: Option<PathBuf>) -> Self {
      let artifact_loader = RemoteArtifactLoader::default().with_cache_dir(cache_dir);
      Groth16Prover { artifact_loader }
   }

   pub fn artifact_loader(&self) -> &RemoteArtifactLoader {
      &self.artifact_loader
   }

   /// Scan disk cache for complete transact circuits (`01x01` ..= `05x05`).
   pub fn available_circuits(&self) -> AvailableCircuits {
      self.artifact_loader.available_circuits()
   }

   /// Max nullifiers with a cached `railgun/{N}x{outputs}` set. `0` if none.
   pub fn max_cached_inputs_for_outputs(&self, outputs: usize) -> usize {
      self.artifact_loader.max_cached_inputs_for_outputs(outputs)
   }

   /// Max inputs usable for a private merge (`Nx01`). Falls back to
   /// [`ARTIFACT_MAX_INPUTS`] when nothing is cached yet (prove path can download).
   pub fn max_merge_inputs(&self) -> usize {
      let cached = self.max_cached_inputs_for_outputs(1);
      if cached >= 2 {
         cached
      } else {
         ARTIFACT_MAX_INPUTS
      }
   }

   /// Download any missing supported transact circuits into the disk cache.
   pub async fn prefetch_artifacts(&self) -> Result<PrefetchReport, Groth16ProverError> {
      Ok(self.artifact_loader.prefetch_all_circuits().await?)
   }
}

impl Groth16Prover {
   #[tracing::instrument(name = "prove_transact", skip_all)]
   pub async fn prove_transact(
      &self,
      inputs: &crate::circuit::inputs::transact_inputs::TransactCircuitInputs,
   ) -> Result<Proof, Groth16ProverError> {
      let nullifiers = inputs.nullifiers.len();
      let commitments = inputs.commitments_out.len();
      let circuit_name = format!("railgun/{:02}x{:02}", nullifiers, commitments);

      self.prove(&circuit_name, inputs.to_circuit_signals()).await
   }

   #[tracing::instrument(name = "prove_poi", skip_all)]
   pub async fn prove_poi(
      &self,
      inputs: &crate::circuit::inputs::poi_inputs::PoiCircuitInputs,
   ) -> Result<Proof, Groth16ProverError> {
      let nullifiers = inputs.nullifiers.len();
      let commitments = inputs.commitments.len();
      let circuit_name = format!("railgun/poi/{:02}x{:02}", nullifiers, commitments);

      self.prove(&circuit_name, inputs.to_circuit_signals()).await
   }

   async fn prove(
      &self,
      circuit_name: &str,
      inputs: HashMap<String, Vec<U256>>,
   ) -> Result<Proof, Groth16ProverError> {
      debug!("Loading artifacts");
      let pk = self.artifact_loader.load_proving_key(circuit_name).await?;

      let matrices = self.artifact_loader.load_matrices(circuit_name).await?;

      debug!("Calculating witness");
      let witnesses = calculate_witness(&self.artifact_loader, circuit_name, inputs).await?;
      let witnesses: Vec<Fr> = witnesses.iter().map(|x| Fr::from(BigInt::from(*x))).collect();

      debug!("Creating proof");
      let proof = Groth16::<Bn254, CircomReduction>::create_proof_with_reduction_and_matrices(
         &pk,
         random(),
         random(),
         &[matrices.a, matrices.b],
         matrices.num_instance_variables,
         matrices.num_constraints,
         &witnesses,
      )?;

      debug!("Verifying proof");
      let public_inputs = &witnesses[1..matrices.num_instance_variables];
      let pvk = prepare_verifying_key(&pk.vk);
      let verified = Groth16::<Bn254, CircomReduction>::verify_proof(&pvk, &proof, &public_inputs)?;

      if !verified {
         return Err(Groth16ProverError::InvalidProof);
      }

      debug!("Proof verified successfully");
      Ok(proof.into())
   }
}
