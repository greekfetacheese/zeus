//! What an agent holds, in one line, so the list of agents can be read without opening every one.
//!
//! The counts are the grant itself — how many wallets it may act on and how many switches are on —
//! rather than a guess from the labels. A read-only agent says so, because that is the difference a
//! user cares about when scanning the list for the one that can move money.

use crate::core::agent::{AgentProfile, CapabilityScope, CapabilityVerb, capability};

/// The row's sentence: how much of the wallet, and how much of the app, this agent may touch.
pub fn summary(profile: &AgentProfile) -> String {
   let mut granted_wallets = 0;
   let mut permissions = 0;
   let mut writes = 0;

   for grant in profile.wallets.values().filter(|grant| grant.enabled) {
      if grant.caps.is_empty() {
         continue;
      }

      granted_wallets += 1;
      permissions += grant.caps.len();

      writes += grant
         .caps
         .iter()
         .filter(|name| capability(name).is_some_and(|entry| entry.verb() == CapabilityVerb::Write))
         .count();
   }

   for (name, grant) in &profile.screens {
      if grant.read {
         permissions += 1;
      }

      // A screen's write is the switch that changes something, and the catalog is where the verb
      // lives: the stored grant is two flags, not two capabilities.
      if grant.write {
         permissions += 1;

         if capability(name).is_some_and(|entry| entry.scope == CapabilityScope::Global) {
            writes += 1;
         }
      }
   }

   if permissions == 0 {
      return "Nothing granted.".to_owned();
   }

   let mut sentence = format!(
      "{} {}, {} {}",
      granted_wallets,
      plural(granted_wallets, "wallet", "wallets"),
      permissions,
      plural(permissions, "permission", "permissions"),
   );

   if writes == 0 {
      sentence.push_str(", read only");
   }

   sentence
}

/// The singular form for a count of one, and the plural for every other count.
fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
   match count {
      1 => one,
      _ => many,
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::agent::TokenScope;
   use zeus_eth::alloy_primitives::Address;

   fn profile() -> AgentProfile {
      AgentProfile::new("Test", [7u8; 32], 0)
   }

   #[test]
   fn an_agent_granted_nothing_says_so() {
      assert_eq!(summary(&profile()), "Nothing granted.");
   }

   #[test]
   fn a_read_only_agent_says_so() {
      let mut profile = profile();
      let address = Address::repeat_byte(1);

      let grant = profile.wallets.entry(address).or_default();
      grant.enabled = true;
      grant.caps.insert("wallet.read.portfolio".to_owned());
      grant.caps.insert("wallet.read.history".to_owned());
      grant.tokens = TokenScope::None;

      assert_eq!(
         summary(&profile),
         "1 wallet, 2 permissions, read only"
      );
   }

   #[test]
   fn a_wallet_that_may_move_money_is_not_called_read_only() {
      let mut profile = profile();
      let address = Address::repeat_byte(2);

      let grant = profile.wallets.entry(address).or_default();
      grant.enabled = true;
      grant.caps.insert("wallet.write.send".to_owned());

      assert_eq!(summary(&profile), "1 wallet, 1 permission");
   }

   #[test]
   fn a_disabled_wallet_is_not_counted() {
      let mut profile = profile();
      let address = Address::repeat_byte(3);

      let grant = profile.wallets.entry(address).or_default();
      grant.enabled = false;
      grant.caps.insert("wallet.write.send".to_owned());

      assert_eq!(summary(&profile), "Nothing granted.");
   }
}
