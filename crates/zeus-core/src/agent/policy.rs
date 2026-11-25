//! The one decision an agent request gets, and the words a refusal is reported in.
//!
//! Nothing here touches a chain, a window or an HTTP request: both functions take a context for the vault
//! lock, a profile for the grant, and a plain description of what is being asked. That is what makes the
//! permission model testable without a GUI — and it is why the decision can be the last thing before an
//! action runs instead of a filter somewhere in the transport.
//!
//! The precheck answers at the request's entry, where the capability is known. The authority answers the
//! question only a simulation can: what is *actually* leaving this wallet. Both agree on the envelope
//! because the envelope hangs off the wallet, not off the capability.

use super::intent::{Asset, Intent};
use super::profile::{
   AgentProfile, CapabilityVerb, NftScope, ProfileMode, TokenRef, TokenScope, WalletGrant,
   capability,
};
use crate::tx::TransactionAnalysis;
use crate::{ZeusContext, ZeusCtx};
use std::fmt;
use zeus_eth::alloy_primitives::{Address, U256};

/// The answer to one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
   /// Run it. The flag is true when the profile asked to see every state change before it happens.
   Allowed { prompt: bool },
   /// The human has to finish this one, so the agent cannot be the one who completes it. The reason is
   /// what the announcement and the transport's user-action code say.
   NeedsUser { reason: &'static str },
   /// Refused, with the row of the rules that refused it.
   Denied(Denial),
}

/// Why a request was refused.
///
/// Each variant is one row of the enforcement rules, and the code is the stable string a client branches
/// on, so a harness never has to parse the sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Denial {
   /// The vault is locked, or was never opened in this run.
   VaultLocked,
   /// The grant does not carry the capability.
   NotGranted {
      capability: String,
      wallet: Option<Address>,
   },
   /// The wallet has no grant at all, or its own switch is off.
   WalletNotGranted { wallet: Address },
   /// The token leaving the wallet is not in its allowlist.
   TokenNotAllowed { wallet: Address, token: TokenRef },
   /// The NFT leaving the wallet is not in its allowlist.
   NftNotAllowed {
      wallet: Address,
      chain: u64,
      collection: Address,
      token_id: U256,
   },
}

impl Denial {
   /// The stable code a client branches on. Adding a variant must not rename an existing one.
   pub const fn code(&self) -> &'static str {
      match self {
         Self::VaultLocked => "VAULT_LOCKED",
         Self::NotGranted { .. } | Self::WalletNotGranted { .. } => "NOT_GRANTED",
         Self::TokenNotAllowed { .. } => "TOKEN_NOT_ALLOWED",
         Self::NftNotAllowed { .. } => "NFT_NOT_ALLOWED",
      }
   }
}

impl fmt::Display for Denial {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      match self {
         Self::VaultLocked => f.write_str("Zeus is locked"),
         Self::NotGranted {
            capability,
            wallet: Some(wallet),
         } => {
            write!(
               f,
               "the grant does not include {capability} on {wallet:#x}"
            )
         }
         Self::NotGranted {
            capability,
            wallet: None,
         } => {
            write!(f, "the grant does not include {capability}")
         }
         Self::WalletNotGranted { wallet } => {
            write!(f, "the agent has no grant on {wallet:#x}")
         }
         Self::TokenNotAllowed { wallet, token } => write!(
            f,
            "{:#x} on chain {} is not in the token allowlist for {wallet:#x}",
            token.address, token.chain
         ),
         Self::NftNotAllowed {
            wallet,
            chain,
            collection,
            token_id,
         } => write!(
            f,
            "{collection:#x} #{token_id} on chain {chain} is not in the NFT allowlist for {wallet:#x}"
         ),
      }
   }
}

/// Decide a request at its entry, before anything is built, simulated or signed.
pub fn precheck(ctx: &ZeusCtx, profile: &AgentProfile, intent: &Intent) -> Decision {
   if !vault_open(ctx) {
      return Decision::Denied(Denial::VaultLocked);
   }

   let name = intent.capability();
   let Some(entry) = capability(name) else {
      // The catalog is the answer to "may this be granted at all", so a name it does not know — a typo
      // in a tool table, a capability from a newer build — denies rather than passes.
      return Decision::Denied(Denial::NotGranted {
         capability: name.to_owned(),
         wallet: intent.wallet(),
      });
   };

   let wallet = intent.wallet();
   let granted = match wallet {
      Some(wallet) => profile.allows(wallet, name),
      None => profile.allows_global(name),
   };

   if !granted {
      return Decision::Denied(Denial::NotGranted {
         capability: name.to_owned(),
         wallet,
      });
   }

   if let Intent::AssetMove { asset, .. } = intent
      && let Some(wallet) = wallet
      && let Some(grant) = armed_grant(profile, wallet)
      && let Some(denial) = asset_denial(wallet, grant, *asset)
   {
      return Decision::Denied(denial);
   }

   match intent {
      // A key is a secret the app owner types into Zeus and a file dialog is a path only they can pick,
      // so here the grant is not enough: the agent opens the form and stops there.
      Intent::NeedsHuman { .. } => {
         return Decision::NeedsUser {
            reason: "the key is typed into Zeus by the app owner, never handed to an agent",
         };
      }
      // An export writes to the destination the grant fixed, so the human's part of it already happened
      // when the grant was made; with no destination there is nothing to write to.
      Intent::Export { .. } if profile.export_dir.is_none() => {
         return Decision::NeedsUser {
            reason: "an export needs a destination folder, chosen in Zeus",
         };
      }
      _ => {}
   }

   // A read changes nothing, so it never asks. Only a write can be made to ask, and only by the
   // profile's own mode. An export is the one write whose human step is fixed at grant time, which is
   // why it stays autonomous even here.
   let prompt = entry.verb() == CapabilityVerb::Write
      && profile.mode == ProfileMode::ConfirmEach
      && !matches!(intent, Intent::Export { .. });

   Decision::Allowed { prompt }
}

/// Decide a transaction from what the simulation says it moves.
///
/// The precheck answered "may this profile do this" at the entry, where the capability was known. This
/// answers the one question the request cannot: what *actually* leaves this wallet. It runs where every
/// broadcasting flow converges on the app's authorising prompt, so it is the gate a future caller cannot
/// route around by forgetting a check of its own.
///
/// The envelope belongs to the wallet rather than to the capability, which is why the analysis alone
/// decides it — and why a swap routed through a token the caller never named is caught here.
pub fn authorise(
   ctx: &ZeusCtx,
   profile: &AgentProfile,
   analysis: &TransactionAnalysis,
) -> Decision {
   if !vault_open(ctx) {
      return Decision::Denied(Denial::VaultLocked);
   }

   let wallet = analysis.sender;
   let Some(grant) = armed_grant(profile, wallet) else {
      return Decision::Denied(Denial::WalletNotGranted { wallet });
   };

   // The native leg comes from the value the transaction declares, not from the simulated balance: every
   // transaction pays its fee in native, so a simulated native diff would demand the native token in the
   // allowlist for an ordinary ERC-20 send. A declared value is native moving on purpose.
   if !analysis.value.is_zero() {
      let token = TokenRef {
         chain: analysis.chain,
         address: Address::ZERO,
      };
      if let Some(denial) = token_denial(wallet, grant, token) {
         return Decision::Denied(denial);
      }
   }

   for change in &analysis.balance_diff.tokens {
      if change.is_increase() {
         continue;
      }

      let token = TokenRef {
         chain: analysis.chain,
         address: change.currency.address(),
      };
      if let Some(denial) = token_denial(wallet, grant, token) {
         return Decision::Denied(denial);
      }
   }

   for change in &analysis.balance_diff.nfts {
      if change.after >= change.before {
         continue;
      }

      if let Some(denial) = nft_denial(
         wallet,
         grant,
         analysis.chain,
         change.collection,
         change.token_id,
      ) {
         return Decision::Denied(denial);
      }
   }

   Decision::Allowed {
      prompt: profile.mode == ProfileMode::ConfirmEach,
   }
}

/// Whether the vault is open: unlocked in this run, and not auto-locked since.
///
/// Both halves are one question to a caller — an agent must not act on a wallet it cannot see — so the
/// two flags are read together and a context that is neither is never mistaken for one that is.
///
/// Public because a caller that answers without a capability — the settings list, which only reports the
/// profile's own switches — still has to refuse while the app is locked, and it must ask the same
/// question the policy asks rather than reading the two flags itself.
pub fn vault_open(ctx: &ZeusCtx) -> bool {
   ctx.read(|open: &ZeusContext| open.vault_unlocked && !open.locked)
}

/// The wallet's grant, or nothing when the profile or that wallet is switched off.
fn armed_grant(profile: &AgentProfile, wallet: Address) -> Option<&WalletGrant> {
   if !profile.enabled {
      return None;
   }

   profile.wallets.get(&wallet).filter(|grant| grant.enabled)
}

fn asset_denial(wallet: Address, grant: &WalletGrant, asset: Asset) -> Option<Denial> {
   match asset {
      Asset::Token(token) => token_denial(wallet, grant, token),
      Asset::Nft {
         chain,
         collection,
         token_id,
      } => nft_denial(wallet, grant, chain, collection, token_id),
   }
}

fn token_denial(wallet: Address, grant: &WalletGrant, token: TokenRef) -> Option<Denial> {
   let allowed = match &grant.tokens {
      TokenScope::None => false,
      TokenScope::All => true,
      TokenScope::Allowlist { tokens } => tokens.contains(&token),
   };

   (!allowed).then_some(Denial::TokenNotAllowed { wallet, token })
}

fn nft_denial(
   wallet: Address,
   grant: &WalletGrant,
   chain: u64,
   collection: Address,
   token_id: U256,
) -> Option<Denial> {
   let allowed = match &grant.nfts {
      NftScope::None => false,
      NftScope::All => true,
      // A collection without ids covers every id in it: that is the shape's whole point, since an open
      // edition cannot be enumerated in a grant.
      NftScope::Allowlist { nfts } => nfts.iter().any(|nft| {
         nft.chain == chain
            && nft.collection == collection
            && nft.token_ids.as_ref().is_none_or(|ids| ids.contains(&token_id))
      }),
   };

   (!allowed).then_some(Denial::NftNotAllowed {
      wallet,
      chain,
      collection,
      token_id,
   })
}
