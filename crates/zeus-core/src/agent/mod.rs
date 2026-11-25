//! The agent (MCP) domain: the grant document, its sealed store, and the bearer-token hash.
//!
//! This module owns *what may be done, and by which credential*. It never speaks HTTP and never draws a
//! frame, and it never decides a live request: it stores the document, drops the parts it does not
//! understand, and answers the two questions a caller has — may this profile act on this wallet, and does
//! it hold this global capability. Enforcement belongs where the action is authorized, not in the layer
//! that asked for it, which is why nothing here ever needs a prompt.

pub mod profile;
pub mod store;
pub mod token;

#[cfg(test)]
mod tests;

pub use profile::{
   AgentProfile, AgentProfileId, Capability, CapabilityScope, CapabilityVerb, MAX_SLIPPAGE_BPS,
   NftRef, NftScope, PrivacyBadge, ProfileMode, RiskTier, ScreenGrant, TokenRef, TokenScope,
   WalletGrant, capability,
};
pub use store::AgentProfileStore;
pub use token::VisibleToken;
