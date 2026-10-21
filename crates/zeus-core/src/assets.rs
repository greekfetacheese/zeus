//! The shipped assets the embedding application hands to core.
//!
//! Core knows the FORMATS (a brotli-packed ERC-7730 registry, the pool snapshot, Railgun circuit
//! artefacts) but not the bytes: those are build artefacts of the binary that ships them, so the host
//! installs them once at startup. A build with nothing installed behaves as if no asset were shipped —
//! no pool seed, no descriptors, no offline circuits — which is the same degradation path a missing
//! asset already had.

use std::sync::OnceLock;
use zeus_railgun::EmbeddedCircuit;

pub trait EmbeddedAssets: Send + Sync + 'static {
   /// The JSON pool snapshot `PoolManager::default()` seeds itself from.
   ///
   /// `None` leaves the manager on its empty-pools default: no pool is known until the state-view
   /// sync discovers one, and the shipped snapshot is never written to `data/`.
   fn pool_data(&self) -> Option<&'static str>;

   /// The brotli-packed ERC-7730 registry (`registry.bin.br`), decoded by `clear_signing::pack`.
   ///
   /// `None` makes every registry lookup miss: typed data falls back to pretty JSON and calldata to
   /// hex, and an allowed path is reported as not being in the embedded registry.
   fn clear_signing_pack(&self) -> Option<&'static [u8]>;

   /// Railgun circuit artefacts the prover may use without downloading.
   ///
   /// An empty list leaves the prover with only its on-disk cache, and — when downloads are
   /// allowed — the remote artifact loader.
   fn railgun_circuits(&self) -> Vec<EmbeddedCircuit>;
}

static ASSETS: OnceLock<Box<dyn EmbeddedAssets>> = OnceLock::new();
static NO_ASSETS: NoAssets = NoAssets;

/// Install the process-wide assets. The first install wins; a second logs a warning.
pub fn install(assets: impl EmbeddedAssets) {
   if ASSETS.set(Box::new(assets)).is_err() {
      tracing::warn!("Zeus assets are already installed; keeping the first set");
   }
}

/// The installed assets, or the empty default when nothing is attached.
pub fn installed() -> &'static dyn EmbeddedAssets {
   match ASSETS.get() {
      Some(assets) => assets.as_ref(),
      None => &NO_ASSETS,
   }
}

/// The asset set used when `main` never installed one: every format takes its missing-asset path,
/// exactly as a build that ships no artefacts.
struct NoAssets;

impl EmbeddedAssets for NoAssets {
   fn pool_data(&self) -> Option<&'static str> {
      None
   }

   fn clear_signing_pack(&self) -> Option<&'static [u8]> {
      None
   }

   fn railgun_circuits(&self) -> Vec<EmbeddedCircuit> {
      Vec::new()
   }
}
