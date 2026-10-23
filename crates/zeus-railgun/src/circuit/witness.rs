use std::collections::HashMap;

use num_bigint::BigInt;
use ruint::aliases::U256;
use tracing::debug;
use wasmer::{Module, Store};

use crate::circuit::remote_artifact_loader::RemoteArtifactLoader;

#[derive(Debug)]
#[non_exhaustive]
pub enum CalculateWitnessError {
   ArtifactLoaderError(Box<dyn std::error::Error + Send + Sync + 'static>),
   CompilerError(wasmer::CompileError),
   CircomError(String),
}

impl std::fmt::Display for CalculateWitnessError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::ArtifactLoaderError(err) => write!(f, "Artifact loader error: {err}"),
         Self::CompilerError(err) => write!(f, "Compiler error: {err}"),
         Self::CircomError(message) => write!(f, "Circom error: {message}"),
      }
   }
}

impl std::error::Error for CalculateWitnessError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::ArtifactLoaderError(err) => Some(err.as_ref()),
         Self::CompilerError(err) => Some(err),
         Self::CircomError(_) => None,
      }
   }
}

impl From<wasmer::CompileError> for CalculateWitnessError {
   fn from(err: wasmer::CompileError) -> Self {
      Self::CompilerError(err)
   }
}

pub async fn calculate_witness(
   artifact_loader: &RemoteArtifactLoader,
   circuit_name: &str,
   inputs: HashMap<String, Vec<U256>>,
) -> Result<Vec<U256>, CalculateWitnessError> {
   let wasm_bytes = artifact_loader
      .load_wasm(circuit_name)
      .await
      .map_err(|e| CalculateWitnessError::ArtifactLoaderError(Box::new(e)))?;

   debug!("Loading WASM Module");
   let mut store = Store::default();
   let module = Module::new(&store, &wasm_bytes)?;
   let mut calculator = ark_circom::WitnessCalculator::from_module(&mut store, module)
      .map_err(|e| CalculateWitnessError::CircomError(e.to_string()))?;

   // Convert inputs from U256 to BigInt
   let inputs: HashMap<String, Vec<BigInt>> = inputs
      .into_iter()
      .map(|(k, v)| (k, v.into_iter().map(BigInt::from).collect()))
      .collect();

   // Calculate witness
   debug!("Calculating witness for circuit {}", circuit_name);
   let witness = calculator
      .calculate_witness(&mut store, inputs, true)
      .map_err(|e| CalculateWitnessError::CircomError(e.to_string()))?;
   let witness: Vec<U256> = witness.into_iter().map(U256::from).collect();

   Ok(witness)
}

#[cfg(test)]
mod error_tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_messages() {
      assert_eq!(
         CalculateWitnessError::ArtifactLoaderError(Box::new(std::io::Error::other("boom")))
            .to_string(),
         "Artifact loader error: boom"
      );
      assert_eq!(
         CalculateWitnessError::CircomError("boom".to_owned()).to_string(),
         "Circom error: boom"
      );
   }

   #[test]
   fn the_artifact_loader_variant_delegates_its_source() {
      let inner = Box::new(std::io::Error::other("boom"));
      let rendered = inner.to_string();
      assert_eq!(
         CalculateWitnessError::ArtifactLoaderError(inner)
            .source()
            .map(ToString::to_string),
         Some(rendered)
      );
   }

   #[test]
   fn circom_error_has_no_source() {
      assert!(CalculateWitnessError::CircomError("boom".to_owned()).source().is_none());
   }
}
