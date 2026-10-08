//! Embed the Zeus icon (and package metadata) in the Windows executable.
//!
//! Only a Windows *target* carries icon resources, so this is a no-op everywhere else. The
//! guard reads `CARGO_CFG_TARGET_OS` at run time because `build.rs` is compiled for and run
//! on the *host*: `#[cfg(target_os = "windows")]` here would test the host, not the target,
//! and so would be silently wrong when cross-compiling from Linux.

fn main() {
   println!("cargo:rerun-if-changed=src/assets/brand/zeus.ico");

   if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
      return;
   }

   let mut resource = winresource::WindowsResource::new();
   resource.set_icon("src/assets/brand/zeus.ico");
   resource.compile().expect("failed to embed the Windows icon resource");
}
