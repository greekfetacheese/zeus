//! Who is calling: one bearer token, hashed and compared, then the profile's own switches.
//!
//! The token is a credential the app owner minted and pasted into a harness, so the only thing stored
//! beside the profile is its hash. Verification is therefore a scan of the stored hashes with a
//! constant-time comparison each: there is no map from hash to profile, because a hash is not an index
//! anyone else could use.
//!
//! Binding to loopback is not an authentication boundary, so every request is authenticated — the
//! stateless revision has no session to bind anything to, which settles the question rather than
//! complicating it.

use std::fmt;
use zeus_core::agent::{AgentProfile, AgentProfileStore, verify_token};

/// One authenticated caller: the profile whose credential matched.
#[derive(Clone, Debug)]
pub struct Caller {
   pub profile: AgentProfile,
}

/// Why a request was not authenticated.
///
/// Deliberately one variant per fact the header can carry, because a 401 a user cannot act on is worse
/// than no answer: the body names which of them it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthError {
   /// No `Authorization` header at all.
   Missing,
   /// A header that is not `Bearer <token>`.
   Malformed,
   /// A well-formed token that matches no profile.
   Unknown,
   /// The profile behind the token is switched off.
   Disabled,
   /// The credential is past its expiry.
   Expired,
}

impl AuthError {
   /// The stable code the answer carries.
   pub const fn code(self) -> &'static str {
      match self {
         Self::Missing | Self::Malformed => "AUTH_MISSING",
         Self::Unknown => "AUTH_UNKNOWN_TOKEN",
         Self::Disabled => "AUTH_PROFILE_DISABLED",
         Self::Expired => "AUTH_EXPIRED",
      }
   }

   /// One sentence naming what to do about it.
   pub const fn message(self) -> &'static str {
      match self {
         Self::Missing => "This endpoint needs an Authorization header carrying a bearer token",
         Self::Malformed => "The Authorization header must be: Bearer <token>",
         Self::Unknown => "That token is not one of this machine's agent credentials",
         Self::Disabled => "The profile behind that token is switched off in Zeus",
         Self::Expired => "That credential has expired",
      }
   }

   /// Where the credential comes from.
   pub const fn hint(self) -> &'static str {
      match self {
         Self::Missing | Self::Malformed | Self::Unknown => {
            "Copy the token from Settings → Agents in Zeus, into the harness's header."
         }
         Self::Disabled => "Turn the profile back on in Settings → Agents in Zeus.",
         Self::Expired => "Rotate the token in Settings → Agents in Zeus, then update the harness.",
      }
   }
}

impl fmt::Display for AuthError {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.write_str(self.message())
   }
}

/// Resolve the `Authorization` header's token to the profile behind it.
///
/// The profile is read from the store on every request rather than cached: a switch turned off, an
/// expiry moved forward or a rotated token has to take effect on the next call, and there is no session
/// that could hold a stale copy.
pub fn authenticate(
   store: &AgentProfileStore,
   header: Option<&str>,
   now: u64,
) -> Result<Caller, AuthError> {
   let token = bearer(header)?;

   let profile = store
      .profiles()
      .into_iter()
      .find(|profile| verify_token(&profile.token_hash, token))
      .ok_or(AuthError::Unknown)?;

   if !profile.enabled {
      return Err(AuthError::Disabled);
   }

   if profile.expires_at.is_some_and(|expires| expires <= now) {
      return Err(AuthError::Expired);
   }

   Ok(Caller { profile })
}

/// The token out of `Bearer <token>`.
fn bearer(header: Option<&str>) -> Result<&str, AuthError> {
   let header = header.ok_or(AuthError::Missing)?;

   let (scheme, token) = header.split_once(' ').ok_or(AuthError::Malformed)?;

   if !scheme.eq_ignore_ascii_case("bearer") {
      return Err(AuthError::Malformed);
   }

   let token = token.trim();

   if token.is_empty() {
      return Err(AuthError::Malformed);
   }

   Ok(token)
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_core::agent::{AgentProfile, generate_token};

   fn store_with(profile: AgentProfile) -> (AgentProfileStore, AgentProfile) {
      let store = AgentProfileStore::default();
      store.insert(profile.clone());
      (store, profile)
   }

   fn profile_with_token() -> (AgentProfile, String) {
      let (token, hash) = generate_token();
      let profile = AgentProfile::new("test", hash, 0);
      (profile, token.as_str().to_owned())
   }

   #[test]
   fn a_minted_token_finds_its_profile() {
      let (profile, token) = profile_with_token();
      let (store, _) = store_with(profile.clone());

      let caller = authenticate(&store, Some(&format!("Bearer {token}")), 10).expect("the token");
      assert_eq!(caller.profile.id, profile.id);
   }

   #[test]
   fn the_scheme_is_case_insensitive_and_the_token_is_trimmed() {
      let (profile, token) = profile_with_token();
      let (store, _) = store_with(profile);

      assert!(authenticate(&store, Some(&format!("bearer {token} ")), 10).is_ok());
   }

   #[test]
   fn a_wrong_token_is_unknown_and_a_missing_one_is_missing() {
      let (profile, _) = profile_with_token();
      let (store, _) = store_with(profile);

      assert_eq!(
         authenticate(&store, None, 10).err(),
         Some(AuthError::Missing)
      );
      assert_eq!(
         authenticate(&store, Some("token"), 10).err(),
         Some(AuthError::Malformed)
      );
      assert_eq!(
         authenticate(
            &store,
            Some(&format!("Bearer zk_{}", "0".repeat(64))),
            10
         )
         .err(),
         Some(AuthError::Unknown)
      );
   }

   #[test]
   fn a_disabled_profile_and_an_expired_credential_are_told_apart() {
      let (mut profile, token) = profile_with_token();
      profile.enabled = false;
      let (store, _) = store_with(profile.clone());

      assert_eq!(
         authenticate(&store, Some(&format!("Bearer {token}")), 10).err(),
         Some(AuthError::Disabled)
      );

      profile.enabled = true;
      profile.expires_at = Some(5);
      let (store, _) = store_with(profile);

      assert_eq!(
         authenticate(&store, Some(&format!("Bearer {token}")), 10).err(),
         Some(AuthError::Expired)
      );
   }
}
