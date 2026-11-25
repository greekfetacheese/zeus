//! The grant document's own tests: no GUI, no chain, no HTTP, no window.

use super::profile::CAPABILITIES;
use super::profile::{
   AgentProfile, AgentProfileId, CapabilityScope, CapabilityVerb, MAX_SLIPPAGE_BPS, NftRef,
   NftScope, PrivacyBadge, ProfileMode, RiskTier, ScreenGrant, TokenRef, TokenScope, WalletGrant,
   capability,
};
use super::store::AgentProfileStore;
use super::token;
use crate::WalletStateKey;
use crate::persisted::{ExportPolicy, PersistedFile, is_allowed_rel_parts};
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::path::PathBuf;
use zeus_eth::alloy_primitives::{Address, U256, keccak256};

fn alice() -> Address {
   Address::repeat_byte(0xa1)
}

fn bob() -> Address {
   Address::repeat_byte(0xb0)
}

/// A profile with every shape the document can hold: two wallets, both scopes, both allowlists, a screen.
fn sample_profile() -> AgentProfile {
   let (_, token_hash) = token::generate();
   let mut profile = AgentProfile::new("Claude Code — laptop", token_hash, 1_700_000_000);
   profile.mode = ProfileMode::ConfirmEach;
   profile.export_dir = Some(PathBuf::from("/tmp/zeus-exports"));

   let mut caps = BTreeSet::new();
   caps.insert("wallet.read.portfolio".to_string());
   caps.insert("wallet.write.send".to_string());

   profile.wallets.insert(
      alice(),
      WalletGrant {
         enabled: true,
         caps,
         tokens: TokenScope::Allowlist {
            tokens: vec![TokenRef {
               chain: 1,
               address: Address::ZERO,
            }],
         },
         nfts: NftScope::Allowlist {
            nfts: vec![NftRef {
               chain: 1,
               collection: bob(),
               token_ids: Some(vec![U256::from(7)]),
            }],
         },
      },
   );

   profile.wallets.insert(
      bob(),
      WalletGrant {
         enabled: true,
         ..WalletGrant::default()
      },
   );

   profile.screens.insert(
      "settings.networks.read".to_string(),
      ScreenGrant {
         read: true,
         write: false,
      },
   );

   profile
}

#[test]
fn a_new_profile_grants_nothing_and_offers_the_swap_ui_maximum() {
   let (_, token_hash) = token::generate();
   let profile = AgentProfile::new("Claude Code — laptop", token_hash, 0);

   assert!(
      profile.wallets.is_empty(),
      "everything off by default: a grant starts empty"
   );
   assert!(profile.screens.is_empty());
   assert!(profile.enabled);
   assert_eq!(
      profile.mode,
      ProfileMode::Authorized,
      "the default is no prompts; a grant that prompts is the cautious opt-in"
   );
   assert_eq!(profile.max_slippage_bps, MAX_SLIPPAGE_BPS);
   assert!(profile.export_dir.is_none());
}

#[test]
fn the_grant_document_roundtrips_through_json() {
   let profile = sample_profile();
   let json = serde_json::to_vec(&profile).expect("serialize");
   let back: AgentProfile = serde_json::from_slice(&json).expect("deserialize");

   assert_eq!(back, profile);

   // The wallet map is keyed by the address on disk, so the file stays readable a year from now.
   let text = String::from_utf8(json).expect("utf8");
   assert!(
      text.to_lowercase().contains(&format!("{:#x}", alice()).to_lowercase()),
      "the wallet's address is the map key: {text}"
   );
}

#[test]
fn an_unknown_capability_is_dropped_and_never_read_as_allowed() {
   let mut profile = sample_profile();
   let grant = profile.wallets.get_mut(&alice()).expect("alice's grant");
   grant.caps.insert("wallet.write.teleport".to_string());
   grant.caps.insert("settings.general.write".to_string());

   let dropped = profile.sanitize();

   assert_eq!(
      dropped,
      ["settings.general.write", "wallet.write.teleport"],
      "a key this build does not know, or one granted for the other scope, is dropped and reported"
   );

   let grant = &profile.wallets[&alice()];
   assert!(!grant.caps.contains("wallet.write.teleport"));
   assert!(
      !grant.caps.contains("settings.general.write"),
      "a global capability written on a wallet grants nothing"
   );
   assert!(grant.caps.contains("wallet.write.send"));
   assert!(!profile.allows(alice(), "wallet.write.teleport"));
}

#[test]
fn an_unknown_screen_is_dropped() {
   let mut profile = sample_profile();
   profile.screens.insert(
      "settings.uniswap.write".to_string(),
      ScreenGrant {
         read: false,
         write: true,
      },
   );
   profile.screens.insert(
      "wallet.write.send".to_string(),
      ScreenGrant {
         read: true,
         write: true,
      },
   );

   let dropped = profile.sanitize();

   assert_eq!(
      dropped,
      ["settings.uniswap.write", "wallet.write.send"]
   );
   assert!(
      profile.allows_global("settings.networks.read"),
      "the screen that was granted through the right switch is still granted"
   );
   assert!(profile.screen("settings.uniswap.write").is_none());
   assert!(
      profile.screen("wallet.write.send").is_none(),
      "a wallet capability is not a screen"
   );
}

#[test]
fn the_slippage_bound_is_clamped_and_a_narrower_one_is_left_alone() {
   let mut profile = sample_profile();
   profile.max_slippage_bps = 50_000;

   assert!(
      profile.sanitize().is_empty(),
      "the bound is clamped, not dropped"
   );
   assert_eq!(profile.max_slippage_bps, MAX_SLIPPAGE_BPS);

   // The ceiling is the only thing clamped: a user who asked for less keeps less.
   profile.max_slippage_bps = 25;
   profile.sanitize();
   assert_eq!(profile.max_slippage_bps, 25);
}

#[test]
fn a_disabled_profile_serialises_and_allows_nothing() {
   let mut profile = sample_profile();
   profile.enabled = false;

   let json = serde_json::to_value(&profile).expect("serialize");
   assert_eq!(json["enabled"], serde_json::json!(false));

   let back: AgentProfile = serde_json::from_value(json).expect("deserialize");
   assert!(!back.enabled);
   assert!(
      !back.allows(alice(), "wallet.write.send"),
      "the kill switch is read before the capability, not instead of it"
   );
   assert!(!back.allows_global("settings.networks.read"));

   let store = AgentProfileStore::default();
   let id = back.id.clone();
   store.insert(back);
   assert!(store.enabled_profiles().is_empty());
   assert!(
      store.profile(&id).is_some(),
      "disabled is a stop, not a deletion: the grant survives being switched off"
   );
}

#[test]
fn a_global_capability_is_granted_by_its_own_switch_only() {
   let mut profile = sample_profile();

   // A write presented through the read flag, and a read through the write flag: neither is a grant.
   profile.screens.insert(
      "settings.networks.write".to_string(),
      ScreenGrant {
         read: true,
         write: false,
      },
   );
   profile.screens.insert(
      "settings.networks.read".to_string(),
      ScreenGrant {
         read: false,
         write: true,
      },
   );

   assert!(!profile.allows_global("settings.networks.write"));
   assert!(!profile.allows_global("settings.networks.read"));

   // Sanitising keeps one switch per entry, the one the catalog says grants it.
   assert!(profile.sanitize().is_empty());
   assert_eq!(
      profile.screens["settings.networks.write"],
      ScreenGrant {
         read: false,
         write: false
      }
   );
   assert_eq!(
      profile.screens["settings.networks.read"],
      ScreenGrant {
         read: false,
         write: false
      }
   );

   // ... and a grant through the right switch stands.
   profile.screens.insert(
      "settings.networks.write".to_string(),
      ScreenGrant {
         read: false,
         write: true,
      },
   );
   assert!(profile.allows_global("settings.networks.write"));
   assert!(!profile.allows_global("settings.networks.read"));
   assert!(
      !profile.allows_global("settings.networks"),
      "the family name is not a capability"
   );
   assert!(
      !profile.allows_global("wallet.write.send"),
      "a wallet capability is not a global one"
   );

   profile.enabled = false;
   assert!(!profile.allows_global("settings.networks.write"));
}

#[test]
fn the_capability_catalog_has_unique_names_and_the_documented_axes() {
   let mut names: Vec<_> = CAPABILITIES.iter().map(|cap| cap.name).collect();
   let total = names.len();
   names.sort_unstable();
   names.dedup();
   assert_eq!(
      names.len(),
      total,
      "two capabilities with one name would make a grant ambiguous"
   );

   // Exactly three capabilities carry a privacy badge, each with the sentence the page shows.
   let badged: Vec<_> = CAPABILITIES.iter().filter(|cap| cap.badge.is_some()).collect();
   assert_eq!(badged.len(), 3);
   for (name, badge) in [
      ("wallet.read.private", PrivacyBadge::Reveals),
      (
         "settings.unshield.write",
         PrivacyBadge::BreaksAnonymity,
      ),
      ("settings.networks.write", PrivacyBadge::Reveals),
   ] {
      let cap = capability(name).expect("a documented capability");
      assert_eq!(cap.badge, Some(badge), "{name}");
   }

   assert_eq!(
      capability("wallet.read.portfolio").map(|cap| cap.tier),
      Some(RiskTier::Read)
   );
   assert_eq!(
      capability("wallet.write.send").map(|cap| cap.tier),
      Some(RiskTier::Money)
   );
   assert_eq!(
      capability("wallet.write.delegate").map(|cap| cap.tier),
      Some(RiskTier::Critical)
   );
   assert_eq!(
      capability("wallet.write.send").map(|cap| cap.scope),
      Some(CapabilityScope::Wallet)
   );
   assert_eq!(
      capability("settings.networks.write").map(|cap| cap.scope),
      Some(CapabilityScope::Global)
   );
   assert!(
      capability("settings.uniswap.write").is_none(),
      "the Uniswap screen is not in the catalog: its preferences are in memory and reset every launch, \
       so a write to them would be a lie"
   );
   assert_eq!(RiskTier::Critical.level(), 3);

   // The granted switch is derived from the tier, so the catalog must not contradict itself: a
   // capability with a ~~read~~ in its name is a read, and a ~~write~~ is not.
   for entry in CAPABILITIES {
      if entry.name.contains(".read") {
         assert_eq!(
            entry.verb(),
            CapabilityVerb::Read,
            "{}",
            entry.name
         );
         assert_eq!(entry.tier, RiskTier::Read, "{}", entry.name);
      }
      if entry.name.contains(".write") {
         assert_eq!(
            entry.verb(),
            CapabilityVerb::Write,
            "{}",
            entry.name
         );
         assert_ne!(entry.tier, RiskTier::Read, "{}", entry.name);
      }
   }
}

#[test]
fn the_new_stores_are_classified_for_backup() {
   for (file, name, policy, sealed) in [
      (
         PersistedFile::AgentProfiles,
         "agent_profiles.data",
         ExportPolicy::Core,
         true,
      ),
      (
         PersistedFile::AgentAudit,
         "agent_audit.data",
         ExportPolicy::Optional,
         true,
      ),
      (
         PersistedFile::Mcp,
         "mcp.json",
         ExportPolicy::Never,
         false,
      ),
   ] {
      assert_eq!(file.name(), name);
      assert_eq!(
         file.export_policy(),
         policy,
         "{name}: export policy"
      );
      assert_eq!(
         file.is_sealed(),
         sealed,
         "{name}: sealed with the vault key"
      );
      assert_eq!(
         is_allowed_rel_parts(&[name.to_string()]),
         policy != ExportPolicy::Never,
         "{name}: only a backed-up file is a known path in an import zip"
      );
   }

   // The exact name is the whole allow-rule: a sibling is a different path.
   for sibling in [
      "agent_profiles.data.bak",
      "agent_profiles.data.quarantine-1",
      "mcp.json.bak",
   ] {
      assert!(
         !is_allowed_rel_parts(&[sibling.to_string()]),
         "{sibling} is not a known persisted path"
      );
   }
}

#[test]
fn a_token_is_256_bits_behind_a_named_prefix() {
   let (visible, hash) = token::generate();
   let text = visible.as_str();

   assert!(text.starts_with(token::TOKEN_PREFIX));
   assert_eq!(text.len(), token::TOKEN_PREFIX.len() + 64);
   assert!(text[token::TOKEN_PREFIX.len()..].chars().all(|c| c.is_ascii_hexdigit()));

   assert_eq!(hash, token::hash(text));
   assert_eq!(hash, keccak256(text.as_bytes()).0);
   assert!(token::verify(&hash, text));
}

#[test]
fn a_wrong_or_edited_token_is_rejected() {
   let (_, hash) = token::generate();
   let (other, _) = token::generate();

   assert!(!token::verify(&hash, other.as_str()));
   assert!(!token::verify(&hash, ""));
   assert!(!token::verify(&hash, token::TOKEN_PREFIX));
   assert!(
      !token::verify(
         &hash,
         &hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
      ),
      "the stored hash is not a credential"
   );

   // One character off is still a different token: a hash, not a prefix comparison, decides.
   let (visible, hash) = token::generate();
   let mut edited = visible.as_str().to_string();
   let last = if edited.ends_with('0') { '1' } else { '0' };
   edited.pop();
   edited.push(last);
   assert!(!token::verify(&hash, &edited));
}

#[test]
fn two_tokens_never_repeat() {
   let (first, first_hash) = token::generate();
   let (second, second_hash) = token::generate();

   assert_ne!(first.as_str(), second.as_str());
   assert_ne!(first_hash, second_hash);
}

#[test]
fn a_visible_token_does_not_print_itself() {
   let (visible, _) = token::generate();

   let debug = format!("{visible:?}");
   assert_eq!(debug, "VisibleToken([redacted])");
   assert!(!debug.contains(visible.as_str()));
}

#[test]
fn profile_ids_are_16_random_bytes_of_hex() {
   let first = AgentProfileId::generate();
   let second = AgentProfileId::generate();

   assert_eq!(first.as_str().len(), 32);
   assert!(first.as_str().chars().all(|c| c.is_ascii_hexdigit()));
   assert_ne!(first, second);
   assert_eq!(first.to_string(), first.as_str());
}

#[test]
fn the_store_roundtrips_through_the_sealed_codec() {
   let key = WalletStateKey::generate().expect("key");
   let store = AgentProfileStore::default();
   let profile = sample_profile();
   let id = profile.id.clone();
   store.insert(profile.clone());

   let sealed = store.encode(&key).expect("seal");
   assert!(
      !sealed
         .windows(profile.label.len())
         .any(|window| window == profile.label.as_bytes()),
      "the label — and the grants beside it — are not on disk in the clear"
   );

   let back = AgentProfileStore::decode(&key, &sealed).expect("open");
   assert_eq!(back.profile(&id), Some(profile));
   assert_eq!(back.profiles().len(), 1);
   assert_eq!(back.unreadable(), None);
}

#[test]
fn another_key_cannot_open_the_store() {
   let key = WalletStateKey::generate().expect("key");
   let other = WalletStateKey::generate().expect("other key");
   let store = AgentProfileStore::default();
   store.insert(sample_profile());

   let sealed = store.encode(&key).expect("seal");

   assert!(
      AgentProfileStore::decode(&other, &sealed).is_err(),
      "the store's own AAD and the vault key both have to match"
   );
}

#[test]
fn a_save_is_blocked_after_the_file_did_not_read() {
   let store = AgentProfileStore::default();
   assert_eq!(store.unreadable(), None);

   store.block_saves("wallet state key mismatch");
   let err = store
      .save(&WalletStateKey::generate().expect("key"))
      .expect_err("a blocked store must not write");

   assert!(
      err.to_string().contains("agent_profiles.data"),
      "{err}"
   );
   assert!(
      err.to_string().contains("wallet state key mismatch"),
      "{err}"
   );

   // A fresh load clears the block: the bytes are no longer the only copy of the grants.
   store.replace_from(&AgentProfileStore::default());
   assert_eq!(store.unreadable(), None);
}

#[test]
fn the_store_keeps_one_profile_per_id_and_sanitises_what_it_takes() {
   let store = AgentProfileStore::default();
   let mut profile = sample_profile();
   let id = profile.id.clone();

   assert!(store.insert(profile.clone()).is_none());
   profile.label = "renamed".to_string();
   let replaced = store.insert(profile).expect("the same id replaces");
   assert_eq!(replaced.label, "Claude Code — laptop");
   assert_eq!(store.profiles().len(), 1);
   assert_eq!(
      store.profile(&id).expect("profile").label,
      "renamed"
   );

   // The store is the one door: an unknown capability never survives being inserted.
   let mut loose = sample_profile();
   loose
      .wallets
      .get_mut(&alice())
      .expect("alice")
      .caps
      .insert("wallet.write.teleport".to_string());
   store.insert(loose);
   for profile in store.profiles() {
      for grant in profile.wallets.values() {
         assert!(!grant.caps.contains("wallet.write.teleport"));
      }
   }

   assert!(store.set_enabled(&id, false));
   assert_eq!(
      store.enabled_profiles().len(),
      1,
      "the sanitised profile stays enabled"
   );
   assert!(store.set_enabled(&id, true));
   assert_eq!(store.enabled_profiles().len(), 2);

   let (visible, hash) = token::generate();
   assert!(store.rotate_token(&id, hash));
   let stored = store.profile(&id).expect("profile");
   assert!(token::verify(
      &stored.token_hash,
      visible.as_str()
   ));
   assert!(!token::verify(&stored.token_hash, "zk_other"));

   store.mark_used(&id, 1_700_000_500);
   assert_eq!(
      store.profile(&id).expect("profile").last_used_at,
      Some(1_700_000_500)
   );

   assert!(store.remove(&id).is_some());
   assert!(store.remove(&id).is_none());
   assert!(!store.set_enabled(&id, true));
}

#[test]
fn a_removed_wallets_grant_is_pruned() {
   let store = AgentProfileStore::default();
   let profile = sample_profile();
   let id = profile.id.clone();
   store.insert(profile);

   let remaining: HashSet<Address> = [alice()].into_iter().collect();
   assert_eq!(store.retain_wallets(&remaining), 1);

   let stored = store.profile(&id).expect("profile");
   assert!(stored.wallets.contains_key(&alice()));
   assert!(
      !stored.wallets.contains_key(&bob()),
      "a grant must not outlive its wallet and be inherited by one derived at the address later"
   );

   // Pruning is idempotent, so calling it on every unlock costs nothing after the first.
   assert_eq!(store.retain_wallets(&remaining), 0);
}
