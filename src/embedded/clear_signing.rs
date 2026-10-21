//! Brotli-packed ERC-7730 registry snapshot.
//!
//! Layout (repo root):
//! ```text
//! embedded/clear_signing/registry.bin.br
//! ```
//!
//! The archive layout and its decoder live in `zeus_core::clear_signing::pack`; this module only
//! carries the shipped bytes and the digest that pins them.

pub(crate) const PACK_BR: &[u8] = include_bytes!("../../embedded/clear_signing/registry.bin.br");
/// SHA-256 of the raw brotli pack bytes (`registry.bin.br`), so a rebuilt pack must change this line deliberately.
#[cfg(test)]
pub(crate) const PACK_SHA256: &str =
   "28867e6cc735d33d8694daf49973a2299a9f33e330bbbdceb08dba73cb1ace54";
