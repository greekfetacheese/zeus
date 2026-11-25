//! What an agent asks for, reduced to what the policy needs to decide it.
//!
//! A request arrives as a tool call, a JSON body and a capability string from a table in another crate.
//! None of that belongs in the decision, so every caller flattens its request into an Intent first: which
//! capability, which wallet, and — when the request names one — which asset. The variants are shaped by
//! the *question*, not by the catalog: a swap and a send are the same question here (may this asset leave
//! this wallet) while a portfolio read asks a different one (may this profile see this wallet at all). The
//! capability string is what tells two otherwise identical shapes apart.

use super::profile::TokenRef;
use zeus_eth::alloy_primitives::{Address, U256};

/// One asset a request names.
///
/// It carries exactly the pair an allowlist is keyed by, so the check is an equality test rather than a
/// lookup that could be mistaken for one: a chain and an address for a fungible asset, a chain and a
/// collection and an id for an NFT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asset {
   /// A fungible asset, native included — the native token is the zero address.
   Token(TokenRef),
   /// One id of one collection.
   Nft {
      chain: u64,
      collection: Address,
      token_id: U256,
   },
}

/// One request, as the policy sees it.
///
/// The capability is a catalog string rather than an enum on purpose: these strings are on disk in the
/// grant document and in every harness's conversation history, so the string is the identity. A name this
/// build does not know grants nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
   /// Reading something about one wallet.
   WalletRead {
      capability: &'static str,
      wallet: Address,
   },
   /// Changing something about one wallet without moving value.
   WalletWrite {
      capability: &'static str,
      wallet: Address,
   },
   /// Moving, or giving away control of, an asset in one wallet: the envelope applies.
   AssetMove {
      capability: &'static str,
      wallet: Address,
      asset: Asset,
   },
   /// Reading the app as a whole.
   GlobalRead { capability: &'static str },
   /// Changing app state that belongs to no wallet.
   GlobalWrite { capability: &'static str },
   /// A write only the human can finish: a key typed into Zeus's own form, an archive chosen in a file
   /// dialog. The agent may open the form and no more.
   NeedsHuman { capability: &'static str },
   /// An export, which runs on its own once the grant names a destination folder.
   Export { capability: &'static str },
}

impl Intent {
   /// The catalog capability this request needs.
   pub const fn capability(self) -> &'static str {
      match self {
         Self::WalletRead { capability, .. }
         | Self::WalletWrite { capability, .. }
         | Self::AssetMove { capability, .. }
         | Self::GlobalRead { capability }
         | Self::GlobalWrite { capability }
         | Self::NeedsHuman { capability }
         | Self::Export { capability } => capability,
      }
   }

   /// The wallet the request acts on, or nothing for one that acts on the app as a whole.
   pub const fn wallet(self) -> Option<Address> {
      match self {
         Self::WalletRead { wallet, .. }
         | Self::WalletWrite { wallet, .. }
         | Self::AssetMove { wallet, .. } => Some(wallet),
         Self::GlobalRead { .. }
         | Self::GlobalWrite { .. }
         | Self::NeedsHuman { .. }
         | Self::Export { .. } => None,
      }
   }
}
