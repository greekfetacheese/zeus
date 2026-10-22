//! Compile-time embedded assets for the Zeus binary.
//!
//! Keep large binary blobs under the repo-root `embedded/` folder and wire them
//! here with `include_bytes!`, then hand them to core through
//! [`zeus_core::assets`] in `main`.

use zeus_railgun::EmbeddedCircuit;

pub mod clear_signing;
pub mod railgun;

pub const POOL_DATA: &str = include_str!("../../embedded/pool_data.json");

/// The assets this binary ships.
pub struct Assets;

impl zeus_core::assets::EmbeddedAssets for Assets {
   fn pool_data(&self) -> Option<&'static str> {
      Some(POOL_DATA)
   }

   fn clear_signing_pack(&self) -> Option<&'static [u8]> {
      Some(clear_signing::PACK_BR)
   }

   fn railgun_circuits(&self) -> Vec<EmbeddedCircuit> {
      railgun::embedded_circuits()
   }
}
