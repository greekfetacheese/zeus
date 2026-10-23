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
      AvailableCircuits, EmbeddedCircuit, PrefetchReport, RemoteArtifactLoader,
      RemoteArtifactLoaderError,
   },
   witness::{CalculateWitnessError, calculate_witness},
};

#[derive(Clone)]
pub struct Groth16Prover {
   artifact_loader: RemoteArtifactLoader,
}

#[derive(Debug, Error)]
#[non_exhaustive]
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
   /// `base_url` is the artifact host root (its `.../artifacts` prefix). The host
   /// app supplies it — Zeus passes `ZeusUrl::RailgunCircuitArtifacts` — so the
   /// crate carries no default host of its own.
   pub fn new(base_url: &str, cache_dir: Option<PathBuf>) -> Self {
      let artifact_loader = RemoteArtifactLoader::new(base_url, cache_dir);
      Groth16Prover { artifact_loader }
   }

   /// Attach host-binary circuit embeds.
   pub fn with_embedded_circuits(
      mut self,
      circuits: impl IntoIterator<Item = EmbeddedCircuit>,
   ) -> Self {
      self.artifact_loader = self.artifact_loader.with_embedded_circuits(circuits);
      self
   }

   pub fn with_allow_download(self, allow: bool) -> Self {
      self.artifact_loader.set_allow_download(allow);
      self
   }

   pub fn set_allow_download(&self, allow: bool) {
      self.artifact_loader.set_allow_download(allow);
   }

   pub fn artifact_loader(&self) -> &RemoteArtifactLoader {
      &self.artifact_loader
   }

   /// Offline-available circuits (binary embeds ∪ disk cache).
   pub fn available_circuits(&self) -> AvailableCircuits {
      self.artifact_loader.available_circuits()
   }

   /// Max nullifiers with an available `railgun/{N}x{outputs}` set. `0` if none.
   pub fn max_cached_inputs_for_outputs(&self, outputs: usize) -> usize {
      self.artifact_loader.max_cached_inputs_for_outputs(outputs)
   }

   /// Max inputs usable for a private merge (`Nx01`) based on the
   /// available circuits that currently the prover has.
   pub fn max_merge_inputs(&self) -> usize {
      let cached = self.max_cached_inputs_for_outputs(1);
      debug!("max_cached_inputs_for_outputs(1)={}", cached);
      cached
   }

   /// Prefetch non-embedded circuits into the disk cache.
   /// Embedded circuits are reported but never written to disk.
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
      let verified = Groth16::<Bn254, CircomReduction>::verify_proof(&pvk, &proof, public_inputs)?;

      if !verified {
         return Err(Groth16ProverError::InvalidProof);
      }

      debug!("Proof verified successfully");
      Ok(proof.into())
   }
}

#[cfg(test)]
mod error_tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_messages() {
      assert_eq!(
         Groth16ProverError::ArtifactLoaderError(RemoteArtifactLoaderError::NoCacheDir).to_string(),
         "Artifact loader error: No on-disk cache directory configured"
      );
      assert_eq!(
         Groth16ProverError::WitnessCalculatorError(CalculateWitnessError::CircomError(
            "boom".to_owned()
         ))
         .to_string(),
         "Witness calculator error: Circom error: boom"
      );
      assert_eq!(
         Groth16ProverError::SynthesisError(SynthesisError::DivisionByZero).to_string(),
         "Synthesis Error"
      );
      assert_eq!(
         Groth16ProverError::InvalidProof.to_string(),
         "Proof verification failed"
      );
   }

   #[test]
   fn the_artifact_loader_variant_delegates_its_source() {
      let inner = RemoteArtifactLoaderError::NoCacheDir;
      let rendered = inner.to_string();
      assert_eq!(
         Groth16ProverError::ArtifactLoaderError(inner).source().map(ToString::to_string),
         Some(rendered)
      );
   }

   #[test]
   fn the_witness_calculator_variant_delegates_its_source() {
      let inner = CalculateWitnessError::CircomError("boom".to_owned());
      let rendered = inner.to_string();
      assert_eq!(
         Groth16ProverError::WitnessCalculatorError(inner)
            .source()
            .map(ToString::to_string),
         Some(rendered)
      );
   }

   #[test]
   fn the_synthesis_variant_delegates_its_source() {
      let inner = SynthesisError::Unsatisfiable;
      let rendered = inner.to_string();
      assert_eq!(
         Groth16ProverError::SynthesisError(inner).source().map(ToString::to_string),
         Some(rendered)
      );
   }

   #[test]
   fn invalid_proof_has_no_source() {
      assert!(Groth16ProverError::InvalidProof.source().is_none());
   }
}
