//! Who a flow is running for.

use super::profile::AgentProfileId;

/// Whose action a flow is performing: the user at the keyboard, or an agent acting under a grant.
///
/// Flows take this rather than the authority alone, so a later slice can carry more of a request's
/// provenance — the client it came from, the audit id it will be recorded under — without touching
/// every signature again. The default is the user, so a flow handed `AgentEnv::user()` behaves
/// exactly as it did before agents existed.
#[derive(Clone, Debug, Default)]
pub struct AgentEnv {
   /// The grant standing in for the click. `None` is the user.
   pub authority: Option<AgentAuthority>,
}

impl AgentEnv {
   /// The user, at the keyboard.
   pub fn user() -> Self {
      Self::default()
   }

   /// An agent acting under `profile`'s grant.
   pub fn agent(profile: AgentProfileId) -> Self {
      Self {
         authority: Some(AgentAuthority { profile }),
      }
   }
}

/// The grant an action runs under: the profile whose switches decide it.
///
/// The id rather than the profile itself. What may be done is read when the action runs, not when it
/// was asked for, so a switch turned off, a wallet revoked or the whole profile deleted between the
/// two takes effect instead of travelling along with the request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentAuthority {
   profile: AgentProfileId,
}

impl AgentAuthority {
   /// The profile that granted this action.
   pub fn profile(&self) -> &AgentProfileId {
      &self.profile
   }
}
