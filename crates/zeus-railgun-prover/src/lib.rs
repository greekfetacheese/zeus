//! zeus-railgun-prover
//!
//! Rust client for generating real Railgun Groth16 proofs via a Node.js sidecar.
//! Replicates the exact sidecar architecture used by zeus-waku-broadcaster.

pub mod client;

pub use client::RailgunProverClient;

/// Placeholder for proof request (will be replaced with real witness types).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProofRequest {
    pub witness: serde_json::Value,
    pub circuit: String,
}
