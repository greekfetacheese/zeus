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

use alloy_ens::ProviderEnsExt;
use alloy_network::Ethereum;
use alloy_primitives::{Address, Bytes};
use alloy_provider::{
   CcipReadClient, CcipReadGateway, CcipReadGatewayError, CcipReadRequest, Provider,
};

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
}
