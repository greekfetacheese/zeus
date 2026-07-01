//! Integration test for the Railgun prover sidecar.
//!
//! These tests require the JS sidecar to be built:
//!   cd crates/zeus-railgun-prover/js-sidecar && npm install
//!
//! Run with:
//!   cargo test -p zeus-railgun-prover --test sidecar_test -- --nocapture
//!
//! IMPORTANT:
//! - First run will download large proving artifacts (zkey + wasm) from IPFS
//!   into ~/.railgun/artifacts-v2.1/. This can take 1-5+ minutes.
//! - The dummy witness used here will cause the circuit to fail (expected).
//! - The goal of this test is to verify that the Rust <-> JS sidecar
//!   communication works, the sidecar starts, receives the prove command,
//!   downloads artifacts, and attempts to run the prover.

use zeus_railgun_prover::{
   FormattedCircuitInputsRailgun, PrivateInputsRailgun, ProofRequest, PublicInputsRailgun,
   RailgunProverClient,
};

#[tokio::test]
async fn test_prover_client_starts_and_proves_dummy() {
   let sidecar_path = concat!(env!("CARGO_MANIFEST_DIR"), "/js-sidecar");
   let client = RailgunProverClient::new();

   println!("[test] Starting Railgun prover sidecar...");
   match client.start(sidecar_path).await {
      Ok(c) => c,
      Err(e) => {
         eprintln!("[test] FAILED to start sidecar: {}", e);
         eprintln!(
            "[test] Make sure you ran: cd crates/zeus-railgun-prover/js-sidecar && npm install"
         );
         eprintln!("[test] And that 'node' is available.");
         panic!("Sidecar failed to start: {}", e);
      }
   };
   println!("[test] Sidecar started successfully.");

   // Build a minimal (invalid) witness.
   // Real usage will populate this from PreparedUnshield + RailgunScanner + keys.
   // Using all zeros here will cause the circuit to reject the proof,
   // but the important thing is that we reach the download + snarkjs.fullProve step.
   let public = PublicInputsRailgun {
      merkle_root: "0".to_string(),
      bound_params_hash: "0".to_string(),
      nullifiers: vec!["0".to_string()],
      commitments_out: vec!["0".to_string()],
   };

   let private = PrivateInputsRailgun {
      token_address: "0".to_string(),
      public_key: vec!["0".to_string(), "0".to_string()],
      random_in: vec!["0".to_string()],
      value_in: vec!["0".to_string()],
      path_elements: vec![vec!["0".to_string(); 16]],
      leaves_indices: vec!["0".to_string()],
      nullifying_key: "0".to_string(),
      npk_out: vec!["0".to_string()],
      value_out: vec!["0".to_string()],
   };

   let request = ProofRequest {
      public_inputs: public,
      private_inputs: private,
      signature: vec!["0".to_string(), "0".to_string(), "0".to_string()],
      circuit_variant: "01x01".to_string(),
   };

   println!(
      "[test] Requesting proof (this may take a long time on first run while downloading artifacts from IPFS)..."
   );
   let start = std::time::Instant::now();

   match client.prove_with_inputs(request).await {
      Ok(proof) => {
         let elapsed = start.elapsed();
         println!(
            "[test] prove_with_inputs returned a proof after {:.1}s",
            elapsed.as_secs_f32()
         );
         println!("[test] Proof: {:?}", proof);
         // In a real scenario with valid witness this would be a valid Groth16 proof.
         // For this dummy test we mostly expect the error path.
      }
      Err(e) => {
         let elapsed = start.elapsed();
         let err_str = e.to_string();
         println!(
            "[test] prove_with_inputs returned error after {:.1}s",
            elapsed.as_secs_f32()
         );
         println!("[test] Sidecar error: {}", err_str);

         // This is the important assertion for the smoke test:
         // We want to confirm the sidecar actually ran the prover (snarkjs or native)
         // and returned a circuit-level error instead of a comms/timeout/download failure.
         assert!(
            err_str.contains("Assert Failed") || err_str.contains("prover error"),
            "Expected a circuit assertion failure from the prover, got: {}",
            err_str
         );
         println!(
            "[test] Confirmed: sidecar executed the Groth16 prover (dummy witness correctly rejected)."
         );
      }
   }

   println!("[test] Stopping sidecar...");
   let _ = client.stop().await;
   println!("[test] Test completed.");
}

#[test]
fn test_formatted_inputs_construction() {
   let public = PublicInputsRailgun {
      merkle_root: "123".to_string(),
      bound_params_hash: "456".to_string(),
      nullifiers: vec!["1".to_string()],
      commitments_out: vec!["2".to_string()],
   };

   let private = PrivateInputsRailgun {
      token_address: "0xabc".to_string(),
      public_key: vec!["pkx".to_string(), "pky".to_string()],
      random_in: vec!["r1".to_string()],
      value_in: vec!["v1".to_string()],
      path_elements: vec![vec!["p".to_string(); 16]],
      leaves_indices: vec!["0".to_string()],
      nullifying_key: "nk".to_string(),
      npk_out: vec!["npk".to_string()],
      value_out: vec!["vo".to_string()],
   };

   let formatted = FormattedCircuitInputsRailgun::from_parts(
      &public,
      &private,
      &["sig1".to_string(), "sig2".to_string(), "sig3".to_string()],
   );

   assert_eq!(formatted.merkle_root, "123");
   assert_eq!(formatted.token, "0xabc");
   assert_eq!(formatted.path_elements.len(), 16);
   assert_eq!(formatted.signature.len(), 3);
}
