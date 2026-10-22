#[cfg(test)]
mod tests {
   use crate::embedded::clear_signing;
   use crate::embedded::railgun::embedded_circuits;
   use sha2::{Digest, Sha256};

   fn hex_digest(bytes: &[u8]) -> String {
      Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
   }

   #[test]
   fn embedded_pack_matches_the_pack_digest() {
      let actual = hex_digest(clear_signing::PACK_BR);
      assert_eq!(
         actual,
         clear_signing::PACK_SHA256,
         "embedded pack digest drifted; update PACK_SHA256 to {actual}"
      );

      let files = zeus_core::clear_signing::pack::decode_pack(clear_signing::PACK_BR)
         .expect("the shipped embedded pack must decode");
      for path in ["index.eip712.json", "index.calldata.json"] {
         assert!(
            files.contains_key(path),
            "embedded pack is missing {path}"
         );
      }
      assert!(
         files.len() > 1000,
         "embedded pack has only {} entries",
         files.len()
      );
   }

   #[test]
   fn embedded_circuits_match_sha256_pins() {
      for circuit in embedded_circuits() {
         zeus_railgun::verify_artifact_pin(circuit.name, "wasm.br", circuit.wasm_br)
            .unwrap_or_else(|e| panic!("{} wasm.br: {e}", circuit.name));
         zeus_railgun::verify_artifact_pin(
            circuit.name,
            "proving_key.bin.br",
            circuit.proving_key_br,
         )
         .unwrap_or_else(|e| panic!("{} proving_key.bin.br: {e}", circuit.name));
         zeus_railgun::verify_artifact_pin(
            circuit.name,
            "matrices.bin.br",
            circuit.matrices_br,
         )
         .unwrap_or_else(|e| panic!("{} matrices.bin.br: {e}", circuit.name));
      }
   }
}
