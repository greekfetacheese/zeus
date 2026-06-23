#![doc = include_str!("../readme.md")]

pub mod error;
pub mod path;
pub mod primitives;
pub mod xpriv;

pub use path::*;
pub use primitives::*;
pub use xpriv::{SecureXPriv, root_from_seed};
