//! The grant document: one agent credential and everything it may do.
//!
//! This is an on-disk shape, so two of its maps are deliberately keyed by **strings** — the capability
//! set on a wallet grant, and the screen map on the profile. Zeus already knows the trap: a renamed enum
//! value in a plaintext store silently resets every other preference in the file. A `BTreeMap<String, _>`
//! loads an unknown key instead of failing the file, [`AgentProfile::sanitize`] drops it with a log line,
//! and an unknown key is never read as allowed.
//!
//! Nothing here decides a request: [`CAPABILITIES`] says what *can* be granted and how loudly each
//! capability is presented, and a caller asks the profile its own question. Keeping the catalog apart from
//! the decision is what lets every rule be tested without a GUI, a chain or an HTTP client.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use zeus_eth::alloy_primitives::{Address, U256, hex};

/// The largest slippage a grant may allow, in basis points.
///
/// It is the maximum the swap UI itself offers — `MAX_SLIPPAGE` is `20.0` percent in
/// `src/gui/ui/dapps/uniswap/settings.rs:8` — because a grant must not let an agent choose a slippage
/// a human cannot. The value is spelled here rather than imported because that constant lives in the
/// binary crate, which this one cannot name, so the swap UI's maximum and this one move together.
pub const MAX_SLIPPAGE_BPS: u16 = 2_000;

/// The most a simulated trade may lose to price impact before an autonomous call is refused.
///
/// Price impact is measured against the quote the caller was shown: 5% by default, and 20% is the widest a
/// grant may set, because past that the "quote" is not a price anyone could trade on.
pub const DEFAULT_MAX_PRICE_IMPACT_BPS: u16 = 500;

/// The ceiling for [AgentProfile::max_price_impact_bps].
pub const MAX_PRICE_IMPACT_BPS: u16 = 2_000;

/// How far short of the promised amount an autonomous call may arrive before it is refused.
///
/// 1% by default — a transfer tax, a fee-on-transfer token and a hook's cut all land above it — and 10% is
/// the widest a grant may set.
pub const DEFAULT_MAX_TRANSFER_SHORTFALL_BPS: u16 = 100;

/// The ceiling for [AgentProfile::max_transfer_shortfall_bps].
pub const MAX_TRANSFER_SHORTFALL_BPS: u16 = 1_000;

/// One client credential: a label, a bearer-token hash, a grant, an autonomy mode.
///
/// Every field added after the first release carries `#[serde(default)]`, so a document written by an
/// older build still loads and one written by a newer build degrades to the safe value instead of
/// failing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfile {
   /// 16 random bytes, hex. The audit log's key.
   pub id: AgentProfileId,
   /// What the user calls this client ("Claude Code — laptop").
   pub label: String,
   /// `keccak256` of the visible token. The token itself is never written, so there is nothing here to
   /// recover a credential from.
   #[serde(default)]
   pub token_hash: [u8; 32],
   /// Unix seconds.
   #[serde(default)]
   pub created_at: u64,
   #[serde(default)]
   pub last_used_at: Option<u64>,
   /// The per-profile kill switch. Read at the first line of every dispatch, not as a filter.
   #[serde(default)]
   pub enabled: bool,
   /// Whether a granted capability acts without a prompt: the grant is the authorisation, or every
   /// state change is confirmed.
   #[serde(default)]
   pub mode: ProfileMode,
   /// Caps a per-call slippage on this profile's swaps; `0` means no slippage at all.
   #[serde(default = "default_max_slippage_bps")]
   pub max_slippage_bps: u16,
   /// Caps the price impact a simulated trade may show against its quote. See
   /// [DEFAULT_MAX_PRICE_IMPACT_BPS].
   ///
   /// Carries a default rather than zero: a grant document written before this field existed must keep
   /// trading, and a ceiling of zero would refuse every swap.
   #[serde(default = "default_max_price_impact_bps")]
   pub max_price_impact_bps: u16,
   /// Caps how far short of the promised amount a simulated action may arrive. See
   /// [DEFAULT_MAX_TRANSFER_SHORTFALL_BPS].
   #[serde(default = "default_max_transfer_shortfall_bps")]
   pub max_transfer_shortfall_bps: u16,
   /// Where `data_export` writes, chosen once when the grant is made. `None` means an export still asks
   /// the user for a folder — the one thing an export cannot choose for itself.
   #[serde(default)]
   pub export_dir: Option<PathBuf>,
   /// Enforced on every request. `None` means the credential does not expire.
   #[serde(default)]
   pub expires_at: Option<u64>,
   /// Per-wallet grants, keyed by the wallet's address. A wallet absent from here is invisible to the
   /// agent: no reads, no actions, and nothing in any list a tool returns.
   #[serde(default)]
   pub wallets: BTreeMap<Address, WalletGrant>,
   /// The global capabilities this profile holds, keyed by capability string (see [`CAPABILITIES`]).
   ///
   /// One switch per entry is meaningful, and the catalog's [`Capability::verb`] decides which: a
   /// `.read` capability is granted by `read` and a `.write` one by `write`. Sanitising normalises
   /// the other flag away, so a file never carries the same grant two ways.
   #[serde(default)]
   pub screens: BTreeMap<String, ScreenGrant>,
}

impl AgentProfile {
   /// A new, enabled profile in [`ProfileMode::Authorized`] with nothing granted yet.
   ///
   /// Enabled on creation on purpose: the switch that matters is the grant, which starts empty. A
   /// profile that silently did nothing until a second switch was found would split one decision in two.
   pub fn new(label: impl Into<String>, token_hash: [u8; 32], created_at: u64) -> Self {
      Self {
         id: AgentProfileId::generate(),
         label: label.into(),
         token_hash,
         created_at,
         last_used_at: None,
         enabled: true,
         mode: ProfileMode::Authorized,
         max_slippage_bps: MAX_SLIPPAGE_BPS,
         max_price_impact_bps: DEFAULT_MAX_PRICE_IMPACT_BPS,
         max_transfer_shortfall_bps: DEFAULT_MAX_TRANSFER_SHORTFALL_BPS,
         export_dir: None,
         expires_at: None,
         wallets: BTreeMap::new(),
         screens: BTreeMap::new(),
      }
   }

   /// Drop grant keys this build does not know, returning them so the caller can log one line.
   ///
   /// A key is kept only when the catalog knows it **and** its scope matches where it was written: a
   /// `wallet.write.send` in the screen map grants nothing, so it is dropped rather than left to look
   /// like a permission. Dropping is deliberate over failing — a profile file that no longer loads would
   /// de-authorise every harness on a downgrade, and a lost key is a narrower problem than a lost file.
   pub fn sanitize(&mut self) -> Vec<String> {
      let mut dropped = Vec::new();

      for grant in self.wallets.values_mut() {
         grant.caps.retain(|name| {
            let known = capability(name).is_some_and(|cap| cap.scope == CapabilityScope::Wallet);
            if !known {
               dropped.push(name.clone());
            }
            known
         });
      }

      self.screens.retain(|name, grant| {
         let Some(entry) = capability(name).filter(|entry| entry.scope == CapabilityScope::Global)
         else {
            dropped.push(name.clone());
            return false;
         };

         // One switch is meaningful per capability, and the catalog decides which one: a `.write` is
         // never granted by setting `read`. Normalising here keeps the file canonical, so the same
         // grant cannot be written two ways and read back differently.
         *grant = grant.granted_for(entry.verb());
         true
      });

      self.clamp();
      dropped
   }

   /// Clamp the bounds a grant carries, the way the plaintext stores are clamped on load and before save.
   ///
   /// Only the ceilings are clamped. A narrower bound — a short token list, a zero slippage, a daily cap
   /// of one — is what the user asked for and is left alone.
   pub fn clamp(&mut self) {
      self.max_slippage_bps = self.max_slippage_bps.min(MAX_SLIPPAGE_BPS);
      self.max_price_impact_bps = self.max_price_impact_bps.min(MAX_PRICE_IMPACT_BPS);
      self.max_transfer_shortfall_bps =
         self.max_transfer_shortfall_bps.min(MAX_TRANSFER_SHORTFALL_BPS);
   }

   /// Whether `address` holds `capability` under this profile.
   ///
   /// The one place the two halves of a grant (the profile's kill switch and the wallet's capability set)
   /// are read together, so a caller cannot check one and forget the other.
   pub fn allows(&self, address: Address, name: &str) -> bool {
      self.enabled
         && self
            .wallets
            .get(&address)
            .is_some_and(|grant| grant.enabled && grant.caps.contains(name))
   }

   /// Whether this profile holds the global `name`, reading the switch the catalog says grants it.
   ///
   /// A capability of the wrong scope, or one this build does not know, is never granted: the catalog is
   /// the answer to "may this be granted at all", and a map entry alone is not.
   pub fn allows_global(&self, name: &str) -> bool {
      if !self.enabled {
         return false;
      }

      let Some(entry) = capability(name).filter(|entry| entry.scope == CapabilityScope::Global)
      else {
         return false;
      };

      self.screens.get(name).is_some_and(|grant| grant.is_granted(entry.verb()))
   }

   /// The stored switches for the global `name`, as the grant page shows them.
   pub fn screen(&self, name: &str) -> Option<ScreenGrant> {
      if !self.enabled {
         return None;
      }
      self.screens.get(name).copied()
   }
}

fn default_max_slippage_bps() -> u16 {
   MAX_SLIPPAGE_BPS
}

fn default_max_price_impact_bps() -> u16 {
   DEFAULT_MAX_PRICE_IMPACT_BPS
}

fn default_max_transfer_shortfall_bps() -> u16 {
   DEFAULT_MAX_TRANSFER_SHORTFALL_BPS
}

/// A profile's stable id: 16 random bytes, hex.
///
/// Short enough to read out when an audit entry needs naming, wide enough that two installs cannot
/// collide. The audit log stores it as a plain string, so this is not a capability.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentProfileId(String);

impl AgentProfileId {
   /// 16 random bytes from the OS.
   pub fn generate() -> Self {
      let mut bytes = [0u8; 16];
      rand::rngs::OsRng.fill_bytes(&mut bytes);
      Self(hex::encode(bytes))
   }

   pub fn as_str(&self) -> &str {
      &self.0
   }
}

impl fmt::Display for AgentProfileId {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.write_str(&self.0)
   }
}

/// How a granted capability acts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileMode {
   /// The grant is the authorisation: a granted capability runs with no prompt.
   #[default]
   Authorized,
   /// A cautious opt-in: every state change raises the in-app prompt, as the app does today.
   ConfirmEach,
}

/// One wallet's slice of a grant.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletGrant {
   /// False is a hard stop for this wallet only, leaving the profile's other wallets alone.
   #[serde(default)]
   pub enabled: bool,
   /// Capability strings from [`CAPABILITIES`], e.g. `wallet.write.send`.
   #[serde(default)]
   pub caps: BTreeSet<String>,
   /// Which assets may leave this wallet.
   #[serde(default)]
   pub tokens: TokenScope,
   /// Which NFTs may leave this wallet.
   #[serde(default)]
   pub nfts: NftScope,
}

/// The read and write switches for one global capability (a settings screen, the wallets, the dapps).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenGrant {
   #[serde(default)]
   pub read: bool,
   #[serde(default)]
   pub write: bool,
}

impl ScreenGrant {
   /// The grant with only the switch `verb` names left set.
   pub const fn granted_for(self, verb: CapabilityVerb) -> Self {
      match verb {
         CapabilityVerb::Read => Self {
            read: self.read,
            write: false,
         },
         CapabilityVerb::Write => Self {
            read: false,
            write: self.write,
         },
      }
   }

   /// Whether the switch `verb` names is on.
   pub const fn is_granted(self, verb: CapabilityVerb) -> bool {
      match verb {
         CapabilityVerb::Read => self.read,
         CapabilityVerb::Write => self.write,
      }
   }
}

/// One asset a wallet grant may move.
///
/// The native token is the zero address and must be listed explicitly: "everything except ETH" is not a
/// grant anyone means to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRef {
   pub chain: u64,
   pub address: Address,
}

/// Which assets may leave a wallet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TokenScope {
   /// Nothing may leave: the deny-by-default value.
   #[default]
   None,
   /// Every asset, chosen deliberately in the editor and never a default: a grant that says `All`
   /// can empty the wallet, so the page has to say so in as many words.
   All,
   /// Only these assets.
   Allowlist { tokens: Vec<TokenRef> },
}

/// One NFT collection a wallet grant may move. `token_ids: None` covers every id in the collection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NftRef {
   pub chain: u64,
   pub collection: Address,
   #[serde(default)]
   pub token_ids: Option<Vec<U256>>,
}

/// Which NFTs may leave a wallet.
///
/// An NFT has no price anywhere in Zeus, so this allowlist *is* the whole protection rather than one
/// bound among several: [`NftScope::All`] is the only way to send one the agent was not handed by name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NftScope {
   /// Nothing may leave: the deny-by-default value.
   #[default]
   None,
   All,
   Allowlist {
      nfts: Vec<NftRef>,
   },
}

/// The second axis on a capability, independent of its risk tier: money and privacy are not comparable,
/// and a spend limit says nothing about an anonymity break, so both are stated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyBadge {
   /// The agent's provider sees something that is not public.
   Reveals,
   /// Acting on it links private funds to the public wallet.
   BreaksAnonymity,
}

/// How loudly the grant page presents a capability.
///
/// The tier is grant severity, not policy: it changes the wording, the grant-time warning and the audit
/// line — never whether a granted action asks, which only the profile's mode decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskTier {
   /// Reads nothing secret and changes nothing.
   Read,
   /// Changes state without moving value.
   Low,
   /// Moves value.
   Money,
   /// Moves value and is hard to undo, or widens every other grant.
   Critical,
}

impl RiskTier {
   /// The number the grant page says out loud.
   pub const fn level(self) -> u8 {
      match self {
         Self::Read => 0,
         Self::Low => 1,
         Self::Money => 2,
         Self::Critical => 3,
      }
   }
}

/// Where a capability applies: to one wallet, or to the app as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityScope {
   /// Granted per wallet address in [`AgentProfile::wallets`].
   Wallet,
   /// Granted per profile in [`AgentProfile::screens`].
   Global,
}

/// One grantable verb.
///
/// A capability is a verb in one flat namespace, not a settings page: a screen is a place in the UI, and
/// several screens are per-wallet data in disguise. Encoding those as screens would mint a capability
/// that silently spans every wallet, which is exactly what a least-privilege grant exists to prevent.
#[derive(Clone, Copy, Debug)]
pub struct Capability {
   pub name: &'static str,
   pub tier: RiskTier,
   pub scope: CapabilityScope,
   pub badge: Option<PrivacyBadge>,
}

/// Which switch grants a capability: it reads, or it changes something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityVerb {
   Read,
   Write,
}

impl Capability {
   /// The verb this capability is granted through.
   ///
   /// Derived from the tier rather than stored beside it: every read in the catalog is tier 0, because a
   /// capability that changes nothing has no fund or privacy consequence to grade. A test pins the two
   /// together, so a read that acquired a tier cannot silently become a write here.
   pub const fn verb(self) -> CapabilityVerb {
      match self.tier {
         RiskTier::Read => CapabilityVerb::Read,
         RiskTier::Low | RiskTier::Money | RiskTier::Critical => CapabilityVerb::Write,
      }
   }
}

/// The whole catalog. Everything off by default; a profile holds only what it was granted.
pub const CAPABILITIES: &[Capability] = &[
   // Granted per wallet address.
   wallet("wallet.read.portfolio", RiskTier::Read),
   wallet("wallet.read.history", RiskTier::Read),
   wallet("wallet.read.approvals", RiskTier::Read),
   badged(
      wallet("wallet.read.private", RiskTier::Read),
      PrivacyBadge::Reveals,
   ),
   wallet("wallet.read.names", RiskTier::Read),
   wallet("wallet.write.approvals_revoke", RiskTier::Money),
   wallet("wallet.write.send", RiskTier::Money),
   wallet("wallet.write.nft_send", RiskTier::Money),
   wallet("wallet.write.swap", RiskTier::Money),
   wallet("wallet.write.bridge", RiskTier::Money),
   wallet("wallet.write.shield", RiskTier::Money),
   wallet("wallet.write.unshield", RiskTier::Money),
   wallet("wallet.write.notes_merge", RiskTier::Low),
   wallet("wallet.write.delegate", RiskTier::Critical),
   // Granted per profile.
   global("settings.general.read", RiskTier::Read),
   global("settings.general.write", RiskTier::Low),
   global("settings.keyboard.read", RiskTier::Read),
   global("settings.keyboard.write", RiskTier::Low),
   global("settings.appearance.read", RiskTier::Read),
   global("settings.appearance.write", RiskTier::Low),
   global("settings.networks.read", RiskTier::Read),
   badged(
      global("settings.networks.write", RiskTier::Critical),
      PrivacyBadge::Reveals,
   ),
   global("settings.security.read", RiskTier::Read),
   global("settings.security.write", RiskTier::Critical),
   global("settings.contacts.read", RiskTier::Read),
   global("settings.contacts.write", RiskTier::Critical),
   global("settings.railgun.read", RiskTier::Read),
   global("settings.railgun.write", RiskTier::Low),
   global("settings.data.read", RiskTier::Read),
   global("settings.data.write", RiskTier::Critical),
   global("settings.pools.read", RiskTier::Read),
   global("settings.pools.write", RiskTier::Money),
   global("settings.bridge.read", RiskTier::Read),
   global("settings.bridge.write", RiskTier::Low),
   global("settings.unshield.read", RiskTier::Read),
   badged(
      global("settings.unshield.write", RiskTier::Money),
      PrivacyBadge::BreaksAnonymity,
   ),
   global("settings.wallets.read", RiskTier::Read),
   global("settings.wallets.derive", RiskTier::Critical),
   global("settings.wallets.import", RiskTier::Critical),
   global("settings.dapps.read", RiskTier::Read),
   global("settings.dapps.write", RiskTier::Low),
];

/// The catalog entry for `name`, if this build knows it.
pub fn capability(name: &str) -> Option<&'static Capability> {
   CAPABILITIES.iter().find(|cap| cap.name == name)
}

const fn wallet(name: &'static str, tier: RiskTier) -> Capability {
   Capability {
      name,
      tier,
      scope: CapabilityScope::Wallet,
      badge: None,
   }
}

const fn global(name: &'static str, tier: RiskTier) -> Capability {
   Capability {
      name,
      tier,
      scope: CapabilityScope::Global,
      badge: None,
   }
}

const fn badged(mut capability: Capability, badge: PrivacyBadge) -> Capability {
   capability.badge = Some(badge);
   capability
}
