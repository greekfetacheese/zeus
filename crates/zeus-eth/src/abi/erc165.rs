//! ERC-165 interface detection.
//!
//! Used to tell NFT contracts apart from ERC-20s and from each other. The answers are only
//! trustworthy when [`Erc165Support::is_compliant`] holds: a contract that reports support for
//! the invalid id `0xffffffff` is lying, and a contract that *reverts* on `supportsInterface`
//! is not ERC-721 at all (CryptoPunks is the classic example). Regular ERC-20s such as WETH
//! simply answer `false` to everything.
//!
//! Gotcha when probing by hand: `bytes4` arguments are **left**-padded in ABI encoding. Sending
//! the id right-padded targets id `0x00000000`, which every contract answers `false` to — the
//! call succeeds and you get a confident wrong answer.

use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, FixedBytes, fixed_bytes};
use alloy_sol_types::sol;

sol! {
    #[sol(rpc)]
    contract IERC165 {
        function supportsInterface(bytes4 interfaceId) external view returns (bool);
    }
}

/// ERC-165 identifier — this is the `supportsInterface(bytes4)` selector itself.
pub const IERC165_ID: FixedBytes<4> = fixed_bytes!("01ffc9a7");
/// ERC-721 interface id.
pub const IERC721_ID: FixedBytes<4> = fixed_bytes!("80ac58cd");
/// ERC-721 Metadata (`name` / `symbol` / `tokenURI`) interface id.
pub const IERC721_METADATA_ID: FixedBytes<4> = fixed_bytes!("5b5e139f");
/// ERC-721 Enumerable (`totalSupply` / `tokenOfOwnerByIndex` / `tokenByIndex`) interface id.
///
/// The only way to enumerate a wallet's tokens without an indexer, and **optional** — most
/// collections do not implement it.
pub const IERC721_ENUMERABLE_ID: FixedBytes<4> = fixed_bytes!("780e9d63");
/// ERC-1155 interface id.
pub const IERC1155_ID: FixedBytes<4> = fixed_bytes!("d9b67a26");
/// ERC-1155 `Metadata_URI` (`uri(uint256)`) interface id.
///
/// Equal to the `uri(uint256)` selector because the interface has a single function. Note
/// ERC-1155 has no standard `name()` / `symbol()` — those are ERC-721 Metadata only.
///
/// **Do not use this as a gate for calling `uri()`.** In practice collections implement `uri()`
/// without advertising the interface: the OpenSea shared storefront returns a real URI for
/// `uri(1)` while answering `false` here. Prefer calling `uri()` and tolerating a revert.
pub const IERC1155_METADATA_ID: FixedBytes<4> = fixed_bytes!("0e89341c");
/// ERC-5216, the ERC-1155 allowance extension (`approve` / `allowance` by `id` and amount).
///
/// Equal to the XOR of `approve(address,uint256,uint256)` and
/// `allowance(address,address,uint256)` — see the test below, which derives it rather than
/// trusting this line.
pub const IERC5216_ID: FixedBytes<4> = fixed_bytes!("1be07d74");
/// ERC-7604, the ERC-1155 `permit` extension — a **draft**, not live as of 2026-10.
///
/// The value is the one the ERC *declares* contracts must answer with. It is deliberately not
/// derived: the XOR of the three selectors the ERC lists
/// (`permit`/`nonces`/`DOMAIN_SEPARATOR`) is `0x29011db4`, which does **not** match the
/// declared id — the draft is internally inconsistent. Since a future implementing contract
/// would register the declared value, that is the one to probe with; a derivation test
/// pins the discrepancy so nobody "fixes" it into disagreement with real contracts.
pub const IERC1155_PERMIT_ID: FixedBytes<4> = fixed_bytes!("7409106d");
/// Must return false on a spec-compliant ERC-165 contract.
pub const INVALID_INTERFACE_ID: FixedBytes<4> = fixed_bytes!("ffffffff");

/// One `supportsInterface` call.
///
/// A **revert** reads as `false`: a contract that does not implement ERC-165 reverts here (or answers
/// `false`), and either way it does not support the interface. An address with **no code** answers `0x`
/// instead of reverting, which alloy reports as [`alloy_contract::Error::ZeroData`] — also `false`, and
/// for the same reason: it does not implement the interface, and "this is not an NFT contract" is the
/// answer, not a failure. A transport failure is *not* an answer and stays an error — the same
/// distinction [`crate::nft::verify_ownership`] draws, and for the same reason: an RPC outage must never
/// be reported to the user as "this is not an NFT".
pub async fn supports_interface<P, N>(
   client: P,
   token: Address,
   interface_id: FixedBytes<4>,
) -> Result<bool, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let contract = IERC165::new(token, client);
   match contract.supportsInterface(interface_id).call().await {
      Ok(supported) => Ok(supported),
      Err(err) if err.as_revert_data().is_some() => Ok(false),
      Err(alloy_contract::Error::ZeroData(..)) => Ok(false),
      Err(err) => Err(err.into()),
   }
}

/// Every ERC-165 answer we care about for one address, from a single [`probe`] sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Erc165Support {
   /// The contract implements `supportsInterface` at all.
   pub erc165: bool,
   /// Reported support for the invalid id `0xffffffff` — a compliant contract says `false`.
   pub invalid_id_supported: bool,
   pub erc721: bool,
   pub erc721_metadata: bool,
   pub erc721_enumerable: bool,
   pub erc1155: bool,
   pub erc1155_metadata: bool,
   /// ERC-5216 allowance extension on an ERC-1155 collection.
   pub erc5216: bool,
   /// ERC-7604 permit extension — a draft, so expect `false` everywhere for now.
   pub erc1155_permit: bool,
}

impl Erc165Support {
   /// Whether the contract implements `supportsInterface` at all.
   pub fn is_erc165(&self) -> bool {
      self.erc165
   }

   /// Whether the ERC-165 answers can be trusted at all.
   ///
   /// A contract that does not implement ERC-165, or that claims the invalid id, is not treated
   /// as an NFT even if some other bit happens to be set.
   pub fn is_compliant(&self) -> bool {
      self.erc165 && !self.invalid_id_supported
   }

   pub fn is_erc721(&self) -> bool {
      self.is_compliant() && self.erc721
   }

   pub fn is_erc1155(&self) -> bool {
      self.is_compliant() && self.erc1155
   }

   /// ERC-721 *or* ERC-1155 — the gate every NFT code path should check first.
   pub fn is_nft(&self) -> bool {
      self.is_erc721() || self.is_erc1155()
   }

   /// ERC-721 Metadata: `name()` / `symbol()` / `tokenURI()` are safe to call.
   pub fn is_erc721_metadata(&self) -> bool {
      self.is_erc721() && self.erc721_metadata
   }

   /// ERC-721 Enumerable: `tokenOfOwnerByIndex()` is safe to call, so the collection can be
   /// enumerated without an indexer.
   pub fn is_erc721_enumerable(&self) -> bool {
      self.is_erc721() && self.erc721_enumerable
   }

   /// ERC-1155 Metadata: `uri(uint256)` is advertised as available.
   ///
   /// Advisory only — a `false` does not mean `uri()` is absent (see [`IERC1155_METADATA_ID`]).
   pub fn is_erc1155_metadata(&self) -> bool {
      self.is_erc1155() && self.erc1155_metadata
   }

   /// ERC-5216 allowance extension: `allowance(account, operator, id)` is safe to call, so a
   /// per-`id` approval can be read rather than inferred from an event.
   pub fn is_erc5216(&self) -> bool {
      self.is_erc1155() && self.erc5216
   }

   /// ERC-7604 permit extension. Advisory, and expected to be `false` until the draft ships —
   /// reading approvals never needs it, since a permit emits the ERC-5216 `Approval` event.
   pub fn is_erc1155_permit(&self) -> bool {
      self.is_erc1155() && self.erc1155_permit
   }
}

/// Probe every interface id we care about for `token`.
///
/// The calls run sequentially on purpose: this crate has no `tokio` dependency, and the sweeps
/// are short enough that the extra round trips do not matter next to the RPC latency.
///
/// `Err` means the contract could not be *asked* — never that it answered "no". A caller that wants to
/// treat an unreachable node as "not an NFT" has to say so itself, which is what keeps a hiccup from
/// being shown to the user as a verdict about the contract.
pub async fn probe<P, N>(client: P, token: Address) -> Result<Erc165Support, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let erc165 = supports_interface(client.clone(), token, IERC165_ID).await?;
   let invalid_id_supported =
      supports_interface(client.clone(), token, INVALID_INTERFACE_ID).await?;
   let erc721 = supports_interface(client.clone(), token, IERC721_ID).await?;
   let erc721_metadata = supports_interface(client.clone(), token, IERC721_METADATA_ID).await?;
   let erc721_enumerable = supports_interface(client.clone(), token, IERC721_ENUMERABLE_ID).await?;
   let erc1155 = supports_interface(client.clone(), token, IERC1155_ID).await?;
   let erc1155_metadata = supports_interface(client.clone(), token, IERC1155_METADATA_ID).await?;
   let erc5216 = supports_interface(client.clone(), token, IERC5216_ID).await?;
   let erc1155_permit = supports_interface(client, token, IERC1155_PERMIT_ID).await?;

   Ok(Erc165Support {
      erc165,
      invalid_id_supported,
      erc721,
      erc721_metadata,
      erc721_enumerable,
      erc1155,
      erc1155_metadata,
      erc5216,
      erc1155_permit,
   })
}

/// Whether `token` is an ERC-721 or ERC-1155 contract.
///
/// Convenience wrapper around [`probe`] for callers that only need the yes/no answer — with the same
/// meaning for `Err`: the contract could not be asked, which is not the same as "no".
pub async fn is_erc721_or_erc1155<P, N>(client: P, token: Address) -> Result<bool, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   Ok(probe(client, token).await?.is_nft())
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::address;
   use alloy_provider::ProviderBuilder;
   use alloy_sol_types::SolCall;

   #[test]
   fn interface_ids_match_the_published_values() {
      assert_eq!(IERC165_ID.0, [0x01, 0xff, 0xc9, 0xa7]);
      assert_eq!(IERC721_ID.0, [0x80, 0xac, 0x58, 0xcd]);
      assert_eq!(IERC721_METADATA_ID.0, [0x5b, 0x5e, 0x13, 0x9f]);
      assert_eq!(IERC721_ENUMERABLE_ID.0, [0x78, 0x0e, 0x9d, 0x63]);
      assert_eq!(IERC1155_ID.0, [0xd9, 0xb6, 0x7a, 0x26]);
      assert_eq!(IERC1155_METADATA_ID.0, [0x0e, 0x89, 0x34, 0x1c]);
      assert_eq!(IERC5216_ID.0, [0x1b, 0xe0, 0x7d, 0x74]);
      assert_eq!(IERC1155_PERMIT_ID.0, [0x74, 0x09, 0x10, 0x6d]);
   }

   /// An ERC-165 interface id is the XOR of the selectors of its functions. Deriving
   /// `IERC5216_ID` from the generated selectors checks the constant against the ABI we
   /// actually decode with — a copied hex string cannot drift from the declarations.
   #[test]
   fn erc5216_id_is_derivable_from_its_selectors() {
      use crate::abi::erc1155::IERC5216;
      use alloy_sol_types::SolCall;

      let approve = u32::from_be_bytes(IERC5216::approveCall::SELECTOR);
      let allowance = u32::from_be_bytes(IERC5216::allowanceCall::SELECTOR);

      assert_eq!(
         FixedBytes::<4>::from((approve ^ allowance).to_be_bytes()),
         IERC5216_ID
      );
   }

   /// ERC-7604 is a **draft, and its declared interface id does not match its own functions**:
   /// XOR-ing the three selectors it lists gives `0x29011db4`, not the `0x7409106d` it tells
   /// contracts to register.
   ///
   /// This test pins both values. The declared one is what we probe with (a real contract
   /// answers what the ERC told it to register), and the mismatch is recorded so a future
   /// reader does not "correct" the constant into disagreeing with every implementing
   /// contract. If the ERC is ever fixed, this test fails loudly — which is the point.
   #[test]
   fn erc7604_declared_id_does_not_match_its_own_selectors() {
      use crate::abi::erc1155::IERC1155Permit;
      use alloy_sol_types::SolCall;

      let permit = u32::from_be_bytes(IERC1155Permit::permitCall::SELECTOR);
      let nonces = u32::from_be_bytes(IERC1155Permit::noncesCall::SELECTOR);
      let domain = u32::from_be_bytes(IERC1155Permit::DOMAIN_SEPARATORCall::SELECTOR);

      let derived = permit ^ nonces ^ domain;
      assert_eq!(
         derived, 0x2901_1db4,
         "the selectors the ERC-7604 text lists"
      );

      assert_ne!(
         FixedBytes::<4>::from(derived.to_be_bytes()),
         IERC1155_PERMIT_ID,
         "if these now agree, the draft was corrected upstream — re-read ERC-7604 and update"
      );
      assert_eq!(IERC1155_PERMIT_ID.0, [0x74, 0x09, 0x10, 0x6d]);
   }

   /// The extensions must only ever be trusted on an ERC-1155 that is itself trustworthy:
   /// a non-compliant contract claiming ERC-5216 or ERC-7604 is not an NFT at all.
   #[test]
   fn extensions_require_a_compliant_erc1155() {
      let mut support = Erc165Support {
         erc165: true,
         erc1155: true,
         erc5216: true,
         erc1155_permit: true,
         ..Default::default()
      };
      assert!(support.is_erc5216());
      assert!(support.is_erc1155_permit());

      // Claims the invalid id — nothing it says can be trusted.
      support.invalid_id_supported = true;
      assert!(!support.is_erc5216());
      assert!(!support.is_erc1155_permit());

      // An ERC-721 answering `erc5216` (garbage, but possible) is not an allowance extension.
      support.invalid_id_supported = false;
      support.erc1155 = false;
      support.erc721 = true;
      assert!(!support.is_erc5216());
      assert!(!support.is_erc1155_permit());
   }

   /// Two of these ids are *defined* as selectors: ERC-165 is the `supportsInterface(bytes4)`
   /// selector, and `Metadata_URI` has a single function so its id is the `uri(uint256)` selector.
   /// Derive them instead of trusting two hand-written tables to agree.
   #[test]
   fn ids_are_derivable_from_their_selectors() {
      assert_eq!(
         IERC165_ID.0,
         IERC165::supportsInterfaceCall::SELECTOR
      );
      assert_eq!(
         IERC1155_METADATA_ID.0,
         crate::abi::erc1155::IERC1155Metadata::uriCall::SELECTOR
      );
   }

   /// A contract that does not implement ERC-165 — or that claims the invalid id — must never be
   /// classified as an NFT, even when it answers `true` for `IERC721_ID`.
   #[test]
   fn non_compliant_answers_are_never_an_nft() {
      let no_erc165 = Erc165Support {
         erc721: true,
         ..Default::default()
      };
      assert!(!no_erc165.is_erc721());
      assert!(!no_erc165.is_nft());

      let lies = Erc165Support {
         erc165: true,
         invalid_id_supported: true,
         erc721: true,
         erc1155: true,
         ..Default::default()
      };
      assert!(!lies.is_compliant());
      assert!(!lies.is_erc721());
      assert!(!lies.is_erc1155());
      assert!(!lies.is_nft());
   }

   /// The optional ERC-721 interfaces only count alongside ERC-721 itself.
   #[test]
   fn optional_interfaces_require_their_core_interface() {
      let orphans = Erc165Support {
         erc165: true,
         erc721_metadata: true,
         erc721_enumerable: true,
         ..Default::default()
      };
      assert!(!orphans.is_erc721_metadata());
      assert!(!orphans.is_erc721_enumerable());

      let bayc_like = Erc165Support {
         erc165: true,
         erc721: true,
         erc721_metadata: true,
         erc721_enumerable: true,
         ..Default::default()
      };
      assert!(bayc_like.is_nft());
      assert!(bayc_like.is_erc721_enumerable());
      assert!(!bayc_like.is_erc1155());
   }

   /// Live check against the real contracts these rules were derived from.
   ///
   /// Ignored by default. It needs an endpoint that serves `eth_call`, and public ones are
   /// unreliable from non-browser clients and rot often — the endpoint the sibling `erc20` tests
   /// use is currently down. Point it at a keyed endpoint with `ZEUS_ETH_RPC` and run on demand:
   ///
   /// ```text
   /// ZEUS_ETH_RPC=<keyed url> cargo test -p zeus-eth --lib abi::erc165 -- --ignored --nocapture
   /// ```
   ///
   /// Expected (verified against mainnet when these rules were written): BAYC = ERC-721 +
   /// metadata + enumerable; OpenSea shared storefront = ERC-1155 and not ERC-721; WETH = no
   /// ERC-165 at all; CryptoPunks = reverts, so "not an NFT" rather than an error.
   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn probes_real_mainnet_contracts() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());

      // BAYC — ERC-721 plus both optional interfaces.
      let bayc = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
      let s = probe(client.clone(), bayc).await.expect("the sweep reached the node");
      assert!(
         s.is_compliant(),
         "BAYC should be ERC-165 compliant"
      );
      assert!(s.is_erc721(), "BAYC is ERC-721");
      assert!(s.is_erc721_metadata(), "BAYC exposes tokenURI");
      assert!(s.is_erc721_enumerable(), "BAYC is enumerable");
      assert!(!s.is_erc1155(), "BAYC is not ERC-1155");

      // OpenSea shared storefront — ERC-1155, and not ERC-721.
      let storefront = address!("495f947276749Ce646f68AC8c248420045cb7b5e");
      let s = probe(client.clone(), storefront).await.expect("the sweep reached the node");
      assert!(s.is_nft(), "storefront is an NFT contract");
      assert!(s.is_erc1155(), "storefront is ERC-1155");
      assert!(!s.is_erc721(), "storefront is not ERC-721");

      // WETH — a plain ERC-20: answers `false` to everything and is not an NFT.
      let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
      let s = probe(client.clone(), weth).await.expect("the sweep reached the node");
      assert!(!s.is_erc165(), "WETH implements no ERC-165");
      assert!(
         !s.is_nft(),
         "WETH must never be treated as an NFT"
      );

      // CryptoPunks — reverts on every probe, so it reads as "not an NFT" rather than erroring.
      let punks = address!("b47e3cd837dDF8e4c57F05d70Ab865de6e193BBB");
      let s = probe(client, punks).await.expect("the sweep reached the node");
      assert!(
         !s.is_compliant(),
         "Punks does not implement ERC-165"
      );
      assert!(!s.is_nft(), "Punks must not be treated as an NFT");
   }

   /// A probe that cannot reach a node is an **error**, never a "no".
   ///
   /// At an unreachable host every `supportsInterface` call fails to arrive; reading that as `false`
   /// would make the sweep report every contract as "not an NFT", which is how a pasted collection
   /// becomes "… is not an NFT contract" during an RPC hiccup. `.invalid` is a reserved TLD that never
   /// resolves ([RFC 2606](https://www.rfc-editor.org/rfc/rfc2606)), so this fails by transport rather
   /// than by a contract reverting — the case the sweep must not swallow.
   #[tokio::test]
   async fn an_unreachable_node_is_an_error_not_a_false_answer() {
      let client =
         ProviderBuilder::new().connect_http("http://zeus.invalid:8545".parse().expect("a url"));

      let result = probe(
         client,
         address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D"),
      )
      .await;

      assert!(
         result.is_err(),
         "an unreachable node is not an answer about the contract: {result:?}"
      );
   }
}
