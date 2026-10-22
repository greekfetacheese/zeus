//! Decoded transaction events and per-protocol param types.
//!
//! - [`DecodedEvent`]: stable serializable user-facing event kind
//! - [`kinds`]: params + log decoders per protocol/family
//! - [`decode`]: registry / topic0 dispatch used by `TransactionAnalysis::new`

pub mod decode;
pub mod kinds;

mod accessors;
mod decoded;

pub use decoded::DecodedEvent;
pub use kinds::*;
