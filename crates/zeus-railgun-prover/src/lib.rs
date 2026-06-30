//! zeus-railgun-prover
//!
//! Rust client for generating real Railgun Groth16 proofs via a Node.js sidecar.
//! Replicates the exact sidecar architecture used by zeus-waku-broadcaster.

pub mod client;
pub mod models;

pub use client::RailgunProverClient;
pub use models::{
    FormattedCircuitInputsRailgun, PrivateInputsRailgun, ProofRequest, ProofResponse,
    PublicInputsRailgun,
};
