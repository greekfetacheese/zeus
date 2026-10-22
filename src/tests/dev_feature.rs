//! The binary's `dev` feature must be forwarded to `zeus-core`.
//! Core gates ~57 `#[cfg(feature = "dev")]` blocks on its OWN feature. Today dropping `zeus-core/dev`
//! from the root `dev` feature fails to compile at `AutoLock::ALL_DEV` (`src/gui/ui/settings/autolock.rs`),
//! the one dev-gated symbol the GUI borrows from core: loud, not silent. That is incidental — core-only
//! gates would go dark unheard, so this test pins the invariant in the flags themselves.

#[test]
fn the_bin_dev_feature_is_forwarded_to_zeus_core() {
   assert_eq!(
      cfg!(feature = "dev"),
      zeus_core::DEV_FEATURE_ENABLED,
      "the bin's dev feature must be forwarded to zeus-core (`dev = [\"zeus-core/dev\"]`)"
   );
}
