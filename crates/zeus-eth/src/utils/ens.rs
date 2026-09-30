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

use alloy_ens::{ProviderEnsExt, try_dns_encode};
use alloy_network::Ethereum;
use alloy_primitives::{Address, Bytes};
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

   if let Some(address) = resolve_evm_coin_type(client, &name, coin_type).await? {
      return Ok(Some(ChainAddress {
         address,
         from_default_evm_record: false,
      }));
   }

   if coin_type != DEFAULT_EVM_COIN_TYPE
      && let Some(address) = resolve_evm_coin_type(client, &name, DEFAULT_EVM_COIN_TYPE).await?
   {
      return Ok(Some(ChainAddress {
         address,
         from_default_evm_record: true,
      }));
   }

   Ok(None)
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
}
