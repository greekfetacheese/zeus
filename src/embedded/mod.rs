//! Compile-time embedded assets for the Zeus binary.
//!
//! Keep large binary blobs under the repo-root `embedded/` folder and wire them
//! here with `include_bytes!`.

pub mod railgun;
