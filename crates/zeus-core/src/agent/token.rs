//! The agent bearer token: generated once, shown once, stored only as a hash.
//!
//! 256 bits from the OS leaves nothing to brute-force, so there is no KDF here: the stored form is
//! `keccak256(token)` and verification is one constant-time comparison. That is the whole design. The
//! shape is the wallet connector's pairing token (`src/connector.rs:30-47`); what is new is that the
//! visible form is a named type, so a log line cannot print it by accident.

use crate::wallet_state_key::ct_eq;
use rand::RngCore;
use std::fmt;
use zeus_eth::alloy_primitives::{hex, keccak256};

/// The prefix every token carries, so a credential pasted into the wrong place is recognisable.
pub const TOKEN_PREFIX: &str = "zk_";

/// 32 bytes: nothing to brute-force, and nothing a KDF would improve.
const TOKEN_BYTES: usize = 32;

/// A freshly minted token, alive only long enough to be shown to the user once.
///
/// There is no accessor that hands out the owned string and no `Display`: the only ways out are
/// [`VisibleToken::as_str`] (to put it in a clipboard or a text field) and redacted `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct VisibleToken(String);

impl VisibleToken {
   pub fn as_str(&self) -> &str {
      &self.0
   }
}

impl fmt::Debug for VisibleToken {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.write_str("VisibleToken([redacted])")
   }
}

/// Mint a token and the hash to store beside the profile.
///
/// The caller shows the token once and drops it; the hash is the only thing that survives.
pub fn generate() -> (VisibleToken, [u8; 32]) {
   let mut bytes = [0u8; TOKEN_BYTES];
   rand::rngs::OsRng.fill_bytes(&mut bytes);
   let visible = format!("{TOKEN_PREFIX}{}", hex::encode(bytes));
   let hash = hash(&visible);
   (VisibleToken(visible), hash)
}

/// `keccak256` of the token exactly as a client sends it, prefix included.
pub fn hash(token: &str) -> [u8; 32] {
   keccak256(token.as_bytes()).0
}

/// Whether `token` is the credential behind `token_hash`.
///
/// Compared in constant time: a length check plus an XOR fold with no early exit.
pub fn verify(token_hash: &[u8; 32], token: &str) -> bool {
   ct_eq(token_hash, &hash(token))
}
