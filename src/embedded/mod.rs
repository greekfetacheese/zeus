//! Compile-time embedded assets for the Zeus binary.
//!
//! Keep large binary blobs under the repo-root `embedded/` folder and wire them
//! here with `include_bytes!`.

pub mod railgun;

pub const TOKEN_DATA: &[u8] = include_bytes!("../../embedded/token_data.data");
pub const POOL_DATA: &str = include_str!("../../embedded/pool_data.json");
