//! ENS name resolution for Zeus.
//!
//! **Policy: mainnet only, onchain only.**
//!
//! Zeus never issues a network request outside its own RPC clients, and
//! `alloy-ens` is not neutral about that: its convenience helpers `resolve_name`
//! and `lookup_address` are implemented as
//! `*_with_ccip_read(name, shared_http_ccip_read_client())`, so any resolver that
//! answers with an ERC-3668 `OffchainLookup` makes them fetch a
//! *contract-controlled* HTTPS URL. This module therefore never calls those
//! helpers: every lookup goes through the `*_with_ccip_read` variants with
//! [`NoOffchainGateway`], which refuses every redirect. A name that can only be
//! answered offchain is reported as "not found" instead of leaking a request to a
//! third party.
//!
//! [`ProviderEnsExt`] is imported here and nowhere else in the workspace, so no
//! other module can reach the HTTP-gateway default by accident.
//!
//! Name normalization is [`normalize_name`] — the ENSIP-15 subset Zeus can
//! validate on its own, without vendoring Unicode tables.
//!
//! **Expiry.** An expired `.eth` name keeps resolving to its last records — the
//! registry owner, the resolver and the resolver's `addr` all survive expiry — so a
//! name can change hands while a wallet is still running. Only the [`.eth`
//! BaseRegistrar](BASE_REGISTRAR_ADDRESS) knows when a registration ends, and
//! [`name_expiry`] reads it. A binding is trustworthy only while
//! `now < takeover_at`: `expires_at` plus the grace period, after which a third
//! party can register the name. See [`NameExpiry`].

use alloy_ens::{ProviderEnsExt, try_dns_encode};
use alloy_network::Ethereum;
use alloy_primitives::{Address, Bytes, U256, keccak256};
use alloy_provider::{
   CcipReadClient, CcipReadGateway, CcipReadGatewayError, CcipReadRequest, Provider,
};
use alloy_sol_types::{SolCall, sol};

/// ENS contracts live on Ethereum mainnet, and the registry / universal resolver
/// addresses `alloy-ens` uses are the mainnet ones, so a lookup must go through a
/// mainnet client no matter which chain the user is currently viewing.
pub const ENS_CHAIN: u64 = crate::types::ETH;

/// Longest single label in bytes (DNS limit, enforced by ENSIP-15).
const MAX_LABEL_LEN: usize = 63;

/// Longest whole name in bytes (DNS limit, enforced by ENSIP-15).
const MAX_NAME_LEN: usize = 255;

/// Refuses every ERC-3668 (CCIP Read) redirect.
///
/// This is the point of the module: it reduces "resolve this name" to a strictly
/// on-chain operation, and makes offchain-only names fail loudly rather than
/// silently reach a third party.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoOffchainGateway;

#[async_trait::async_trait]
impl CcipReadGateway for NoOffchainGateway {
   async fn request(
      &self,
      request: &CcipReadRequest,
      _max_response_size: usize,
   ) -> Result<Bytes, CcipReadGatewayError> {
      Err(CcipReadGatewayError::new(format!(
         "offchain ENS resolution is disabled ({} gateway url(s) offered)",
         request.urls.len()
      )))
   }
}

/// The client every lookup in this module runs through.
fn onchain_only() -> CcipReadClient<NoOffchainGateway> {
   CcipReadClient::new(NoOffchainGateway)
}

/// Why a string cannot be used as an ENS name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
   /// Nothing left after trimming.
   Empty,
   /// Longer than [`MAX_NAME_LEN`] bytes.
   TooLong,
   /// A label is empty (leading, trailing, or doubled dot).
   EmptyLabel,
   /// A label is longer than [`MAX_LABEL_LEN`] bytes.
   LabelTooLong,
   /// A character outside `[a-z0-9_-]`.
   UnsupportedChar(char),
   /// A label starts or ends with `-`.
   HyphenEdge,
}

impl std::fmt::Display for NameError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         NameError::Empty => write!(f, "empty name"),
         NameError::TooLong => write!(f, "longer than {} bytes", MAX_NAME_LEN),
         NameError::EmptyLabel => write!(f, "empty label"),
         NameError::LabelTooLong => write!(f, "label longer than {} bytes", MAX_LABEL_LEN),
         NameError::UnsupportedChar(c) => write!(f, "unsupported character {:?}", c),
         NameError::HyphenEdge => write!(f, "label starts or ends with '-'"),
      }
   }
}

impl std::error::Error for NameError {}

/// Normalize a user-typed ENS name — the ENSIP-15 subset Zeus can verify itself.
///
/// Full ENSIP-15 needs the Unicode confusable / whole-script tables (a ~2 MB
/// dependency) so, instead of guessing, Zeus accepts exactly what it can validate
/// on its own and rejects everything else:
///
/// * surrounding whitespace is trimmed (people paste),
/// * ASCII uppercase is lowered (ENSIP-15 case folding),
/// * labels are `[a-z0-9_-]` and may not start or end with `-`,
/// * empty labels, and the 63 / 255 byte DNS limits, are enforced,
/// * any non-ASCII character is rejected, which also rejects the confusable,
///   combining and zero-width characters that make homograph names dangerous.
///
/// A rejected name must never be resolved. `alloy-ens` hashes whatever it is given,
/// so an un-normalized name silently hashes to a *different* name — for example
/// `"Vitalik.eth"` resolves to the zero address as a "success".
pub fn normalize_name(input: &str) -> Result<String, NameError> {
   let trimmed = input.trim();

   if trimmed.is_empty() {
      return Err(NameError::Empty);
   }

   if trimmed.len() > MAX_NAME_LEN {
      return Err(NameError::TooLong);
   }

   let mut normalized = String::with_capacity(trimmed.len());

   for (index, label) in trimmed.split('.').enumerate() {
      if label.is_empty() {
         return Err(NameError::EmptyLabel);
      }

      if label.len() > MAX_LABEL_LEN {
         return Err(NameError::LabelTooLong);
      }

      if label.starts_with('-') || label.ends_with('-') {
         return Err(NameError::HyphenEdge);
      }

      if index > 0 {
         normalized.push('.');
      }

      for ch in label.chars() {
         match ch {
            'a'..='z' | '0'..='9' | '-' | '_' => normalized.push(ch),
            'A'..='Z' => normalized.push(ch.to_ascii_lowercase()),
            other => return Err(NameError::UnsupportedChar(other)),
         }
      }
   }

   Ok(normalized)
}

/// Is this search-bar text worth an ENS round-trip?
///
/// Allocation-free and cheap on purpose: it runs on the egui frame path for every
/// keystroke while the search box is open. [`normalize_name`] stays the authority
/// on validity — this is only the gate that decides whether to spend an RPC call.
///
/// It rejects hex addresses, dotless queries (contact / token searches), and a
/// trailing dot (a name still being typed).
pub fn looks_like_name(query: &str) -> bool {
   let query = query.trim();

   query.len() >= 4
      && query.contains('.')
      && !query.starts_with("0x")
      && !query.starts_with("0X")
      && !query.ends_with('.')
      && query
         .bytes()
         .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// Forward resolution: ENS name to address, onchain only.
///
/// `Ok(None)` means "no address": the name did not survive [`normalize_name`], or
/// it has no `addr` record. `alloy-ens` reports a name with no `addr` record as a
/// *successful* lookup of [`Address::ZERO`], so that has to be turned into "not
/// found" here — otherwise the zero address would be offered as a recipient.
///
/// `Err` means "could not ask" (no such name, revert, refused offchain gateway,
/// transport problem). It is not a hard failure for the caller: a half-typed name
/// is expected to land there, so callers show nothing rather than an error.
pub async fn resolve_name<P>(client: &P, name: &str) -> Result<Option<Address>, alloy_ens::EnsError>
where
   P: Provider<Ethereum>,
{
   let Ok(name) = normalize_name(name) else {
      return Ok(None);
   };

   let address = client.resolve_name_with_ccip_read(&name, &onchain_only()).await?;

   Ok((address != Address::ZERO).then_some(address))
}

/// Reverse resolution: address to its primary ENS name, onchain only.
///
/// The Universal Resolver already checks that the returned primary name resolves
/// back to `address`, so this is a verified name rather than whatever the reverse
/// record happens to claim. On top of that the name must survive
/// [`normalize_name`] *unchanged*: a name that only normalizes into something
/// different (non-ASCII, mixed case) is dropped instead of displayed, because Zeus
/// cannot detect confusables.
///
/// `Ok(None)` means "no primary name" — the resolver answers with an empty string,
/// which is the common case for EOAs and contracts alike.
pub async fn lookup_name<P>(
   client: &P,
   address: &Address,
) -> Result<Option<String>, alloy_ens::EnsError>
where
   P: Provider<Ethereum>,
{
   if address.is_zero() {
      return Ok(None);
   }

   let name = client.lookup_address_with_ccip_read(address, &onchain_only()).await?;

   if name.is_empty() {
      return Ok(None);
   }

   match normalize_name(&name) {
      Ok(normalized) if normalized == name => Ok(Some(name)),
      _ => Ok(None),
   }
}

/// ENS `.eth` BaseRegistrar on Ethereum mainnet (ERC-721, `BaseRegistrarImplementation`).
///
/// [`EnsBaseRegistrar::nameExpires`] is the **only** onchain source of a name's registration end:
/// the registry owner, the resolver and the resolver's records all survive expiry, so an expired
/// name keeps resolving to its last records. Only the registrar knows when the registration ends.
pub const BASE_REGISTRAR_ADDRESS: Address =
   alloy_primitives::address!("0x57f1887a8BF19b14fC0dF6Fd9B2acc9Af147eA85");

/// `.eth` grace period in seconds, matching [`EnsBaseRegistrar::GRACE_PERIOD`] (90 days).
///
/// Only a fallback: the live value is read once per process. It bounds the window after expiry
/// during which nobody can register the name and its owner cannot transfer it away, so it is what
/// turns a registration end into a *takeover* time.
pub const GRACE_PERIOD_SECS: u64 = 90 * 24 * 60 * 60;

/// The registrable label of a `.eth` name — the label immediately left of `eth`.
///
/// `alice.eth` → `alice`; `sub.alice.eth` → `alice`, because a subdomain is controlled by its
/// registrable parent and it is the parent's registration that can lapse. `None` for any name
/// whose TLD is not `eth`, and for bare `eth` — those have no onchain expiry to read.
pub fn registrable_label(name: &str) -> Option<&str> {
   let mut labels = name.rsplit('.');

   if !labels.next()?.eq_ignore_ascii_case("eth") {
      return None;
   }

   labels.next().filter(|label| !label.is_empty())
}

/// When a name's `name ↔ address` binding stops being trustworthy.
///
/// A binding may be trusted only while `now < takeover_at`. This is deliberately **not** just
/// `expires_at`: during the 90-day grace period after expiry nobody can register the name and its
/// owner cannot even transfer it, so the binding is still the owner's. The takeover window — the
/// Temporary Premium auction, then open availability — only opens at `expires_at + grace`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameExpiry {
   /// Registration end, `BaseRegistrar.nameExpires`.
   pub expires_at: u64,
   /// First moment a third party can take the name over: `expires_at + grace period`.
   pub takeover_at: u64,
}

impl NameExpiry {
   /// Is the `name ↔ address` binding still trustworthy at `now`?
   pub fn is_trusted(&self, now: u64) -> bool {
      now < self.takeover_at
   }
}

sol! {
   /// ENS `.eth` BaseRegistrar. `alloy-ens` has no registrar binding and only the registrar knows
   /// when a registration ends.
   #[sol(rpc)]
   contract EnsBaseRegistrar {
      /// Unix timestamp at which the registration of `id` (a label's `keccak256`) ends. `0` when
      /// the label was never registered.
      function nameExpires(uint256 id) external view returns (uint256);

      /// Seconds after expiry during which the name cannot be registered by anyone, but its owner
      /// can still renew it and ownership cannot change.
      function GRACE_PERIOD() external view returns (uint256);
   }
}

/// The `.eth` grace period, read from the registrar once per process and cached.
///
/// It is a contract constant (90 days today), so one read is enough; the cache keeps the
/// per-lookup cost at a single extra `eth_call`. Falls back to [`GRACE_PERIOD_SECS`] if the read
/// fails, which only shifts the trust window, never inverts it.
async fn grace_period<P>(client: &P) -> u64
where
   P: Provider<Ethereum>,
{
   use std::sync::atomic::{AtomicU64, Ordering};

   static CACHE: AtomicU64 = AtomicU64::new(0);

   let cached = CACHE.load(Ordering::Relaxed);

   if cached != 0 {
      return cached;
   }

   match EnsBaseRegistrar::new(BASE_REGISTRAR_ADDRESS, client)
      .GRACE_PERIOD()
      .call()
      .await
   {
      Ok(seconds) => {
         // A value that does not fit a `u64` is not a grace period. Fall back to the constant
         // rather than clamping, so a broken or hostile answer cannot widen the trust window.
         if seconds > U256::from(u64::MAX) {
            tracing::warn!("ens: GRACE_PERIOD read is out of range, using the constant");
            return GRACE_PERIOD_SECS;
         }

         let seconds = seconds.to::<u64>();
         if seconds != 0 {
            CACHE.store(seconds, Ordering::Relaxed);
         }
         seconds
      }
      Err(e) => {
         tracing::warn!("ens: GRACE_PERIOD read failed: {:?}", e);
         GRACE_PERIOD_SECS
      }
   }
}

/// Read a name's registration expiry, onchain only.
///
/// Only registrable `.eth` second-level names have an onchain expiry. For anything else — a
/// non-`.eth` TLD, bare `eth` — the answer is `Ok(None)`: "no expiry is known", which callers must
/// treat as *unverifiable*, never as *expired*. A subdomain (`sub.alice.eth`) is bounded by its
/// registrable parent (`alice.eth`).
pub async fn name_expiry<P>(
   client: &P,
   name: &str,
) -> Result<Option<NameExpiry>, alloy_ens::EnsError>
where
   P: Provider<Ethereum>,
{
   let Ok(name) = normalize_name(name) else {
      return Ok(None);
   };

   let Some(label) = registrable_label(&name) else {
      return Ok(None);
   };

   let labelhash = keccak256(label.as_bytes());

   let expires = EnsBaseRegistrar::new(BASE_REGISTRAR_ADDRESS, client)
      .nameExpires(U256::from_be_bytes(labelhash.0))
      .call()
      .await
      .map_err(alloy_ens::EnsError::Resolve)?;

   if expires.is_zero() {
      return Ok(None);
   }

   // A registration end too large for a `u64` can only be an unpayable duration or a lying RPC (a
   // real one is bounded by what anyone could pay to renew), so it reads as "registered well beyond
   // any horizon" — trusted, which is what a value that large means. `saturating_to` keeps that
   // from being a panic where `to` would be; a plausible future timestamp is trusted regardless.
   let expires_at = expires.saturating_to::<u64>();
   let takeover_at = expires_at.saturating_add(grace_period(client).await);

   Ok(Some(NameExpiry {
      expires_at,
      takeover_at,
   }))
}

/// Forward resolution plus the name's expiry, failing closed on the expiry.
///
/// The expiry is **not** optional for a name that has one: if it cannot be read the whole lookup
/// fails, so an address whose name may already have lapsed is never offered. Only a name with no
/// expiry to read — a non-`.eth` name — comes back as `Ok(Some((address, None)))`. `Ok(None)` means
/// "no address", as in [`resolve_name`].
pub async fn resolve_name_with_expiry<P>(
   client: &P,
   name: &str,
) -> Result<Option<(Address, Option<NameExpiry>)>, alloy_ens::EnsError>
where
   P: Provider<Ethereum>,
{
   let Some(address) = resolve_name(client, name).await? else {
      return Ok(None);
   };

   // Refusing here, rather than resolving with `None`, is the point: an unreadable expiry must
   // never be mistaken for "this name cannot expire".
   let expiry = match name_expiry(client, name).await {
      Ok(expiry) => expiry,
      Err(e) => {
         tracing::warn!(
            "ens: expiry lookup failed for {:?}, refusing to resolve: {:?}",
            name,
            e
         );
         return Err(e);
      }
   };

   Ok(Some((address, expiry)))
}

/// ENSIP-24 data record key holding a chain's ERC-7930 *Interoperable Address*.
pub const INTEROPERABLE_ADDRESS_KEY: &str = "interoperable-address";

/// The namespace chain labels live under: `base` → `base.on.eth`.
pub const CHAIN_REGISTRY_SUFFIX: &str = "on.eth";

/// The ENSIP-19 coin type of the "default EVM chain" record (`chainFromCoinType` = 0).
const DEFAULT_EVM_COIN_TYPE: u64 = 0x8000_0000;

sol! {
   /// ENSIP-24: Arbitrary Data Resolution.
   ///
   /// `alloy-ens` has no binding for this, so it is declared here. It is never called directly —
   /// `resolve_chain_label` sends it through the Universal Resolver, which is the only way a
   /// wildcard (ENSIP-10) namespace answers.
   interface IInteroperableAddressResolver {
      function data(bytes32 node, string key) external view returns (bytes);
   }
}

/// An address resolved for one specific chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainAddress {
   pub address: Address,
   /// True when the address came from the name's ENSIP-19 *default EVM chain* record rather than
   /// one set for this exact chain — a weaker claim, and the UI should say so.
   ///
   /// Best effort: a resolver that implements the default substitution itself returns the value
   /// from the per-chain call, so it reports `false` even though the address is the default one.
   pub from_default_evm_record: bool,
   /// The name's registration expiry. Chain-independent — the same registration whichever coin
   /// type the address came from. `None` for a name with no onchain expiry (non-`.eth`); a name
   /// that *has* one never reaches here with `None`, because an unreadable expiry fails the whole
   /// lookup (see [`resolve_name_for_chain`]).
   pub expiry: Option<NameExpiry>,
}

/// Resolve a chain label under `on.eth` to its EIP-155 chain id, onchain only.
///
/// The `on.eth` resolver is wildcard-only (ENSIP-10) and **reverts** on a direct call, so this
/// goes through the Universal Resolver's `resolve(bytes name, bytes data)` — the flow the ENS blog
/// and ERC-7828 both describe. The inner call is ENSIP-24 `data(node, "interoperable-address")`,
/// and the bytes it returns are an ERC-7930 *Interoperable Address* whose `ChainReference` is the
/// chain id.
///
/// The Universal Resolver is called **without** a [`CcipReadClient`], so an ERC-3668
/// `OffchainLookup` revert surfaces as a plain error and the lookup fails closed — the same
/// guarantee [`NoOffchainGateway`] gives every other lookup in this module.
///
/// `Ok(None)` means "no such label": the resolver returned empty, the label is not one Zeus can
/// validate, or the chain is not an EVM chain.
pub async fn resolve_chain_label<P>(
   client: &P,
   label: &str,
) -> Result<Option<u64>, alloy_ens::EnsError>
where
   P: Provider<Ethereum>,
{
   // Same rule as a name: the label becomes `<label>.on.eth`, and alloy-ens hashes whatever it is
   // given, so an un-normalized label would resolve a *different* chain.
   let Ok(label) = normalize_name(label) else {
      return Ok(None);
   };

   // A label, not a name — `base.on.eth` must not become `base.on.eth.on.eth`.
   if label.contains('.') {
      return Ok(None);
   }

   let name = format!("{}.{}", label, CHAIN_REGISTRY_SUFFIX);
   let encoded_name = try_dns_encode(&name)?;

   let call = IInteroperableAddressResolver::dataCall {
      node: alloy_ens::namehash(&name),
      key: INTEROPERABLE_ADDRESS_KEY.to_string(),
   };

   let resolved = alloy_ens::UniversalResolver::new(alloy_ens::UNIVERSAL_RESOLVER_ADDRESS, client)
      .resolve(encoded_name.into(), call.abi_encode().into())
      .call()
      .await
      .map_err(alloy_ens::EnsError::Resolve)?;

   let raw = IInteroperableAddressResolver::dataCall::abi_decode_returns(&resolved._0)
      .map_err(|_| alloy_ens::EnsError::InvalidResponse)?;

   Ok(super::interoperable_name::decode_chain_id(&raw).ok())
}

/// Forward resolution for **one specific EVM chain**, onchain only.
///
/// Uses the chain's ENSIP-11 coin type (`0x8000_0000 | chainId`, or `60` for mainnet) rather than
/// mainnet's `addr` record. A chain with no record returns **empty bytes**, never `addr(60)`:
/// silently substituting the mainnet address is the exact ambiguity ERC-7828 exists to remove, and
/// ENSIP-19 ("Deprecating Mainnet as Default") tells clients to stop doing it.
///
/// ENSIP-19 also specifies that `addr(node, coinType)` should fall back to
/// `addr(node, 0x8000_0000)` — the name's *default EVM chain* address, valid on every EVM chain.
/// Most resolvers do not implement that substitution yet, so it is applied here explicitly and
/// reported through [`ChainAddress::from_default_evm_record`].
///
/// `Ok(None)` means the name has neither an address for this chain nor a default EVM address.
pub async fn resolve_name_for_chain<P>(
   client: &P,
   name: &str,
   chain_id: u64,
) -> Result<Option<ChainAddress>, alloy_ens::EnsError>
where
   P: Provider<Ethereum>,
{
   let Ok(name) = normalize_name(name) else {
      return Ok(None);
   };

   let Some(coin_type) = alloy_ens::coin_type::evm_chain(chain_id) else {
      return Ok(None);
   };

   let (address, from_default_evm_record) =
      match resolve_evm_coin_type(client, &name, coin_type).await? {
         Some(address) => (address, false),
         None if coin_type != DEFAULT_EVM_COIN_TYPE => {
            match resolve_evm_coin_type(client, &name, DEFAULT_EVM_COIN_TYPE).await? {
               Some(address) => (address, true),
               None => return Ok(None),
            }
         }
         None => return Ok(None),
      };

   // The name's expiry is orthogonal to the coin type, and like [`resolve_name_with_expiry`] it is
   // not optional: an unreadable expiry fails the lookup rather than resolving to "no expiry".
   let expiry = match name_expiry(client, &name).await {
      Ok(expiry) => expiry,
      Err(e) => {
         tracing::warn!(
            "ens: expiry lookup failed for {:?}, refusing to resolve: {:?}",
            name,
            e
         );
         return Err(e);
      }
   };

   Ok(Some(ChainAddress {
      address,
      from_default_evm_record,
      expiry,
   }))
}

/// `addr(node, coinType)` for an EVM coin type.
///
/// `alloy-ens` hands back the resolver's raw bytes: empty means "no record", and anything that is
/// not 20 bytes is not an EVM address.
async fn resolve_evm_coin_type<P>(
   client: &P,
   name: &str,
   coin_type: u64,
) -> Result<Option<Address>, alloy_ens::EnsError>
where
   P: Provider<Ethereum>,
{
   let raw = client
      .resolve_name_for_coin_type_with_ccip_read(name, coin_type, &onchain_only())
      .await?;

   if raw.is_empty() {
      return Ok(None);
   }

   if raw.len() != 20 {
      tracing::warn!(
         "ens: {} returned {} bytes for coin type {}, expected 20",
         name,
         raw.len(),
         coin_type
      );
      return Ok(None);
   }

   Ok(Some(Address::from_slice(&raw)))
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn normalizes_ascii_names() {
      assert_eq!(
         normalize_name("vitalik.eth").unwrap(),
         "vitalik.eth"
      );
      assert_eq!(
         normalize_name("Vitalik.ETH").unwrap(),
         "vitalik.eth"
      );
      assert_eq!(
         normalize_name("  vitalik.eth  ").unwrap(),
         "vitalik.eth"
      );
      assert_eq!(
         normalize_name("a-b.c_d.eth").unwrap(),
         "a-b.c_d.eth"
      );
      assert_eq!(
         normalize_name("1.offchainexample.eth").unwrap(),
         "1.offchainexample.eth"
      );
      assert_eq!(normalize_name("vitalik").unwrap(), "vitalik");
   }

   #[test]
   fn rejects_what_it_cannot_validate() {
      assert_eq!(normalize_name(""), Err(NameError::Empty));
      assert_eq!(normalize_name("   "), Err(NameError::Empty));
      assert_eq!(
         normalize_name("vitalik..eth"),
         Err(NameError::EmptyLabel)
      );
      assert_eq!(
         normalize_name(".vitalik.eth"),
         Err(NameError::EmptyLabel)
      );
      assert_eq!(
         normalize_name("vitalik.eth."),
         Err(NameError::EmptyLabel)
      );
      assert_eq!(
         normalize_name("-vitalik.eth"),
         Err(NameError::HyphenEdge)
      );
      assert_eq!(
         normalize_name("vitalik-.eth"),
         Err(NameError::HyphenEdge)
      );
      assert_eq!(
         normalize_name("vitalik eth"),
         Err(NameError::UnsupportedChar(' '))
      );
      // Homographs and other Unicode never reach a hash.
      assert_eq!(
         normalize_name("vital\u{456}k.eth"),
         Err(NameError::UnsupportedChar('\u{456}'))
      );
      assert_eq!(
         normalize_name("\u{1F4A9}.eth"),
         Err(NameError::UnsupportedChar('\u{1F4A9}'))
      );
      assert_eq!(
         normalize_name("vitalik.eth\u{FE0F}"),
         Err(NameError::UnsupportedChar('\u{FE0F}'))
      );
      assert_eq!(
         normalize_name(&"a".repeat(64)).unwrap_err(),
         NameError::LabelTooLong
      );
   }

   #[test]
   fn name_gate_matches_what_can_be_resolved() {
      assert!(looks_like_name("vitalik.eth"));
      assert!(looks_like_name(" Vitalik.ETH "));
      assert!(!looks_like_name("vitalik"));
      assert!(!looks_like_name("vitalik."));
      assert!(!looks_like_name(
         "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"
      ));
      assert!(!looks_like_name("vitalik eth"));
   }

   /// The chain-label call is byte-for-byte the one that was sent to mainnet when the `on.eth`
   /// registry was verified — the Universal Resolver answered it with chain 8453.
   ///
   /// This is the hermetic half of the live test: it pins the ABI encoding and the DNS encoding
   /// against a real mainnet response, so the only thing left unverified without a network is the
   /// transport itself.
   #[test]
   fn chain_label_calldata_matches_what_mainnet_answered() {
      let name = "base.on.eth";

      assert_eq!(
         try_dns_encode(name).unwrap(),
         alloy_primitives::hex::decode("0462617365026f6e0365746800").unwrap()
      );

      let call = IInteroperableAddressResolver::dataCall {
         node: alloy_ens::namehash(name),
         key: INTEROPERABLE_ADDRESS_KEY.to_string(),
      };

      // `data(bytes32,string)` for namehash("base.on.eth") + "interoperable-address".
      // selector(ecbfada3) | node | offset(0x40) | length(0x15) | "interoperable-address"
      let expected = alloy_primitives::hex::decode(concat!(
         "ecbfada3",
         "40d581c1d53524df6850c4e63f8967a97f00b3c1deac30319fbb166a276d60f7",
         "0000000000000000000000000000000000000000000000000000000000000040",
         "0000000000000000000000000000000000000000000000000000000000000015",
         "696e7465726f70657261626c652d616464726573730000000000000000000000",
      ))
      .unwrap();

      assert_eq!(call.abi_encode(), expected);
   }

   /// Decode the exact bytes mainnet returned for `ethereum.on.eth` and `base.on.eth`.
   #[test]
   fn decodes_the_chain_registrys_answers() {
      use crate::utils::interoperable_name::decode_chain_id;

      // These are the ERC-7930 values the mainnet registry returned (and what ERC-7828 prints).
      assert_eq!(
         decode_chain_id(&alloy_primitives::hex::decode("00010000010100").unwrap()).unwrap(),
         1
      );
      assert_eq!(
         decode_chain_id(&alloy_primitives::hex::decode("00010000022105").unwrap()).unwrap(),
         8453
      );
      assert_eq!(
         decode_chain_id(&alloy_primitives::hex::decode("00010000010a00").unwrap()).unwrap(),
         10
      );
      assert_eq!(
         decode_chain_id(&alloy_primitives::hex::decode("0001000002a4b1").unwrap()).unwrap(),
         42161
      );
   }

   #[test]
   fn registrable_label_is_the_second_level_under_eth() {
      assert_eq!(registrable_label("alice.eth"), Some("alice"));
      assert_eq!(registrable_label("sub.alice.eth"), Some("alice"));
      assert_eq!(registrable_label("a.b.alice.eth"), Some("alice"));
      assert_eq!(registrable_label("alice.xyz"), None);
      assert_eq!(registrable_label("eth"), None);
      assert_eq!(registrable_label("alice.eth."), None);
   }

   /// Pins the registrar ABI against what mainnet answered. `nameExpires` takes the *label's*
   /// `keccak256`, not the namehash, and its selector is the one the live read used.
   #[test]
   fn registrar_calldata_is_pinned() {
      assert_eq!(
         EnsBaseRegistrar::nameExpiresCall::SELECTOR,
         alloy_primitives::hex!("d6e4fa86")
      );

      // `keccak256("vitalik")` is the token id the registrar answered for vitalik.eth.
      assert_eq!(
         keccak256("vitalik").0,
         alloy_primitives::hex!("af2caa1c2ca1d027f1ac823b529d0a67cd144264b2789fa2ea4d63a67c7103cc")
      );
   }

   #[test]
   fn name_expiry_trusts_through_the_grace_period() {
      let expiry = NameExpiry {
         expires_at: 1_000,
         takeover_at: 1_000 + GRACE_PERIOD_SECS,
      };

      assert!(expiry.is_trusted(999)); // still registered
      assert!(expiry.is_trusted(1_000)); // just expired, still in grace
      assert!(expiry.is_trusted(expiry.takeover_at - 1)); // last second of grace
      assert!(!expiry.is_trusted(expiry.takeover_at)); // takeover window opens
   }
}
