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
   ClearSigningRegistry => "https://raw.githubusercontent.com/ethereum/clear-signing-erc7730-registry/master",
   // Across fee API (default; user-overridable in Bridge settings).
   AcrossSuggestedFees => "https://app.across.to/api/suggested-fees",
   // Public Pimlico bundler base (`…/{chain}/rpc`).
   PimlicoBundler => "https://public.pimlico.io/v2",
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
}
