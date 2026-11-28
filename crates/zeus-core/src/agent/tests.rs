//! The grant document's own tests: no GUI, no chain, no HTTP, no window.

use super::audit::{AUDIT_RING_CAPACITY, AgentAuditLog, AuditDecision, AuditEntry};
use super::env::AgentEnv;
use super::exec::{
   BridgeRequest, Broadcast, ExecCall, ExecError, Origin, QuoteFuture, QuoteRequest, SendRequest,
   SwapQuote, SwapRequest, TxFlows, TxFuture, UnshieldRequest, WalletLock, execute,
   overrides_broadcast, preview,
};
use super::intent::{Asset, Intent};
use super::policy::{Decision, Denial, authorise, precheck};
use super::profile::CAPABILITIES;
use super::profile::{
   AgentProfile, AgentProfileId, CapabilityScope, CapabilityVerb, DEFAULT_MAX_PRICE_IMPACT_BPS,
   DEFAULT_MAX_TRANSFER_SHORTFALL_BPS, MAX_PRICE_IMPACT_BPS, MAX_SLIPPAGE_BPS,
   MAX_TRANSFER_SHORTFALL_BPS, NftRef, NftScope, PrivacyBadge, ProfileMode, RiskTier, ScreenGrant,
   TokenRef, TokenScope, WalletGrant, capability,
};
use super::read;
use super::store::AgentProfileStore;
use super::token;
use crate::WalletStateKey;
use crate::ZeusCtx;
use crate::persisted::{ExportPolicy, PersistedFile, is_allowed_rel_parts};
use crate::sim::balance_diff::{BalanceChange, NftBalanceChange, native_change};
use crate::tx::{Amount, PriorityTier, TransactionAnalysis, TransactionRich, TxPromise};
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
   assert_eq!(
      profile.max_price_impact_bps,
      DEFAULT_MAX_PRICE_IMPACT_BPS
   );
   assert_eq!(
      profile.max_transfer_shortfall_bps,
      DEFAULT_MAX_TRANSFER_SHORTFALL_BPS
   );
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
fn the_guard_ceilings_are_clamped_and_a_narrower_one_is_left_alone() {
   let mut profile = sample_profile();
   profile.max_price_impact_bps = 50_000;
   profile.max_transfer_shortfall_bps = 50_000;

   assert!(
      profile.sanitize().is_empty(),
      "a bound is clamped, not dropped"
   );
   assert_eq!(profile.max_price_impact_bps, MAX_PRICE_IMPACT_BPS);
   assert_eq!(
      profile.max_transfer_shortfall_bps,
      MAX_TRANSFER_SHORTFALL_BPS
   );

   // A user who asked for less keeps less: zero is "no impact at all", not "no limit".
   profile.max_price_impact_bps = 50;
   profile.max_transfer_shortfall_bps = 0;
   profile.sanitize();
   assert_eq!(profile.max_price_impact_bps, 50);
   assert_eq!(profile.max_transfer_shortfall_bps, 0);
}

/// A grant document written before the guards existed must load with the safe default, not zero — a zero
/// ceiling would refuse every autonomous swap the moment this build read an older file.
#[test]
fn a_grant_written_before_the_guards_loads_with_the_defaults() {
   let mut profile = sample_profile();
   profile.max_price_impact_bps = DEFAULT_MAX_PRICE_IMPACT_BPS;
   profile.max_transfer_shortfall_bps = DEFAULT_MAX_TRANSFER_SHORTFALL_BPS;

   let mut value = serde_json::to_value(&profile).expect("serialize");
   let object = value.as_object_mut().expect("an object");
   object.remove("max_price_impact_bps");
   object.remove("max_transfer_shortfall_bps");

   let back: AgentProfile = serde_json::from_value(value).expect("deserialize");

   assert_eq!(
      back.max_price_impact_bps,
      DEFAULT_MAX_PRICE_IMPACT_BPS
   );
   assert_eq!(
      back.max_transfer_shortfall_bps,
      DEFAULT_MAX_TRANSFER_SHORTFALL_BPS
   );
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

   let Decision::Denied(denial) = authorise(&unlocked(), &profile, &analysis, None) else {
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
      authorise(
         &unlocked(),
         &profile,
         &simulated(unknown, 1),
         None
      ),
      Decision::Denied(Denial::WalletNotGranted { wallet: unknown })
   );

   profile.wallets.get_mut(&bob()).expect("bob").enabled = false;
   assert_eq!(
      authorise(&unlocked(), &profile, &simulated(bob(), 1), None),
      Decision::Denied(Denial::WalletNotGranted { wallet: bob() })
   );
}

#[test]
fn authorise_is_refused_while_the_vault_is_locked() {
   let profile = sample_profile();

   assert_eq!(
      authorise(&locked(), &profile, &simulated(alice(), 1), None),
      Decision::Denied(Denial::VaultLocked)
   );
}

#[test]
fn authorise_prompts_only_in_confirm_each() {
   let mut profile = sample_profile();

   assert_eq!(
      authorise(
         &unlocked(),
         &profile,
         &simulated(alice(), 1),
         None
      ),
      Decision::Allowed { prompt: true }
   );

   profile.mode = ProfileMode::Authorized;
   assert_eq!(
      authorise(
         &unlocked(),
         &profile,
         &simulated(alice(), 1),
         None
      ),
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
         authorise(&unlocked(), &profile, &analysis, None),
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
      authorise(&unlocked(), &profile, &spending, None),
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
         authorise(&unlocked(), &profile, &fee_only, None),
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
      authorise(&unlocked(), &profile, &analysis, None),
      Decision::Allowed { prompt: true },
      "the listed id may go"
   );

   analysis.balance_diff.nfts.clear();
   analysis.balance_diff.nfts.push(nft_row(bob(), 8, 1, 0));

   let Decision::Denied(denial) = authorise(&unlocked(), &profile, &analysis, None) else {
      panic!("an id the grant never named must not leave the wallet")
   };
   assert_eq!(denial.code(), "NFT_NOT_ALLOWED");
}

/// An unlocked context in which the app quotes two prices: ETH at $1 700 and the fixture token at $1.
///
/// The price table is what the pool pricing would have filled, and it is set here rather
/// than mocked away because the gate reads it through the same accessor every flow does.
fn priced() -> ZeusCtx {
   let ctx = unlocked();
   let token = usdc();
   ctx.price_manager().write(|manager| {
      manager.token_prices.insert(
         (1, ERC20Token::wrapped_native_token(1).address),
         NumericValue::currency_price(1700.0),
      );
      manager.token_prices.insert(
         (1, token.address),
         NumericValue::currency_price(1.0),
      );
   });
   ctx
}

/// A swap of one ETH into a stablecoin, quoted at `expected_out` and measured at `arrived`.
fn swap_of(promise: f64, arrived: f64) -> (TransactionAnalysis, TxPromise) {
   let mut analysis = simulated(alice(), 1);
   analysis.set_measured_out(NumericValue::from_f64(arrived));

   let promise = TxPromise::Swap {
      input: Amount::new(Currency::native(1), NumericValue::from_f64(1.0)),
      output: Currency::from(usdc()),
      expected_out: NumericValue::from_f64(promise),
      floor: NumericValue::from_f64(promise * 0.995),
   };

   (analysis, promise)
}

#[test]
fn an_autonomous_swap_at_the_impact_ceiling_is_allowed_and_one_basis_point_over_is_refused() {
   let profile = sample_profile();
   let ctx = priced();

   // $1 700 in, $1 615 out: 5%, exactly the default ceiling, which is the last impact that passes.
   let (at, promise) = swap_of(1_615.0, 1_615.0);
   assert_eq!(
      authorise(&ctx, &profile, &at, Some(&promise)),
      Decision::Allowed { prompt: true },
      "a ConfirmEach profile has the human as its reviewer, so no ceiling applies to it"
   );

   let mut autonomous = profile.clone();
   autonomous.mode = ProfileMode::Authorized;
   assert!(
      matches!(
         authorise(&ctx, &autonomous, &at, Some(&promise)),
         Decision::Allowed { prompt: false }
      ),
      "exactly at the ceiling is allowed: the ceiling is what a grant permits, not what it doubts"
   );

   // One basis point further, $1 614.815 out.
   let (over, promise) = swap_of(1_614.815, 1_614.815);
   let Decision::Denied(denial) = authorise(&ctx, &autonomous, &over, Some(&promise)) else {
      panic!("a trade past the ceiling must not run without a human")
   };
   assert_eq!(denial.code(), "PRICE_IMPACT");

   let Denial::PriceImpact(impact) = &denial else {
      panic!("the refusal carries the numbers it was made from")
   };
   assert_eq!(impact.bps, 501);
   assert_eq!(impact.allowed_bps, DEFAULT_MAX_PRICE_IMPACT_BPS);
   assert_eq!(
      impact.usd_in.as_ref().map(NumericValue::f64),
      Some(1700.0)
   );
   assert_eq!(
      impact.usd_out.as_ref().map(NumericValue::f64),
      Some(1_614.815)
   );

   let sentence = denial.to_string();
   assert!(sentence.contains("5.01%"), "{sentence}");
   assert!(sentence.contains("5.00%"), "{sentence}");
}

#[test]
fn an_autonomous_send_that_arrives_short_is_refused_and_the_ceiling_is_the_last_one_allowed() {
   let profile = sample_profile();
   let ctx = priced();

   let transfer = |arrived: f64| {
      let mut analysis = simulated(alice(), 1);
      analysis.set_measured_out(NumericValue::from_f64(arrived));

      let promise = TxPromise::Transfer {
         amount: Amount::new(
            Currency::from(usdc()),
            NumericValue::from_f64(100.0),
         ),
      };

      (analysis, promise)
   };

   // 99 arrives where 100 was promised: 1%, the default ceiling.
   let (at, promise) = transfer(99.0);
   assert!(
      matches!(
         authorise(&ctx, &profile, &at, Some(&promise)),
         Decision::Allowed { .. }
      ),
      "a ConfirmEach profile keeps the decision"
   );

   let mut autonomous = profile.clone();
   autonomous.mode = ProfileMode::Authorized;
   assert!(
      matches!(
         authorise(&ctx, &autonomous, &at, Some(&promise)),
         Decision::Allowed { prompt: false }
      ),
      "exactly at the ceiling is allowed"
   );

   // A 3% transfer tax: 97 arrive where 100 were promised.
   let (short, promise) = transfer(97.0);
   let Decision::Denied(denial) = authorise(&ctx, &autonomous, &short, Some(&promise)) else {
      panic!("a transfer tax has to be something the agent can see")
   };
   assert_eq!(denial.code(), "OUTPUT_SHORTFALL");

   let Denial::OutputShortfall(shortfall) = &denial else {
      panic!("the refusal carries both amounts")
   };
   assert_eq!(shortfall.bps, 300);
   assert_eq!(
      shortfall.allowed_bps,
      DEFAULT_MAX_TRANSFER_SHORTFALL_BPS
   );
   assert_eq!(shortfall.expected.f64(), 100.0);
   assert_eq!(shortfall.measured.f64(), 97.0);
   assert_eq!(shortfall.currency.symbol(), "USDC");

   let sentence = denial.to_string();
   assert!(sentence.contains("97"), "{sentence}");
   assert!(sentence.contains("100"), "{sentence}");
   assert!(sentence.contains("3.00%"), "{sentence}");
}

#[test]
fn an_output_no_one_can_price_is_refused_while_the_grant_asks_for_a_priced_trade() {
   let profile = sample_profile();
   let mut autonomous = profile.clone();
   autonomous.mode = ProfileMode::Authorized;

   // The app knows no price for this token at all, which is the state a fresh listing is in.
   let (analysis, promise) = swap_of(1_615.0, 1_615.0);
   let Decision::Denied(denial) = authorise(
      &unlocked(),
      &autonomous,
      &analysis,
      Some(&promise),
   ) else {
      panic!("an unpriced output cannot be measured against a dollar ceiling")
   };
   assert_eq!(denial.code(), "PRICE_UNAVAILABLE");

   let Denial::PriceUnavailable { currencies } = &denial else {
      panic!("the refusal names what it could not price")
   };
   assert_eq!(currencies.len(), 2, "both legs of the trade");
   assert!(denial.to_string().contains("USDC"));

   assert!(
      matches!(
         authorise(&unlocked(), &profile, &analysis, Some(&promise)),
         Decision::Allowed { .. }
      ),
      "the human sees the same window and keeps the decision"
   );
}

/// A one-sided move is bounded by the shortfall alone, which is measured in the asset's own units — so a
/// token the app has no market for can still be sent, shielded or unshielded under the grant that allows it.
#[test]
fn a_one_sided_move_of_an_unpriced_token_is_allowed() {
   let profile = sample_profile();
   let mut autonomous = profile.clone();
   autonomous.mode = ProfileMode::Authorized;

   let mut analysis = simulated(alice(), 1);
   analysis.set_measured_out(NumericValue::from_f64(1_000.0));

   let promise = TxPromise::Transfer {
      amount: Amount::new(
         Currency::from(usdc()),
         NumericValue::from_f64(1_000.0),
      ),
   };

   assert!(
      matches!(
         authorise(
            &unlocked(),
            &autonomous,
            &analysis,
            Some(&promise)
         ),
         Decision::Allowed { .. }
      ),
      "nothing in a transfer needs a price, so an unpriced token must not be refused for want of one"
   );
}

/// The confirmation seam answers `anyhow`, so the wrapper is how a policy refusal keeps the variant the
/// tool boundary turns into a code. Without it every refusal read as "the app's own words".
#[test]
fn an_agent_denial_keeps_its_code_through_an_anyhow_error() {
   let error: anyhow::Error = crate::agent::AgentDenial(Denial::TokenNotAllowed {
      wallet: alice(),
      token: TokenRef {
         chain: 1,
         address: Address::repeat_byte(0x2a),
      },
   })
   .into();

   let carried = error
      .downcast_ref::<crate::agent::AgentDenial>()
      .expect("the typed refusal survives the error");

   assert_eq!(carried.0.code(), "TOKEN_NOT_ALLOWED");
}

#[test]
fn a_request_that_states_no_promise_is_decided_exactly_as_it_was_before() {
   let profile = sample_profile();
   let mut autonomous = profile.clone();
   autonomous.mode = ProfileMode::Authorized;

   // The analysis says a swap went badly wrong — far past every ceiling — but no flow stated what it
   // promised, so there is nothing to hold it to: a dapp request, an approval and a preview all land here.
   let (analysis, _) = swap_of(1_000.0, 500.0);
   assert!(
      matches!(
         authorise(&unlocked(), &autonomous, &analysis, None),
         Decision::Allowed { prompt: false }
      ),
      "a flow that has not been taught to state a promise must not be refused for lacking one"
   );
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

/// A flow that records whether it was reached, and answers one fixed transaction.
#[derive(Default)]
struct RecordingFlows {
   sends: std::sync::atomic::AtomicUsize,
   swaps: std::sync::atomic::AtomicUsize,
   unshields: std::sync::atomic::AtomicUsize,
   /// How the fake flow's transaction reads against its promise, for the record that carries one.
   review: Option<crate::tx::SimReview>,
   /// The URL the app "has stored", for the unshield override question.
   bundler: Option<&'static str>,
}

impl RecordingFlows {
   fn sends(&self) -> usize {
      self.sends.load(std::sync::atomic::Ordering::SeqCst)
   }

   /// The transaction this flow answers with, carrying the reading when one was set.
   fn done(&self) -> TransactionRich {
      let mut rich = TransactionRich::dummy_clear_signed();

      if let Some(review) = self.review.clone() {
         rich.analysis.set_review(review);
      }

      rich
   }

   fn swaps(&self) -> usize {
      self.swaps.load(std::sync::atomic::Ordering::SeqCst)
   }
}

impl TxFlows for RecordingFlows {
   fn send(
      &self,
      _env: AgentEnv,
      _request: SendRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      self.sends.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

      let done = self.done();

      Box::pin(async move { Ok(done) })
   }

   fn swap(
      &self,
      _env: AgentEnv,
      _request: SwapRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      self.swaps.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

      Box::pin(async { Ok(TransactionRich::dummy_clear_signed()) })
   }

   fn unshield(
      &self,
      _env: AgentEnv,
      _request: UnshieldRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      self.unshields.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

      Box::pin(async { Ok(TransactionRich::dummy_clear_signed()) })
   }

   fn quote(&self, _request: QuoteRequest) -> QuoteFuture<'_> {
      Box::pin(async { Ok(SwapQuote::default()) })
   }

   fn stored_bundler_url(&self) -> Option<String> {
      self.bundler.map(str::to_owned)
   }
}

/// One native send from alice, the asset her grant does allow.
fn exec_send() -> ExecCall {
   ExecCall::Send(SendRequest {
      chain: 1,
      from: alice(),
      to: bob(),
      token: None,
      amount: U256::from(1_500_000_000_000_000u64),
      decimals: 18,
      priority: PriorityTier::Normal,
   })
}

fn exec_origin() -> Origin<'static> {
   Origin {
      tool: "tx_send",
      client_info: Some("tests"),
   }
}

/// Every sentence the app said out loud, for the tests that are about being heard.
static ANNOUNCEMENTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn announcements() -> Vec<String> {
   ANNOUNCEMENTS.lock().unwrap().clone()
}

thread_local! {
   /// What this test's own thread said, so a test can count its sentences rather than every test's.
   static SAID: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Drain the sentences this thread announced, oldest first.
fn said_on_this_thread() -> Vec<String> {
   SAID.with(|said| std::mem::take(&mut *said.borrow_mut()))
}

/// A host that answers like the headless one and remembers what was announced.
struct CountingHost;

impl crate::ui::UiHost for CountingHost {
   fn request_repaint(&self) {}

   fn open_loading(&self, _msg: &str) {}

   fn reset_loading(&self) {}

   fn open_tx_confirm(&self, _req: crate::ui::TxConfirmRequest) -> crate::ui::PromptOpen {
      crate::ui::PromptOpen::Unavailable
   }

   fn take_tx_confirm(&self) -> Option<crate::ui::TxConfirmAnswer> {
      None
   }

   fn release_tx_confirm(&self) {}

   fn open_sign(&self, _req: crate::ui::SignRequest) -> crate::ui::PromptOpen {
      crate::ui::PromptOpen::Unavailable
   }

   fn take_sign(&self) -> Option<bool> {
      None
   }

   fn release_sign(&self) {}

   fn open_confirm(&self, _req: crate::ui::ConfirmRequest) -> crate::ui::PromptOpen {
      crate::ui::PromptOpen::Unavailable
   }

   fn take_confirm(&self) -> Option<bool> {
      None
   }

   fn release_confirm(&self) {}

   fn notify_pending(&self, _title: &str, _kind: crate::tx::notification::NotificationType) {}

   fn notify_mined(
      &self,
      _title: &str,
      _kind: crate::tx::notification::NotificationType,
      _tx: TransactionRich,
      _now_ms: u64,
      _finish_ms: u64,
   ) {
   }

   fn announce(&self, message: &str) {
      ANNOUNCEMENTS.lock().unwrap().push(message.to_owned());
      SAID.with(|said| said.borrow_mut().push(message.to_owned()));
   }

   fn lockout_and_shutdown(&self, _msg: &str) {}
}

#[tokio::test]
async fn a_wallet_takes_one_action_at_a_time() {
   let held = WalletLock::take(1, alice()).await.expect("the first action takes it");

   assert!(
      matches!(
         WalletLock::take(1, alice()).await,
         Err(ExecError::Busy)
      ),
      "a second action on the same wallet is told to come back, not queued behind a prompt"
   );

   drop(WalletLock::take(1, bob()).await.expect("another wallet is another action"));

   drop(held);
   drop(
      WalletLock::take(1, alice())
         .await
         .expect("the wallet is free once the action ends"),
   );
}

#[tokio::test]
async fn a_locked_vault_refuses_before_the_flow_runs() {
   let flows = RecordingFlows::default();
   let ctx = locked();

   let error = execute(
      ctx.clone(),
      &sample_profile(),
      exec_send(),
      &exec_origin(),
      &flows,
   )
   .await
   .expect_err("a locked vault signs nothing");

   assert_eq!(error, ExecError::Locked);
   assert_eq!(flows.sends(), 0);

   let tail = ctx.agent_audit().tail(1);
   assert_eq!(tail[0].denial.as_deref(), Some("VAULT_LOCKED"));
   assert_eq!(tail[0].tool, "tx_send");
}

#[tokio::test]
async fn an_asset_outside_the_allowlist_never_reaches_the_flow() {
   let flows = RecordingFlows::default();
   let mut profile = sample_profile();
   profile.mode = ProfileMode::Authorized;

   let call = ExecCall::Send(SendRequest {
      chain: 1,
      from: alice(),
      to: bob(),
      token: Some(usdc().address),
      amount: U256::from(1_000_000u64),
      decimals: 6,
      priority: PriorityTier::Normal,
   });

   let error = execute(unlocked(), &profile, call, &exec_origin(), &flows)
      .await
      .expect_err("USDC is not in alice's allowlist");

   assert_eq!(error.code(), "TOKEN_NOT_ALLOWED");
   assert_eq!(
      flows.sends(),
      0,
      "the allowlist stops the action before anything is simulated, signed or sent"
   );
}

/// An autonomous action leaves a record, and the record says what the reading was.
#[tokio::test]
async fn a_completed_action_records_what_its_simulation_read() {
   crate::ui::install(CountingHost);

   let flows = RecordingFlows {
      review: Some(crate::tx::SimReview {
         usd_in: Some(NumericValue::currency_price(1700.0)),
         usd_out: Some(NumericValue::currency_price(1615.0)),
         price_impact_bps: Some(29),
         shortfall_bps: Some(0),
         ..crate::tx::SimReview::default()
      }),
      ..RecordingFlows::default()
   };

   let mut profile = sample_profile();
   profile.mode = ProfileMode::Authorized;
   let ctx = unlocked();

   let answer = execute(
      ctx.clone(),
      &profile,
      exec_send(),
      &exec_origin(),
      &flows,
   )
   .await
   .expect("the flow answered");

   // The answer carries it, so a caller reads the same numbers the gate decided on.
   let review = &answer["simulation"]["review"];
   assert_eq!(review["price_impact_bps"], 29, "{answer}");
   assert_eq!(review["complete"], true);

   // And the record keeps it: nothing asked the user first, so this entry is the only trace there is.
   let detail = ctx.agent_audit().tail(1)[0]
      .detail
      .clone()
      .expect("the record says what happened");
   assert!(detail.starts_with("Agent sent "), "{detail}");
   assert!(detail.ends_with("price impact 0.29%"), "{detail}");
}

/// A flow that states no promise records no reading, and its answer says so rather than reporting zeroes.
#[tokio::test]
async fn an_action_without_a_promise_reports_no_reading() {
   crate::ui::install(CountingHost);

   let flows = RecordingFlows::default();
   let mut profile = sample_profile();
   profile.mode = ProfileMode::Authorized;
   let ctx = unlocked();

   let answer = execute(
      ctx.clone(),
      &profile,
      exec_send(),
      &exec_origin(),
      &flows,
   )
   .await
   .expect("the flow answered");

   assert!(
      answer["simulation"]["review"].is_null(),
      "an unmeasured action must not read as a perfect one: {answer}"
   );

   let detail = ctx.agent_audit().tail(1)[0]
      .detail
      .clone()
      .expect("the record says what happened");
   assert!(!detail.contains("price impact"), "{detail}");
}

#[tokio::test]
async fn a_completed_action_is_recorded_and_said_out_loud() {
   crate::ui::install(CountingHost);

   let flows = RecordingFlows::default();
   let mut profile = sample_profile();
   profile.mode = ProfileMode::Authorized;
   let ctx = unlocked();

   let answer = execute(
      ctx.clone(),
      &profile,
      exec_send(),
      &exec_origin(),
      &flows,
   )
   .await
   .expect("the flow answered");

   assert!(answer["tx"]["hash"].is_string(), "{answer}");
   assert_eq!(answer["policy"]["decision"], "allowed");
   assert_eq!(flows.sends(), 1);

   let tail = ctx.agent_audit().tail(1);
   assert_eq!(tail[0].decision, AuditDecision::Allowed);
   assert_eq!(tail[0].wallet, Some(alice()));
   assert_eq!(tail[0].chain, Some(1));
   assert!(
      tail[0].tx_hash.is_some(),
      "the record points at the row it made"
   );
   assert_eq!(tail[0].client_info.as_deref(), Some("tests"));

   let detail = tail[0].detail.clone().expect("the record says what happened");
   assert!(detail.starts_with("Agent sent "), "{detail}");

   assert!(
      announcements().iter().any(|line| line == &detail),
      "nothing asked the user first, so the app says what it did: {:?}",
      announcements()
   );

   assert_eq!(
      said_on_this_thread(),
      [detail],
      "one action is one sentence, not a burst of them"
   );
}

/// An action the policy refuses is recorded, and the refusal is the one sentence the user hears.
#[tokio::test]
async fn a_denied_action_is_said_out_loud_once() {
   crate::ui::install(CountingHost);

   let flows = RecordingFlows::default();
   let mut profile = sample_profile();
   profile.mode = ProfileMode::Authorized;

   let call = ExecCall::Send(SendRequest {
      chain: 1,
      from: alice(),
      to: bob(),
      token: Some(usdc().address),
      amount: U256::from(1_000_000u64),
      decimals: 6,
      priority: PriorityTier::Normal,
   });

   let ctx = unlocked();

   execute(
      ctx.clone(),
      &profile,
      call,
      &exec_origin(),
      &flows,
   )
   .await
   .expect_err("USDC is not in alice's allowlist");

   let detail = ctx.agent_audit().tail(1)[0].detail.clone().expect("the record says why");

   let said = said_on_this_thread();

   assert_eq!(said.len(), 1, "{said:?}");
   assert_eq!(said[0], format!("Agent was denied: {detail}"));
   assert!(
      said[0].contains("token allowlist"),
      "the sentence names the allowlist that refused it: {}",
      said[0]
   );
}

/// The activity read answers one profile's entries, newest first, and counts what it left out.
#[test]
fn the_activity_read_is_one_profile_newest_first() {
   let ctx = unlocked();
   let profile = sample_profile();
   let other = AgentProfile::new("a second credential", [4u8; 32], 0);

   for (ts, tool) in [(10, "wallet_list"), (20, "tx_send"), (30, "settings_set")] {
      ctx.agent_audit().record(
         AuditEntry::new(&profile, tool, "wallet.read.history", ts)
            .decided(&Decision::Allowed { prompt: false }),
      );
   }

   ctx.agent_audit().record(
      AuditEntry::new(&other, "tx_send", "wallet.write.send", 40)
         .decided(&Decision::Denied(Denial::VaultLocked)),
   );

   let (all, newest, since) = ctx.read(|state| {
      (
         read::activity(state, &profile, 20, None),
         read::activity(state, &profile, 1, None),
         read::activity(state, &profile, 20, Some(20)),
      )
   });

   assert_eq!(all["count"], 3);
   assert_eq!(all["matching"], 3);
   assert_eq!(
      all["entries"][0]["tool"], "settings_set",
      "newest first"
   );
   assert_eq!(all["entries"][2]["tool"], "wallet_list");
   assert_eq!(all["entries"][0]["decision"], "allowed");

   assert_eq!(newest["count"], 1, "the limit bounds the rows");
   assert_eq!(
      newest["matching"], 3,
      "and not the count of what matches"
   );
   assert_eq!(newest["entries"][0]["tool"], "settings_set");

   assert_eq!(
      since["count"], 2,
      "since keeps the entries at or after it"
   );

   let text = all.to_string();

   assert!(
      !text.contains("a second credential"),
      "another credential's work is not this profile's answer: {text}"
   );
}

/// Two wallets the money tests of this group act for, so their actions queue behind nothing.
fn carol() -> Address {
   Address::repeat_byte(0xa3)
}

fn dave() -> Address {
   Address::repeat_byte(0xa4)
}

/// A profile that may bridge dave's own coin, and no more than that.
fn bridging_profile() -> AgentProfile {
   let mut profile = sample_profile();
   profile.mode = ProfileMode::Authorized;

   let mut caps = BTreeSet::new();
   caps.insert("wallet.write.bridge".to_string());

   profile.wallets.insert(
      dave(),
      WalletGrant {
         enabled: true,
         caps,
         tokens: TokenScope::Allowlist {
            tokens: vec![TokenRef {
               chain: 1,
               address: Address::ZERO,
            }],
         },
         ..WalletGrant::default()
      },
   );

   profile
}

/// A swap carol's grant would allow: the native token leaves, and the route's own tokens are not named.
fn exec_swap(slippage_bps: u16) -> ExecCall {
   ExecCall::Swap(SwapRequest {
      chain: 1,
      from: carol(),
      currency_in: Address::ZERO,
      currency_out: usdc().address,
      amount_in: U256::from(1_000_000_000_000_000u64),
      decimals_in: 18,
      slippage_bps,
      deadline: 5,
      priority: PriorityTier::Normal,
   })
}

/// A profile that may swap carol's own coin, and no more than that.
fn swapping_profile(max_slippage_bps: u16) -> AgentProfile {
   let mut profile = sample_profile();
   profile.mode = ProfileMode::Authorized;
   profile.max_slippage_bps = max_slippage_bps;

   let mut caps = BTreeSet::new();
   caps.insert("wallet.write.swap".to_string());

   profile.wallets.insert(
      carol(),
      WalletGrant {
         enabled: true,
         caps,
         tokens: TokenScope::Allowlist {
            tokens: vec![TokenRef {
               chain: 1,
               address: Address::ZERO,
            }],
         },
         ..WalletGrant::default()
      },
   );

   profile
}

#[tokio::test]
async fn a_swap_over_the_grant_s_cap_never_reaches_the_flow() {
   let flows = RecordingFlows::default();
   let profile = swapping_profile(50);

   let error = execute(
      unlocked(),
      &profile,
      exec_swap(500),
      &exec_origin(),
      &flows,
   )
   .await
   .expect_err("5% is over a 0.5% cap");

   assert_eq!(
      error.code(),
      "VALIDATION",
      "an agent may not pick a slippage its grant does not allow"
   );
   assert_eq!(flows.swaps(), 0);
}

#[tokio::test]
async fn a_swap_within_the_cap_runs_through_the_port() {
   let flows = RecordingFlows::default();
   let profile = swapping_profile(50);

   execute(
      unlocked(),
      &profile,
      exec_swap(50),
      &exec_origin(),
      &flows,
   )
   .await
   .expect("the cap allows this one");

   assert_eq!(flows.swaps(), 1);
}

#[tokio::test]
async fn a_bridge_asks_the_port_to_screen_the_destination_chain() {
   let flows = RecordingFlows::default();
   let profile = bridging_profile();

   let call = ExecCall::Bridge(BridgeRequest {
      chain: 1,
      dest_chain: 10,
      from: dave(),
      to: bob(),
      token: Address::ZERO,
      amount: U256::from(1_000_000u64),
      decimals: 18,
      priority: PriorityTier::Normal,
   });

   // Every port method refuses unless it is implemented, and the screen is the first thing a bridge asks
   // for: an unscreened recipient is one the bridge must not send to.
   let error = execute(unlocked(), &profile, call, &exec_origin(), &flows)
      .await
      .expect_err("a port that cannot screen cannot bridge");

   assert_eq!(error, ExecError::Unavailable);
}

#[test]
fn a_bundler_url_is_an_override_only_when_it_differs() {
   // Saying what the app already says is not an override: a caller repeating the settings it read has not
   // asked for anything new.
   let stored = RecordingFlows {
      bundler: Some("https://bundler.example/rpc"),
      ..RecordingFlows::default()
   };

   let same = Broadcast {
      self_broadcast: false,
      bundler_url: Some("https://bundler.example/rpc".to_owned()),
   };
   assert!(!overrides_broadcast(&stored, &same));

   let different = Broadcast {
      self_broadcast: false,
      bundler_url: Some("https://mine.example/rpc".to_owned()),
   };
   assert!(overrides_broadcast(&stored, &different));
   assert!(!overrides_broadcast(
      &stored,
      &Broadcast {
         self_broadcast: false,
         bundler_url: None,
      }
   ));
}

#[tokio::test]
async fn a_preview_says_it_is_one() {
   let flows = RecordingFlows::default();

   // A preview of a send whose recipient cannot be screened is refused like the send would be, and it
   // never reaches the flow: a dry run is the simulation, not a second attempt.
   let error = preview(unlocked(), &sample_profile(), &exec_send())
      .await
      .expect_err("no node is attached, so the recipient cannot be screened");

   assert_eq!(error.code(), "VALIDATION");
   assert_eq!(flows.sends(), 0);
}
