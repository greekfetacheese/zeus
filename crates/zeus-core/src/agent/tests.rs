//! The grant document's own tests: no GUI, no chain, no HTTP, no window.

use super::audit::{AUDIT_RING_CAPACITY, AgentAuditLog, AuditDecision, AuditEntry};
use super::intent::{Asset, Intent};
use super::policy::{Decision, Denial, authorise, precheck};
use super::profile::CAPABILITIES;
use super::profile::{
   AgentProfile, AgentProfileId, CapabilityScope, CapabilityVerb, MAX_SLIPPAGE_BPS, NftRef,
   NftScope, PrivacyBadge, ProfileMode, RiskTier, ScreenGrant, TokenRef, TokenScope, WalletGrant,
   capability,
};
use super::store::AgentProfileStore;
use super::token;
use crate::WalletStateKey;
use crate::ZeusCtx;
use crate::persisted::{ExportPolicy, PersistedFile, is_allowed_rel_parts};
use crate::sim::balance_diff::{BalanceChange, NftBalanceChange, native_change};
use crate::tx::TransactionAnalysis;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::path::PathBuf;
use zeus_eth::alloy_primitives::{Address, U256, keccak256};
use zeus_eth::currency::{Currency, ERC20Token};
use zeus_eth::nft::NftStandard;
use zeus_eth::utils::NumericValue;

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
   // capability with a `read` in its name is a read, and a `write` is not.
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

/// An unlocked context. A fresh one starts with the vault closed, which is the point of the gate.
fn unlocked() -> ZeusCtx {
   let ctx = ZeusCtx::new(crate::platform::runtime::test_handle());
   ctx.write(|ctx| {
      ctx.vault_unlocked = true;
      ctx.locked = false;
   });
   ctx
}

/// The same context after the auto-lock timer fired: the server is still up, every action is refused.
fn locked() -> ZeusCtx {
   let ctx = ZeusCtx::new(crate::platform::runtime::test_handle());
   ctx.write(|ctx| {
      ctx.vault_unlocked = true;
      ctx.locked = true;
   });
   ctx
}

/// A token no allowlist in this file names.
fn usdc() -> ERC20Token {
   ERC20Token::from_components(
      1,
      Address::repeat_byte(0x5c),
      "USDC",
      "USD Coin",
      6,
      U256::from(1_000_000_000_000u64),
   )
}

/// Sending the native token from a wallet, which alice's grant does allow.
fn send_native(wallet: Address) -> Intent {
   Intent::AssetMove {
      capability: "wallet.write.send",
      wallet,
      asset: Asset::Token(TokenRef {
         chain: 1,
         address: Address::ZERO,
      }),
   }
}

/// An analysis of a transaction from `sender` on `chain` that moves nothing yet.
fn simulated(sender: Address, chain: u64) -> TransactionAnalysis {
   let mut analysis = TransactionAnalysis::default();
   analysis.sender = sender;
   analysis.chain = chain;
   analysis
}

fn leaving(currency: Currency) -> BalanceChange {
   BalanceChange::from_wei(
      currency,
      NumericValue::default(),
      U256::from(50_000_000u64),
      U256::from(0u64),
   )
   .expect("a balance that moved")
}

fn receiving(currency: Currency) -> BalanceChange {
   BalanceChange::from_wei(
      currency,
      NumericValue::default(),
      U256::from(0u64),
      U256::from(50_000_000u64),
   )
   .expect("a balance that moved")
}

fn nft_row(collection: Address, token_id: u64, before: u64, after: u64) -> NftBalanceChange {
   NftBalanceChange::new(
      collection,
      U256::from(token_id),
      NftStandard::Erc721,
      U256::from(before),
      U256::from(after),
   )
   .expect("a row for an id that moved")
}

#[test]
fn a_read_needs_its_own_grant_and_the_refusal_names_the_switch() {
   let profile = sample_profile();

   // bob's grant exists but carries no capability, which is what a grant looks like before any switch.
   let intent = Intent::WalletRead {
      capability: "wallet.read.portfolio",
      wallet: bob(),
   };

   let Decision::Denied(denial) = precheck(&unlocked(), &profile, &intent) else {
      panic!("a read needs its own grant")
   };

   assert_eq!(denial.code(), "NOT_GRANTED");
   assert!(
      denial.to_string().contains("wallet.read.portfolio"),
      "the refusal names the switch to turn on: {denial}"
   );
}

#[test]
fn a_granted_read_never_prompts_even_in_confirm_each() {
   let profile = sample_profile();
   assert_eq!(profile.mode, ProfileMode::ConfirmEach);

   let intent = Intent::WalletRead {
      capability: "wallet.read.portfolio",
      wallet: alice(),
   };

   assert_eq!(
      precheck(&unlocked(), &profile, &intent),
      Decision::Allowed { prompt: false },
      "a read changes nothing, so there is nothing to confirm"
   );
}

#[test]
fn a_granted_write_runs_without_a_prompt_and_prompts_in_confirm_each() {
   let mut profile = sample_profile();
   let intent = send_native(alice());

   assert_eq!(
      precheck(&unlocked(), &profile, &intent),
      Decision::Allowed { prompt: true },
      "the cautious mode asks before a state change"
   );

   profile.mode = ProfileMode::Authorized;
   assert_eq!(
      precheck(&unlocked(), &profile, &intent),
      Decision::Allowed { prompt: false },
      "the grant is the authorisation"
   );
}

#[test]
fn a_token_outside_the_allowlist_is_refused_with_the_asset_named() {
   let profile = sample_profile();
   let intent = Intent::AssetMove {
      capability: "wallet.write.send",
      wallet: alice(),
      asset: Asset::Token(TokenRef {
         chain: 1,
         address: usdc().address,
      }),
   };

   let Decision::Denied(denial) = precheck(&unlocked(), &profile, &intent) else {
      panic!("the allowlist is the whole bound on what may leave")
   };

   assert_eq!(denial.code(), "TOKEN_NOT_ALLOWED");
   assert!(
      denial.to_string().contains(&format!("{:#x}", usdc().address)),
      "{denial}"
   );
}

#[test]
fn the_native_token_must_be_listed_and_is_not_implied() {
   let mut profile = sample_profile();
   profile.wallets.get_mut(&alice()).expect("alice").tokens = TokenScope::Allowlist {
      tokens: vec![TokenRef {
         chain: 1,
         address: usdc().address,
      }],
   };

   assert!(
      matches!(
         precheck(&unlocked(), &profile, &send_native(alice())),
         Decision::Denied(Denial::TokenNotAllowed { .. })
      ),
      "everything except the native token is not a grant anyone means to make"
   );
}

#[test]
fn a_token_scope_of_none_admits_nothing_and_all_admits_everything() {
   let mut profile = sample_profile();
   let intent = Intent::AssetMove {
      capability: "wallet.write.send",
      wallet: alice(),
      asset: Asset::Token(TokenRef {
         chain: 1,
         address: usdc().address,
      }),
   };

   profile.wallets.get_mut(&alice()).expect("alice").tokens = TokenScope::All;
   assert_eq!(
      precheck(&unlocked(), &profile, &intent),
      Decision::Allowed { prompt: true }
   );

   profile.wallets.get_mut(&alice()).expect("alice").tokens = TokenScope::None;
   assert!(
      matches!(
         precheck(&unlocked(), &profile, &intent),
         Decision::Denied(Denial::TokenNotAllowed { .. })
      ),
      "the default is that nothing may leave"
   );
}

#[test]
fn an_nft_outside_the_allowlist_is_refused_and_a_collection_without_ids_is_not() {
   let mut profile = sample_profile();
   profile
      .wallets
      .get_mut(&alice())
      .expect("alice")
      .caps
      .insert("wallet.write.nft_send".to_string());

   let intent = |token_id: u64| Intent::AssetMove {
      capability: "wallet.write.nft_send",
      wallet: alice(),
      asset: Asset::Nft {
         chain: 1,
         collection: bob(),
         token_id: U256::from(token_id),
      },
   };

   assert_eq!(
      precheck(&unlocked(), &profile, &intent(7)),
      Decision::Allowed { prompt: true },
      "the id the grant names"
   );

   let Decision::Denied(denial) = precheck(&unlocked(), &profile, &intent(8)) else {
      panic!("an id outside the list is outside the grant")
   };
   assert_eq!(denial.code(), "NFT_NOT_ALLOWED");

   // An open edition cannot be enumerated in a grant, so a collection with no ids covers every id in it.
   profile.wallets.get_mut(&alice()).expect("alice").nfts = NftScope::Allowlist {
      nfts: vec![NftRef {
         chain: 1,
         collection: bob(),
         token_ids: None,
      }],
   };
   assert_eq!(
      precheck(&unlocked(), &profile, &intent(8)),
      Decision::Allowed { prompt: true }
   );
}

#[test]
fn a_locked_vault_is_refused_before_the_grant_is_read() {
   let profile = sample_profile();
   let intent = Intent::WalletRead {
      capability: "wallet.read.portfolio",
      wallet: alice(),
   };

   assert_eq!(
      precheck(&unlocked(), &profile, &intent),
      Decision::Allowed { prompt: false }
   );
   assert_eq!(
      precheck(&locked(), &profile, &intent),
      Decision::Denied(Denial::VaultLocked),
      "a locked vault is refused before anything else is looked at"
   );
}

#[test]
fn a_disabled_profile_grants_nothing() {
   let mut profile = sample_profile();
   profile.enabled = false;

   let intent = Intent::WalletRead {
      capability: "wallet.read.portfolio",
      wallet: alice(),
   };

   assert_eq!(
      precheck(&unlocked(), &profile, &intent),
      Decision::Denied(Denial::NotGranted {
         capability: "wallet.read.portfolio".to_string(),
         wallet: Some(alice()),
      }),
      "the profile's own switch is a hard stop, not a filter"
   );
}

#[test]
fn an_import_is_handed_back_to_the_human_only_when_it_is_granted() {
   let mut profile = sample_profile();
   let intent = Intent::NeedsHuman {
      capability: "settings.wallets.import",
   };

   assert!(
      matches!(
         precheck(&unlocked(), &profile, &intent),
         Decision::Denied(_)
      ),
      "an ungranted import gets a refusal, not a form"
   );

   profile.screens.insert(
      "settings.wallets.import".to_string(),
      ScreenGrant {
         read: false,
         write: true,
      },
   );

   let Decision::NeedsUser { reason } = precheck(&unlocked(), &profile, &intent) else {
      panic!("the key is typed into Zeus, so the agent opens the form and stops")
   };
   assert!(reason.contains("typed into Zeus"), "{reason}");
}

#[test]
fn an_export_runs_to_the_granted_folder_and_asks_for_one_when_there_is_none() {
   let mut profile = sample_profile();
   profile.screens.insert(
      "settings.data.write".to_string(),
      ScreenGrant {
         read: false,
         write: true,
      },
   );

   let intent = Intent::Export {
      capability: "settings.data.write",
   };

   assert_eq!(
      precheck(&unlocked(), &profile, &intent),
      Decision::Allowed { prompt: false },
      "the destination was chosen when the grant was made, so there is nothing left to confirm"
   );

   profile.export_dir = None;
   assert!(
      matches!(
         precheck(&unlocked(), &profile, &intent),
         Decision::NeedsUser { .. }
      ),
      "a folder is a path only the human can pick"
   );
}

#[test]
fn a_capability_this_build_does_not_know_is_never_granted() {
   let profile = sample_profile();
   let intent = Intent::GlobalRead {
      capability: "settings.telepathy.read",
   };

   assert!(matches!(
      precheck(&unlocked(), &profile, &intent),
      Decision::Denied(Denial::NotGranted { .. })
   ));
}

#[test]
fn a_wallet_capability_in_a_global_intent_grants_nothing() {
   let profile = sample_profile();
   let intent = Intent::GlobalRead {
      capability: "wallet.read.portfolio",
   };

   assert!(
      matches!(
         precheck(&unlocked(), &profile, &intent),
         Decision::Denied(Denial::NotGranted { .. })
      ),
      "the scope a grant is written in is part of the grant"
   );
}

#[test]
fn a_swap_routed_through_an_unlisted_token_is_refused_from_the_diff() {
   let mut profile = sample_profile();
   profile
      .wallets
      .get_mut(&alice())
      .expect("alice")
      .caps
      .insert("wallet.write.swap".to_string());

   // The request declares the native token in, which the grant does allow ...
   let intent = Intent::AssetMove {
      capability: "wallet.write.swap",
      wallet: alice(),
      asset: Asset::Token(TokenRef {
         chain: 1,
         address: Address::ZERO,
      }),
   };
   assert_eq!(
      precheck(&unlocked(), &profile, &intent),
      Decision::Allowed { prompt: true }
   );

   // ... but the route spends USDC it never named, and the simulation is what sees that.
   let mut analysis = simulated(alice(), 1);
   analysis.balance_diff.tokens.push(leaving(Currency::from(usdc())));

   let Decision::Denied(denial) = authorise(&unlocked(), &profile, &analysis) else {
      panic!("an asset the intent never named must not leave the wallet")
   };
   assert_eq!(denial.code(), "TOKEN_NOT_ALLOWED");
   assert!(
      denial.to_string().contains(&format!("{:#x}", usdc().address)),
      "{denial}"
   );
}

#[test]
fn authorise_refuses_a_wallet_with_no_grant_and_a_wallet_switched_off() {
   let mut profile = sample_profile();
   let unknown = Address::repeat_byte(0xc0);

   assert_eq!(
      authorise(&unlocked(), &profile, &simulated(unknown, 1)),
      Decision::Denied(Denial::WalletNotGranted { wallet: unknown })
   );

   profile.wallets.get_mut(&bob()).expect("bob").enabled = false;
   assert_eq!(
      authorise(&unlocked(), &profile, &simulated(bob(), 1)),
      Decision::Denied(Denial::WalletNotGranted { wallet: bob() })
   );
}

#[test]
fn authorise_is_refused_while_the_vault_is_locked() {
   let profile = sample_profile();

   assert_eq!(
      authorise(&locked(), &profile, &simulated(alice(), 1)),
      Decision::Denied(Denial::VaultLocked)
   );
}

#[test]
fn authorise_prompts_only_in_confirm_each() {
   let mut profile = sample_profile();

   assert_eq!(
      authorise(&unlocked(), &profile, &simulated(alice(), 1)),
      Decision::Allowed { prompt: true }
   );

   profile.mode = ProfileMode::Authorized;
   assert_eq!(
      authorise(&unlocked(), &profile, &simulated(alice(), 1)),
      Decision::Allowed { prompt: false }
   );
}

#[test]
fn authorise_lets_an_inflow_through() {
   let profile = sample_profile();
   let mut analysis = simulated(alice(), 1);
   analysis.balance_diff.tokens.push(receiving(Currency::from(usdc())));

   assert!(
      matches!(
         authorise(&unlocked(), &profile, &analysis),
         Decision::Allowed { .. }
      ),
      "a token arriving is not leaving, so no allowlist can refuse it"
   );
}

#[test]
fn authorise_reads_the_declared_native_value_and_ignores_the_fee() {
   let mut profile = sample_profile();
   profile.wallets.get_mut(&alice()).expect("alice").tokens = TokenScope::Allowlist {
      tokens: vec![TokenRef {
         chain: 1,
         address: usdc().address,
      }],
   };

   // A declared value is native moving on purpose, so it has to be in the allowlist.
   let mut spending = simulated(alice(), 1);
   spending.value = U256::from(1_000_000_000_000_000_000u64);
   assert_eq!(
      authorise(&unlocked(), &profile, &spending),
      Decision::Denied(Denial::TokenNotAllowed {
         wallet: alice(),
         token: TokenRef {
            chain: 1,
            address: Address::ZERO,
         },
      })
   );

   // The fee alone is not: an ERC-20 send pays gas in native without ever moving native on purpose.
   let mut fee_only = simulated(alice(), 1);
   fee_only.balance_diff.native = native_change(
      1,
      NumericValue::default(),
      U256::from(10u64),
      U256::from(9u64),
   );
   assert!(
      matches!(
         authorise(&unlocked(), &profile, &fee_only),
         Decision::Allowed { .. }
      ),
      "the gas fee must not demand the native token in every allowlist"
   );
}

#[test]
fn authorise_refuses_an_nft_the_allowlist_does_not_hold() {
   let profile = sample_profile();
   let mut analysis = simulated(alice(), 1);

   analysis.balance_diff.nfts.push(nft_row(bob(), 7, 1, 0));
   assert_eq!(
      authorise(&unlocked(), &profile, &analysis),
      Decision::Allowed { prompt: true },
      "the listed id may go"
   );

   analysis.balance_diff.nfts.clear();
   analysis.balance_diff.nfts.push(nft_row(bob(), 8, 1, 0));

   let Decision::Denied(denial) = authorise(&unlocked(), &profile, &analysis) else {
      panic!("an id the grant never named must not leave the wallet")
   };
   assert_eq!(denial.code(), "NFT_NOT_ALLOWED");
}

#[test]
fn an_entry_records_what_the_policy_decided() {
   let profile = sample_profile();

   let denied = AuditEntry::new(&profile, "tx_send", "wallet.write.send", 10)
      .decided(&Decision::Denied(Denial::VaultLocked));
   assert_eq!(denied.decision, AuditDecision::Denied);
   assert_eq!(denied.denial.as_deref(), Some("VAULT_LOCKED"));
   assert_eq!(denied.detail.as_deref(), Some("Zeus is locked"));

   let prompted = AuditEntry::new(
      &profile,
      "networks_set_rpc",
      "settings.networks.write",
      11,
   )
   .decided(&Decision::Allowed { prompt: true });
   assert_eq!(prompted.decision, AuditDecision::Prompted);

   let allowed = AuditEntry::new(
      &profile,
      "wallet_balance",
      "wallet.read.portfolio",
      12,
   )
   .decided(&Decision::Allowed { prompt: false });
   assert_eq!(allowed.decision, AuditDecision::Allowed);
}

#[test]
fn the_audit_ring_drops_the_oldest_entry() {
   let log = AgentAuditLog::default();
   let profile = sample_profile();

   for ts in 0..=AUDIT_RING_CAPACITY as u64 {
      log.record(AuditEntry::new(
         &profile,
         "tx_send",
         "wallet.write.send",
         ts,
      ));
   }

   assert_eq!(
      log.len(),
      AUDIT_RING_CAPACITY,
      "the ring has a ceiling"
   );
   let tail = log.tail(AUDIT_RING_CAPACITY);
   assert_eq!(
      tail.first().expect("an entry").ts,
      1,
      "the oldest fell off"
   );
   assert_eq!(
      tail.last().expect("an entry").ts,
      AUDIT_RING_CAPACITY as u64
   );
   assert_eq!(
      log.tail(2).len(),
      2,
      "asking for the tail never returns more than asked"
   );
}

#[test]
fn the_audit_log_roundtrips_through_the_sealed_codec() {
   let key = WalletStateKey::generate().expect("key");
   let profile = sample_profile();
   let log = AgentAuditLog::default();

   log.record(AuditEntry::new(
      &profile,
      "tx_send",
      "wallet.write.send",
      10,
   ));
   log.record(
      AuditEntry::new(
         &profile,
         "wallet_balance",
         "wallet.read.portfolio",
         11,
      )
      .decided(&Decision::Allowed { prompt: false }),
   );

   let sealed = log.encode(&key).expect("seal");
   assert!(
      !sealed
         .windows(profile.label.len())
         .any(|window| window == profile.label.as_bytes()),
      "the label, and the tool names beside it, are not on disk in the clear"
   );

   let back = AgentAuditLog::decode(&key, &sealed).expect("open");
   assert_eq!(back.len(), 2);
   assert_eq!(back.tail(10), log.tail(10));
   assert_eq!(back.unreadable(), None);
}

#[test]
fn an_audit_save_is_blocked_after_the_file_did_not_read() {
   let log = AgentAuditLog::default();
   log.block_saves("wallet state key mismatch");

   let err = log
      .save(&WalletStateKey::generate().expect("key"))
      .expect_err("a blocked log must not write");
   assert!(
      err.to_string().contains("agent_audit.data"),
      "{err}"
   );
   assert!(
      err.to_string().contains("wallet state key mismatch"),
      "{err}"
   );

   // A fresh load clears the block: the bytes are no longer the only copy of the history.
   log.replace_from(&AgentAuditLog::default());
   assert_eq!(log.unreadable(), None);
}
