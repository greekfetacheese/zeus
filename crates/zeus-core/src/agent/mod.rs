//! The agent (MCP) domain: the grant document, its sealed store, the token hash, the decision a request
//! gets, and the record of what was asked.
//!
//! This module never speaks HTTP and never draws a frame. It owns *what may be done, and by which
//! credential*: the profile describes it, the store persists it, the policy decides one request against
//! it, and the audit keeps the answer. The decision is deliberately the last thing before an action runs,
//! at the app's own authorising prompt, so a caller cannot get round it by asking a different layer.

pub mod audit;
pub mod env;
pub mod intent;
pub mod policy;
pub mod profile;
pub mod store;
pub mod token;

#[cfg(test)]
mod tests;

pub use audit::{AUDIT_RING_CAPACITY, AgentAuditLog, AuditDecision, AuditEntry, BroadcastPath};
pub use env::{AgentAuthority, AgentEnv};
pub use intent::{Asset, Intent};
pub use policy::{Decision, Denial, authorise, precheck};
pub use profile::{
   AgentProfile, AgentProfileId, CAPABILITIES, Capability, CapabilityScope, CapabilityVerb,
   MAX_SLIPPAGE_BPS, NftRef, NftScope, PrivacyBadge, ProfileMode, RiskTier, ScreenGrant, TokenRef,
   TokenScope, WalletGrant, capability,
};
pub use store::AgentProfileStore;
pub use token::{
   VisibleToken, generate as generate_token, hash as hash_token, verify as verify_token,
};
