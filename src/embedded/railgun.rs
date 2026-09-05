//! Railgun circuit artifacts baked into the Zeus binary.
//!
//! Layout (repo root):
//! ```text
//! embedded/railgun/{01x01,01x02,...}/{wasm,proving_key.bin,matrices}.br
//! ```
//!
//! These are registered on [`zeus_railgun::RemoteArtifactLoader`] and count as
//! available offline **without** being copied into the on-disk cache.

use zeus_railgun::EmbeddedCircuit;

macro_rules! railgun_circuit {
   ($folder:literal) => {
      EmbeddedCircuit::new(
         concat!("railgun/", $folder),
         include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/embedded/railgun/",
            $folder,
            "/wasm.br"
         )),
         include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/embedded/railgun/",
            $folder,
            "/proving_key.bin.br"
         )),
         include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/embedded/railgun/",
            $folder,
            "/matrices.bin.br"
         )),
      )
   };
}

/// Guaranteed offline hot-set:
/// - `01x01` / `01x02` / `01x03` — common unshield (+ fee outs)
/// - `02x01` — merge / consolidate packs
pub fn embedded_circuits() -> Vec<EmbeddedCircuit> {
   vec![
      railgun_circuit!("01x01"),
      railgun_circuit!("01x02"),
      railgun_circuit!("01x03"),
      railgun_circuit!("02x01"),
   ]
}

#[cfg(test)]
mod tests {
   use super::embedded_circuits;
   use zeus_railgun::verify_artifact_pin;

   #[test]
   fn embedded_circuits_match_sha256_pins() {
      for circuit in embedded_circuits() {
         verify_artifact_pin(circuit.name, "wasm.br", circuit.wasm_br)
            .unwrap_or_else(|e| panic!("{} wasm.br: {e}", circuit.name));
         verify_artifact_pin(
            circuit.name,
            "proving_key.bin.br",
            circuit.proving_key_br,
         )
         .unwrap_or_else(|e| panic!("{} proving_key.bin.br: {e}", circuit.name));
         verify_artifact_pin(
            circuit.name,
            "matrices.bin.br",
            circuit.matrices_br,
         )
         .unwrap_or_else(|e| panic!("{} matrices.bin.br: {e}", circuit.name));
      }
   }
}
