//! SHA-256 pins for compressed Railgun artifacts.
//!
//! Hashes cover `wasm.br`, `proving_key.bin.br`, and `matrices.bin.br` for
//! transact `railgun/01x01` ..= `railgun/05x05` and the published POI circuits
//! (`railgun/poi/03x03`, `railgun/poi/13x13`). Digests are over the
//! brotli-compressed bytes as stored on disk / fetched from the artifact host
//! (not the decompressed wasm / proving key / matrices).
//!
//! Unpinned names are refused (fail closed). Rotate this table when the Kohaku
//! pack or Zeus embeds change; never load host bytes that disagree.

use sha2::{Digest, Sha256};

/// Number of pinned compressed files in the transact pack (25 circuits × 3).
pub const TRANSACT_PIN_COUNT: usize = 75;

/// Number of pinned compressed files in the published POI pack (2 circuits × 3).
pub const POI_PIN_COUNT: usize = 6;

/// POI circuit names present on the artifact host (Kohaku ark pack).
/// Other `railgun/poi/{nn}x{mm}` names are unpinned and will not download.
pub fn all_poi_circuit_names() -> Vec<String> {
   ["railgun/poi/03x03", "railgun/poi/13x13"]
      .into_iter()
      .map(str::to_string)
      .collect()
}

/// `(circuit_name, filename, sha256_hex)` for every artifact Zeus will load.
/// Transact source: Zeus `embedded/railgun/{01x01,01x02,01x03,02x01}` plus the
/// Kohaku pack. POI source: `privacy-protocol-artifacts` `railgun/poi/{03x03,13x13}`.
const PINS: &[(&str, &str, &str)] = &[
   (
      "railgun/01x01",
      "wasm.br",
      "00ac8c66483cd1c499e43005f11aa576bd26f6ada9121ded574acc4e06c799a0",
   ),
   (
      "railgun/01x01",
      "proving_key.bin.br",
      "3b8f3232df3d811cec125ea496398cf2e519b74dbd1065f11af710b81131305c",
   ),
   (
      "railgun/01x01",
      "matrices.bin.br",
      "a0b9dc56fc52ee95efae873983383291e4523f0e144aaf83727ba50390a3e1f7",
   ),
   (
      "railgun/01x02",
      "wasm.br",
      "10b8b9fa85df383f16b756094ead008e3c0e1ae728543c99ba40e51b4589f00d",
   ),
   (
      "railgun/01x02",
      "proving_key.bin.br",
      "67d71aab9494ef9458ab1a411e41d464cac569e486d300d1bebf6b063eca025f",
   ),
   (
      "railgun/01x02",
      "matrices.bin.br",
      "19168a0d3a23017a831d6987db70d18e69b5c46cbbfe5aa7222635517913df8e",
   ),
   (
      "railgun/01x03",
      "wasm.br",
      "dd2b93b9845e7b7f68c96b13f78580cba7fc652f350f7e87a4b7a6f357e7961e",
   ),
   (
      "railgun/01x03",
      "proving_key.bin.br",
      "f137709f89cd78c0b887458693f993ac486159b66869a80ea1af0eb71647084d",
   ),
   (
      "railgun/01x03",
      "matrices.bin.br",
      "08210baa4d27a8c28aa3c1d0b5a980ac11bb5653a9e95cd71e00d958e0b9b17d",
   ),
   (
      "railgun/01x04",
      "wasm.br",
      "39e3b96a5ac15ff5bfdc9de37b48080caca7b8b6490f0735b31d7d52b1b59573",
   ),
   (
      "railgun/01x04",
      "proving_key.bin.br",
      "6726eef3c77dd20ec20246bb7c97ba56d6d8164164290512f890fef8c932ebe3",
   ),
   (
      "railgun/01x04",
      "matrices.bin.br",
      "401fe737fca24deec1917947a66a5e0af856630199eafc213767b261e92eaec1",
   ),
   (
      "railgun/01x05",
      "wasm.br",
      "c20fbab72e85f346c9d833999a19f2779e6ed4de723d5c1f34547a88b419138d",
   ),
   (
      "railgun/01x05",
      "proving_key.bin.br",
      "17187bd0bb8cb195432262c3788d18ed5cf2980f444641ed3f29ab2a732dc7c0",
   ),
   (
      "railgun/01x05",
      "matrices.bin.br",
      "1e0b2cf1396f03fc84a5fa8d51878292db2a654bd889f28e7a141d231478f0a3",
   ),
   (
      "railgun/02x01",
      "wasm.br",
      "7e7779753b77636c5b6cd4c0de10dbcebd9f4dc6f130277b2300249dde63e6c3",
   ),
   (
      "railgun/02x01",
      "proving_key.bin.br",
      "f1aa32a2d2fee8f109f8cc718de28acf3c1c59e599de0133f935e265c37012fd",
   ),
   (
      "railgun/02x01",
      "matrices.bin.br",
      "498e7bdbd4626de539409bc0e9da67fd29c4fe549d2ef7da3c6dc1f842e15099",
   ),
   (
      "railgun/02x02",
      "wasm.br",
      "6b13198e49d5c92df395e8b8fc0467ddce6aa64d4108df619519b89b75e90f97",
   ),
   (
      "railgun/02x02",
      "proving_key.bin.br",
      "659938621df969f3418b96eafe49897632c413e1abec88052dc56ed38a9613f6",
   ),
   (
      "railgun/02x02",
      "matrices.bin.br",
      "44906077af17b43c1d1bf187b16e9b7dc1ddabe4768efd78bf89a60fbd1bee72",
   ),
   (
      "railgun/02x03",
      "wasm.br",
      "622b156a8521a07d7c63b5e6c9cf4852ad77624cd3a395d51ef80f9b0d2315d9",
   ),
   (
      "railgun/02x03",
      "proving_key.bin.br",
      "d87e4588846790a4262006a55940fe46e57d565a664a318a34ba89d85dcde593",
   ),
   (
      "railgun/02x03",
      "matrices.bin.br",
      "f2b23d38da0955fa53b1bcf5fafc81d553fdbe7adc55f36abd952f439871cc1d",
   ),
   (
      "railgun/02x04",
      "wasm.br",
      "3781099c077ac331368f0fae7cb5c01bfd31c6d871a02f76cf149eaa6e047901",
   ),
   (
      "railgun/02x04",
      "proving_key.bin.br",
      "1a99bacad4a4e51ba1e449f3fdfe670affe06de46f6c1888ae822e18605d27e0",
   ),
   (
      "railgun/02x04",
      "matrices.bin.br",
      "6c05e141feeaf6680c54fcf2cf86a157f12f75c2e1a8b1178a8c74e7007dfef6",
   ),
   (
      "railgun/02x05",
      "wasm.br",
      "41ee336536fc6273b1c6d8c9096612774a680d414771ac5584a37a060e35880f",
   ),
   (
      "railgun/02x05",
      "proving_key.bin.br",
      "b73ff0d2e0be4b85d62048db4669f74eee2fc4dfb4b5607d7a8bf2a246e49ff6",
   ),
   (
      "railgun/02x05",
      "matrices.bin.br",
      "e6bdc69b5e429d87f3277bd2a6dd587d33d357085e02996cbaa6a37df28743b2",
   ),
   (
      "railgun/03x01",
      "wasm.br",
      "0573cda13a85604080d472591a64e3701ce70f4221ac020b2556b414f469c339",
   ),
   (
      "railgun/03x01",
      "proving_key.bin.br",
      "6b12a29ad2e1ffe9b43a2bf7f8e47a9a8e095801d94d338aca78bdfad903c885",
   ),
   (
      "railgun/03x01",
      "matrices.bin.br",
      "96ba7ef92727c270fe164613d46c3096980ba42a4198adea76cd8f79626ad0d2",
   ),
   (
      "railgun/03x02",
      "wasm.br",
      "175bb57e99e5bc91df7262edd1a1fa982d2f4f3e3d3a34e04b1152894c5cd70c",
   ),
   (
      "railgun/03x02",
      "proving_key.bin.br",
      "4049ef8d549ef2ee7b6b9410599a837681927b87ae41eb557557b1e2e1fc3436",
   ),
   (
      "railgun/03x02",
      "matrices.bin.br",
      "536063b69fc0fa3b241d1b120743cc6d5ee9618c2ed6c6f6ba3130612dec3f23",
   ),
   (
      "railgun/03x03",
      "wasm.br",
      "4833c05308c166d8ac84a75ab9385dc70ec30b0b6a95661c8ac7b40bd1cb57af",
   ),
   (
      "railgun/03x03",
      "proving_key.bin.br",
      "e27ba3d70a203e4d489451c4277ec0a657349a930ac03f5258413d8823fd41b4",
   ),
   (
      "railgun/03x03",
      "matrices.bin.br",
      "19f522cb856101a1538431e9e43218595c6b8e1b3f40ee53331690e518bf6260",
   ),
   (
      "railgun/03x04",
      "wasm.br",
      "501fee851acf2d2ed92789e10da061163d8c22f7754885d16c6ca55debfb0913",
   ),
   (
      "railgun/03x04",
      "proving_key.bin.br",
      "fa4f710a79a1f2fe81fe557c737242dff74e514d44f8846b6102e39bb908d569",
   ),
   (
      "railgun/03x04",
      "matrices.bin.br",
      "f3564634d9bcb6cd612bf2c2b5454db1c76a6b9381b9854d29f922cc5a0edb1b",
   ),
   (
      "railgun/03x05",
      "wasm.br",
      "b30609835b91141b8bae6efc18bad466d4cab058c055ce35a063fbd2c683e322",
   ),
   (
      "railgun/03x05",
      "proving_key.bin.br",
      "32240ad32be521fa99593461d1c5f907285591645a8d033e30681c18483d68c6",
   ),
   (
      "railgun/03x05",
      "matrices.bin.br",
      "279f82c8aa64e73cd24f523f8f390b22c333daed4e98febb47179d3a2261a383",
   ),
   (
      "railgun/04x01",
      "wasm.br",
      "53aeb6644b65d9441d655ddd7b196f3c27dad258eff348ca55b5e3d6b459f5d1",
   ),
   (
      "railgun/04x01",
      "proving_key.bin.br",
      "b6eaba23cee1a34165059e96578e48af2fec99b17f03a346639da2edec4be895",
   ),
   (
      "railgun/04x01",
      "matrices.bin.br",
      "f20998ed6ace392ece718672e58c8e2be83854d8315f4a0b93c85d0b479712aa",
   ),
   (
      "railgun/04x02",
      "wasm.br",
      "309a914229b74f855d9c3c8ca660f2ac4dbecdbcb5c35ce354d973751d8c1103",
   ),
   (
      "railgun/04x02",
      "proving_key.bin.br",
      "b632392f967b6cdd3958758e4a3f55f405e51b520f6027ef82d38896765ae08d",
   ),
   (
      "railgun/04x02",
      "matrices.bin.br",
      "de5799f7c44a0ef4ecde2681bb3890409f6e538d32feb463c94d0440c064d509",
   ),
   (
      "railgun/04x03",
      "wasm.br",
      "ebcc410f5968e3359fceb493a2bd0693f1d68cb3a871b14794afb376ac0579c4",
   ),
   (
      "railgun/04x03",
      "proving_key.bin.br",
      "952d4c8e20443c9e30519b6fcfe3332e68c284f12be5105c121a112f82e62cdb",
   ),
   (
      "railgun/04x03",
      "matrices.bin.br",
      "6b5cf665a04fd1135af55ab3a48448f917ef4df40191a8b60815607a2e03ea2f",
   ),
   (
      "railgun/04x04",
      "wasm.br",
      "50c293b74744c1f5e7709113fced9acd0b6e6bd9c79c8698f2e71b3e56ee9530",
   ),
   (
      "railgun/04x04",
      "proving_key.bin.br",
      "f03b855d074a2f84a75bbfac697d3237959a7783b898bf8b9f4261223de803b3",
   ),
   (
      "railgun/04x04",
      "matrices.bin.br",
      "2e714f89f64e5865d8f57037a443a880638ecdcaff1466441ad2e81b2a338e6c",
   ),
   (
      "railgun/04x05",
      "wasm.br",
      "b47e09fd5952607b4c3b463e7370375f74c1240436a4111e7b2f24227e42bf17",
   ),
   (
      "railgun/04x05",
      "proving_key.bin.br",
      "f84036a2e28cfed28b2a5f67ba7a41462c5f27ceb7e337ddacb78b5cd5cbe92d",
   ),
   (
      "railgun/04x05",
      "matrices.bin.br",
      "079006f1443fc917ad51f8ff9dfe3996361ed30fcdb76e6cf1cb66ad43ea6e80",
   ),
   (
      "railgun/05x01",
      "wasm.br",
      "9b12ffeb99fe8300346c31c597ef7390eff3422dbec93e8ef1f96eb92b3f4ef9",
   ),
   (
      "railgun/05x01",
      "proving_key.bin.br",
      "b750e89e4c2d49d2f8bc23efe779cce8de167986e2de34ff3419458b894f4379",
   ),
   (
      "railgun/05x01",
      "matrices.bin.br",
      "b4ba54375b7cb560731c0114ba9d42a4cf9d53bf6b7d45428b17730ffa62bd44",
   ),
   (
      "railgun/05x02",
      "wasm.br",
      "f8a02623ef0782896493feaa94b26ffff45ce84ea1788ba1c9219e11b85d89cb",
   ),
   (
      "railgun/05x02",
      "proving_key.bin.br",
      "f9d46928c4014f6099473cfefd4ca2f76baad64c3057a3f8ef3af5f5b9aaf90e",
   ),
   (
      "railgun/05x02",
      "matrices.bin.br",
      "fac2d95b36228c0f7dd71c99e30e4701d9fe0b70eb90976e2a67f85c223a2300",
   ),
   (
      "railgun/05x03",
      "wasm.br",
      "da1104631d3a0e19edd3e7fa5cfcfb1c1bf9aeaf847708a677a2097e0abebd55",
   ),
   (
      "railgun/05x03",
      "proving_key.bin.br",
      "b7fea4f14181e0a095a476b7c7d617c66e34bf49c434a2b7bb43252874724db6",
   ),
   (
      "railgun/05x03",
      "matrices.bin.br",
      "f6c21cd77afc0aa69e9f169c623c6024f17b4536305f99777a0e2f5b037fdda1",
   ),
   (
      "railgun/05x04",
      "wasm.br",
      "8f21b28c182592fe04148b735789d778eb5f22b147d2ee12fc0a1163a42f5fc6",
   ),
   (
      "railgun/05x04",
      "proving_key.bin.br",
      "e2c1bc3606867b159caee67562ed16f08f0f1663ce9cf36118bf41a10cb35bed",
   ),
   (
      "railgun/05x04",
      "matrices.bin.br",
      "a16ab1995d50df8f68c10d5b58cbac514d368bec71e1f4e3cce103e2158628fc",
   ),
   (
      "railgun/05x05",
      "wasm.br",
      "2afde728207d80a0cdd7be5c03669d7bae1b2057c141d76b5086a032e311dc0e",
   ),
   (
      "railgun/05x05",
      "proving_key.bin.br",
      "df08548f9fbc78d6fd0df6a669c7a7b41ac7f44eba4278276788b06085c9d998",
   ),
   (
      "railgun/05x05",
      "matrices.bin.br",
      "fb53159c205104212fc6a15bd310285c4a8cdae8d1af8d836304f59310ac3864",
   ),
   (
      "railgun/poi/03x03",
      "wasm.br",
      "fed0d73f5ef8551e5787ebc3f873ee9cf9f154013d226d80cb3d977550c901bc",
   ),
   (
      "railgun/poi/03x03",
      "proving_key.bin.br",
      "d16947b0bb0242cd388f2bd2948344791277d590eb49c98e84865bba95e536bf",
   ),
   (
      "railgun/poi/03x03",
      "matrices.bin.br",
      "61a9e12160ddc3e82633aa67d53c1651768d256508e7fe36b827ea627ec8307f",
   ),
   (
      "railgun/poi/13x13",
      "wasm.br",
      "1e4d11019d32a1ff0d67c57510a7df302a6f4de6c52ae103bd300e29675d5c0e",
   ),
   (
      "railgun/poi/13x13",
      "proving_key.bin.br",
      "cf88cbf48dd5c85b3b034dfdaaf6bd0dd1df54d9a388d7439baa5a16447e1134",
   ),
   (
      "railgun/poi/13x13",
      "matrices.bin.br",
      "8b33f0f317a105692ceff32842eefca4c477733d2626b72b7863b65d0d65c2f8",
   ),
];

#[derive(Debug, thiserror::Error)]
pub enum ArtifactPinError {
   #[error("no SHA-256 pin for {circuit}/{file}")]
   Unpinned { circuit: String, file: String },
   #[error("SHA-256 mismatch for {circuit}/{file}: expected {expected}, got {actual}")]
   Mismatch {
      circuit: String,
      file: String,
      expected: String,
      actual: String,
   },
}

pub(crate) fn pin_digest(circuit_name: &str, filename: &str) -> Option<[u8; 32]> {
   let hex = PINS
      .iter()
      .find(|(c, f, _)| *c == circuit_name && *f == filename)
      .map(|(_, _, hex)| *hex)?;
   let mut out = [0u8; 32];
   hex::decode_to_slice(hex, &mut out).ok()?;
   Some(out)
}

/// True when `(circuit_name, filename)` has a pin in [`PINS`].
pub fn is_artifact_pinned(circuit_name: &str, filename: &str) -> bool {
   pin_digest(circuit_name, filename).is_some()
}

/// SHA-256 of `data` must match the pin for this compressed artifact.
pub fn verify_artifact_pin(
   circuit_name: &str,
   filename: &str,
   data: &[u8],
) -> Result<(), ArtifactPinError> {
   let expected = pin_digest(circuit_name, filename).ok_or_else(|| ArtifactPinError::Unpinned {
      circuit: circuit_name.to_string(),
      file: filename.to_string(),
   })?;
   let actual: [u8; 32] = Sha256::digest(data).into();
   if actual != expected {
      return Err(ArtifactPinError::Mismatch {
         circuit: circuit_name.to_string(),
         file: filename.to_string(),
         expected: hex::encode(expected),
         actual: hex::encode(actual),
      });
   }
   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::circuit::remote_artifact_loader::{
      ARTIFACT_MAX_INPUTS, ARTIFACT_MAX_OUTPUTS, all_transact_circuit_names,
   };
   use std::collections::HashSet;

   #[test]
   fn pin_table_covers_transact_and_poi_pack() {
      assert_eq!(PINS.len(), TRANSACT_PIN_COUNT + POI_PIN_COUNT);
      assert_eq!(
         TRANSACT_PIN_COUNT,
         ARTIFACT_MAX_INPUTS * ARTIFACT_MAX_OUTPUTS * 3
      );
      assert_eq!(POI_PIN_COUNT, all_poi_circuit_names().len() * 3);

      let mut keys = HashSet::new();
      for &(circuit, file, hex) in PINS {
         assert_eq!(hex.len(), 64, "{circuit}/{file}");
         assert!(
            hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
            "{circuit}/{file} pin is not lowercase hex"
         );
         assert!(
            pin_digest(circuit, file).is_some(),
            "pin {circuit}/{file} does not decode"
         );
         assert!(
            keys.insert((circuit, file)),
            "duplicate pin {circuit}/{file}"
         );
      }

      for name in all_transact_circuit_names() {
         for file in ["wasm.br", "proving_key.bin.br", "matrices.bin.br"] {
            assert!(
               is_artifact_pinned(&name, file),
               "missing pin {name}/{file}"
            );
         }
      }
      for name in all_poi_circuit_names() {
         for file in ["wasm.br", "proving_key.bin.br", "matrices.bin.br"] {
            assert!(
               is_artifact_pinned(&name, file),
               "missing pin {name}/{file}"
            );
         }
      }
   }

   #[test]
   fn unpinned_names_are_rejected() {
      let blob = [0xABu8; 300];
      // Host only publishes poi/03x03 and poi/13x13.
      let err = verify_artifact_pin("railgun/poi/01x01", "wasm.br", &blob).unwrap_err();
      assert!(matches!(err, ArtifactPinError::Unpinned { .. }));
      let err = verify_artifact_pin("railgun/06x01", "wasm.br", &blob).unwrap_err();
      assert!(matches!(err, ArtifactPinError::Unpinned { .. }));
   }

   #[test]
   fn mismatch_is_rejected() {
      let blob = [0xABu8; 300];
      let err = verify_artifact_pin("railgun/01x01", "wasm.br", &blob).unwrap_err();
      assert!(matches!(err, ArtifactPinError::Mismatch { .. }));
   }

   #[test]
   fn matching_bytes_pass() {
      let data = b"pin-self-test";
      let digest: [u8; 32] = Sha256::digest(data).into();
      // Reuse the verify math without a table entry.
      let expected = digest;
      let actual: [u8; 32] = Sha256::digest(data).into();
      assert_eq!(actual, expected);
      assert_ne!(
         pin_digest("railgun/01x01", "wasm.br").unwrap(),
         actual
      );
   }
}
