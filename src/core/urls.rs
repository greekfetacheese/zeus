//! Catalog of the external endpoints Zeus talks to.
//!
//! Base URLs live here — not as parallel `&str` constants in the modules that
//! fetch from them. Adding an endpoint means adding a variant, and callers read
//! the value from here instead of re-spelling the host, so every destination
//! Zeus contacts is discoverable in one file.
//!
//! Parameterised endpoints (a SmolDapp icon, a Pimlico bundler per chain) hold
//! their base literal and are completed by the named builders below, so the
//! format string sits beside the literal it extends.
//!
//! Deliberately out of scope: the per-chain RPC tables in
//! [`crate::core::context::client`] (a chain-keyed table, not a flat catalog),
//! and the endpoints owned by workspace crates — `zeus-railgun`'s Subsquid /
//! POI / circuit-artifact hosts and `zeus-tokens`' Trust Wallet repo stay with
//! their crate, which is compiled without the GUI.
//!
//! Also out of scope, because the host is chosen by chain data rather than by
//! Zeus: an NFT's `tokenURI` and the image URL inside the metadata it returns
//! ([`crate::utils::nft_icon`]), and ENS record URLs — never fetched, since
//! resolution refuses CCIP-read (`zeus_eth::utils::ens`). Those hosts cannot be
//! listed, so the opt-ins that can reach them carry a caveat instead
//! ([`UrlPurpose::trailing_note`]).

use zeus_eth::alloy_primitives::Address;

macro_rules! zeus_urls {
   ($($variant:ident => $url:literal),* $(,)?) => {
      /// A base URL Zeus may contact.
      #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
      pub enum ZeusUrl {
         $($variant,)*
      }

      impl ZeusUrl {
         pub const ALL: &[Self] = &[$(Self::$variant,)*];

         /// The base endpoint.
         ///
         /// Parameterised endpoints hold the prefix a builder below extends;
         /// call the builder, not this, when a full path is needed.
         pub const fn base(self) -> &'static str {
            match self {
               $(Self::$variant => $url,)*
            }
         }
      }
   };
}

zeus_urls! {
   // SmolDapp token-icon CDN.
   SmoldappToken => "https://assets.smold.app/token",
   // IPFS gateways, tried in the order listed in `ZeusUrl::IPFS_GATEWAYS`.
   IpfsPinata => "https://gateway.pinata.cloud/ipfs",
   Ipfs4everland => "https://4everland.io/ipfs",
   IpfsIo => "https://ipfs.io/ipfs",
   IpfsDwebLink => "https://dweb.link/ipfs",
   // Arweave gateway for `ar://` art.
   Arweave => "https://arweave.net",
   // Verified-source lookups for clear signing.
   Sourcify => "https://sourcify.dev/server",
   // ERC-7730 clear-signing registry.
   // ? This is actually unused since the registry is built-in to Zeus binary.
   ClearSigningRegistry => "https://raw.githubusercontent.com/ethereum/clear-signing-erc7730-registry/master",
   // Across fee API (default; user-overridable in Bridge settings).
   AcrossSuggestedFees => "https://app.across.to/api/suggested-fees",
   // Public Pimlico bundler base (`…/{chain}/rpc`).
   PimlicoBundler => "https://public.pimlico.io/v2",
   // Railgun proving-circuit artifacts (GitHub raw).
   RailgunCircuitArtifacts => "https://github.com/greekfetacheese/privacy-protocol-artifacts/raw/refs/heads/main/artifacts",
   // Zeus releases, queried by the self-updater.
   ZeusReleases => "https://github.com/greekfetacheese/zeus",
}

impl ZeusUrl {
   /// IPFS gateways, tried in order.
   ///
   /// The order is load-bearing. `ipfs.io` and `dweb.link` answer 429
   /// ("service worker gateway only") to non-browser clients, and
   /// `cloudflare-ipfs.com` no longer resolves, so the two that answered
   /// reliably are tried first and the throttled pair is a last resort. Never
   /// trust one gateway: they pin different content, so a miss on one is not a
   /// miss overall.
   pub const IPFS_GATEWAYS: &'static [ZeusUrl] = &[
      ZeusUrl::IpfsPinata,
      ZeusUrl::Ipfs4everland,
      ZeusUrl::IpfsIo,
      ZeusUrl::IpfsDwebLink,
   ];
}

/// What an endpoint is for — lets the settings UI name exactly the hosts an
/// opt-in turns on, and forces a new endpoint to be classified at compile time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlPurpose {
   /// Token icons and NFT art (`Download Token Icons & NFT Images`).
   AssetImages,
   /// Verified contract names and ERC-7730 descriptors (`Fetch Contract Names`).
   ContractNames,
   /// Release checks (`Check for Updates`).
   Updates,
   /// The Across bridge fee API.
   Bridge,
   /// The Pimlico bundler used by Railgun operations.
   Railgun,
   /// Railgun proving-circuit artifacts (`Allow Circuit Download`).
   Circuits,
}

impl ZeusUrl {
   /// Which opt-in (if any) owns this endpoint.
   pub const fn purpose(self) -> UrlPurpose {
      match self {
         Self::SmoldappToken
         | Self::IpfsPinata
         | Self::Ipfs4everland
         | Self::IpfsIo
         | Self::IpfsDwebLink
         | Self::Arweave => UrlPurpose::AssetImages,
         Self::Sourcify | Self::ClearSigningRegistry => UrlPurpose::ContractNames,
         Self::ZeusReleases => UrlPurpose::Updates,
         Self::AcrossSuggestedFees => UrlPurpose::Bridge,
         Self::PimlicoBundler => UrlPurpose::Railgun,
         Self::RailgunCircuitArtifacts => UrlPurpose::Circuits,
      }
   }

   /// A short human label for the UI. Exhaustive, so a new endpoint cannot ship
   /// unlabelled.
   pub const fn label(self) -> &'static str {
      match self {
         Self::SmoldappToken => "Token icons - SmolDapp",
         Self::IpfsPinata => "NFT metadata - IPFS (Pinata)",
         Self::Ipfs4everland => "NFT metadata - IPFS (4EVERLAND)",
         Self::IpfsIo => "NFT metadata - IPFS (ipfs.io)",
         Self::IpfsDwebLink => "NFT metadata - IPFS (dweb.link)",
         Self::Arweave => "NFT metadata - Arweave",
         Self::Sourcify => "Verified contract names - Sourcify",
         Self::ClearSigningRegistry => "Clear-signing descriptors - ERC-7730 registry",
         Self::AcrossSuggestedFees => "Bridge fees - Across",
         Self::PimlicoBundler => "Railgun bundler - Pimlico",
         Self::RailgunCircuitArtifacts => "Railgun circuits - GitHub artifacts",
         Self::ZeusReleases => "App updates - GitHub releases",
      }
   }
}

impl UrlPurpose {
   /// A caveat appended after an opt-in's fixed endpoint list.
   ///
   /// [`UrlPurpose::AssetImages`] reaches hosts its list cannot name: an NFT's
   /// `tokenURI` and the image URL inside the metadata it returns are chosen by
   /// the collection's contract (see [`crate::utils::nft_icon`]). Every other
   /// purpose contacts only the endpoints listed for it.
   pub const fn trailing_note(self) -> Option<&'static str> {
      match self {
         Self::AssetImages => Some(
            "NFT collections may host their own metadata/images; those hosts come from the \
             collection's contract and are not listed here. Zeus only uses https and refuses \
             local or private addresses.",
         ),
         Self::ContractNames | Self::Updates | Self::Bridge | Self::Railgun | Self::Circuits => {
            None
         }
      }
   }
}

/// `intro` followed by one line per endpoint `purpose` owns — for a settings
/// hover, so the tooltip names the hosts an opt-in actually contacts.
pub fn purpose_tip(intro: &str, purpose: UrlPurpose) -> String {
   let mut tip = intro.to_string();
   let mut first = true;
   for url in ZeusUrl::ALL.iter().filter(|u| u.purpose() == purpose) {
      tip.push_str(if first { "\n\n" } else { "\n" });
      first = false;
      tip.push_str(url.label());
      tip.push_str(": ");
      tip.push_str(url.base());
   }

   if let Some(note) = purpose.trailing_note() {
      tip.push_str("\n\n");
      tip.push_str(note);
   }

   tip
}

/// The 32px SmolDapp icon URL for a token.
pub fn smoldapp_token_icon(chain_id: u64, address: Address) -> String {
   format!(
      "{}/{chain_id}/{address:#x}/logo-32.png",
      ZeusUrl::SmoldappToken.base()
   )
}

/// The default public Pimlico bundler RPC for a chain.
pub fn pimlico_bundler(chain_id: u64) -> String {
   format!(
      "{}/{chain_id}/rpc",
      ZeusUrl::PimlicoBundler.base()
   )
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn bases_are_unique() {
      let mut bases: Vec<_> = ZeusUrl::ALL.iter().map(|u| u.base()).collect();
      bases.sort_unstable();
      bases.dedup();
      assert_eq!(bases.len(), ZeusUrl::ALL.len());
   }

   #[test]
   fn builders_extend_the_catalog_base() {
      let address = Address::from([0xab; 20]);
      assert_eq!(
         smoldapp_token_icon(1, address),
         format!(
            "{}/1/{address:#x}/logo-32.png",
            ZeusUrl::SmoldappToken.base()
         )
      );
      assert_eq!(
         pimlico_bundler(10),
         format!("{}/10/rpc", ZeusUrl::PimlicoBundler.base())
      );
   }

   /// The gateway order is what makes a throttled pair a last resort, so the
   /// list must keep pinata and 4everland ahead of ipfs.io and dweb.link.
   #[test]
   fn ipfs_gateways_keep_the_reliable_pair_first() {
      assert_eq!(
         ZeusUrl::IPFS_GATEWAYS,
         &[
            ZeusUrl::IpfsPinata,
            ZeusUrl::Ipfs4everland,
            ZeusUrl::IpfsIo,
            ZeusUrl::IpfsDwebLink,
         ]
      );
   }

   #[test]
   fn labels_are_non_empty_and_unique() {
      let mut labels: Vec<_> = ZeusUrl::ALL.iter().map(|u| u.label()).collect();
      assert!(labels.iter().all(|l| !l.is_empty()));
      labels.sort_unstable();
      labels.dedup();
      assert_eq!(labels.len(), ZeusUrl::ALL.len());
   }

   #[test]
   fn purpose_tip_lists_the_endpoints_of_that_purpose() {
      let tip = purpose_tip("intro", UrlPurpose::AssetImages);
      assert!(tip.starts_with("intro\n\n"));
      assert!(tip.contains("assets.smold.app"));
      assert!(tip.contains("gateway.pinata.cloud"));
      assert!(tip.contains("arweave.net"));
      assert!(
         !tip.contains("sourcify.dev"),
         "other purposes stay out"
      );

      assert!(
         tip.contains("not listed here"),
         "the asset-image opt-in carries the unlistable-host caveat"
      );

      let circuits = purpose_tip("intro", UrlPurpose::Circuits);
      assert!(circuits.contains("privacy-protocol-artifacts"));
      assert!(
         !circuits.contains("pimlico"),
         "the bundler is a different purpose"
      );
      assert!(
         !circuits.contains("not listed here"),
         "only the asset-image opt-in has a trailing caveat"
      );
   }
}
