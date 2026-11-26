//! The fund-moving pipeline: one order of guards, one outcome, one record.
//!
//! A money tool does five things and the order is the point. The vault's lock, the wallet's lock, the
//! grant with the asset envelope, the funds screen, then the flow: each guard is cheaper than the one after
//! it and stops what the next would do, so a refusal costs nothing and a refusal after the flow costs a
//! simulation, a signature or a prompt.
//!
//! What this module does **not** do is build a transaction. That is the flow's, behind [`TxFlows`], because
//! the flows are private functions of the binary crate; the pipeline is the guard order, the outcome and
//! the record. A dry run is the same module with the flow left out: the pipeline's simulation stage
//! ([`crate::tx::analyse`]) plus the policy check over what it measured, and an answer that says it is a
//! preview.
//!
//! The wallet lock lives here rather than in the edge because it is not an edge concern: the contract it
//! keeps is "two state-changing actions never build the same nonce", true of any caller. The user's own
//! flows do not take it (D20), so the guarantee is "serialized against other agents" — which is what the
//! autonomous mode needs.

use super::audit::{AuditDecision, AuditEntry};
use super::env::AgentEnv;
use super::intent::{Asset, Intent};
use super::policy::{Decision, Denial, announce_denial, authorise, precheck, vault_open};
use super::profile::{AgentProfile, ProfileMode, TokenRef};
use super::read;
use crate::sim::approval_diff::ApprovalKind;
use crate::tx::{
   PriorityTier, TransactionAnalysis, TransactionRich, analyse, ensure_recipient_is_not_contract,
   price_analysis,
};
use crate::{ZeusCtx, ui};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use zeus_eth::abi::{erc20, erc721, erc1155};
use zeus_eth::alloy_primitives::{Address, Bytes, U256};
use zeus_eth::nft::NftStandard;
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

/// What a self-broadcast costs, in the words the app's own switch carries.
///
/// A copy, because that switch is a binary module this one cannot name — the same reason and the same risk
/// as `MAX_SLIPPAGE_BPS` — so the swap page's tip and this must move together.
pub const SELF_BROADCAST_NOTICE: &str = "Submits the unshield from your public wallet. Breaks anonymity \
                                        only use if private broadcast is unavailable.";

/// How long a second action waits for the wallet before it is told `BUSY`.
///
/// Short on purpose: an action can be held by a confirmation the user has not answered yet, and an agent
/// waiting behind someone else's prompt looks hung. The answer it needs is "this wallet is in use".
const WALLET_WAIT: Duration = Duration::from_millis(500);

/// One send of a fungible asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendRequest {
   pub chain: u64,
   pub from: Address,
   pub to: Address,
   /// `None` sends the chain's own coin.
   pub token: Option<Address>,
   /// Exactly what the chain moves, already scaled by `decimals`.
   pub amount: U256,
   /// The token's decimals, for the readable form the flow builds and the answer carries.
   pub decimals: u8,
   pub priority: PriorityTier,
}

/// One NFT transfer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NftSendRequest {
   pub chain: u64,
   pub from: Address,
   pub to: Address,
   pub collection: Address,
   pub token_id: U256,
   pub standard: NftStandard,
   /// One for ERC-721, the token count for ERC-1155.
   pub amount: U256,
   pub priority: PriorityTier,
}

/// One EIP-7702 delegation change.
///
/// There is no preview of one: the authorization it carries is a signature, so a dry run would be a depth
/// that believes it measured a transaction it cannot build. A call that asks for one is told that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegateRequest {
   pub chain: u64,
   pub from: Address,
   /// The implementation to delegate to. The zero address removes the delegation.
   pub delegate_to: Address,
   pub priority: PriorityTier,
}

/// One Uniswap swap.
///
/// The allowlisted asset is the one that `currency_in` names: it is what leaves the wallet. The quote is
/// the route the caller was shown, so the swap it signs is the one it described to its user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapRequest {
   pub chain: u64,
   pub from: Address,
   pub currency_in: Address,
   pub currency_out: Address,
   /// Exactly what the chain moves, already scaled by `decimals_in`.
   pub amount_in: U256,
   /// The input token's decimals.
   pub decimals_in: u8,
   /// How far the output may fall below the quote, in basis points.
   pub slippage_bps: u16,
   /// How long the transaction stays valid, in minutes.
   pub deadline: u64,
   pub priority: PriorityTier,
}

/// One Across bridge.
///
/// The recipient lives on the destination chain, which is why the pipeline screens it there and refuses a
/// bridge whose destination chain it cannot read: a recipient it cannot check is one it cannot vouch for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeRequest {
   pub chain: u64,
   pub dest_chain: u64,
   pub from: Address,
   pub to: Address,
   /// The wrapped token the bridge deposits, on the source chain.
   pub token: Address,
   /// Exactly what the chain moves, already scaled by `decimals`.
   pub amount: U256,
   pub decimals: u8,
   pub priority: PriorityTier,
}

/// One way into the private pool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShieldRequest {
   pub chain: u64,
   pub from: Address,
   /// The fungible token to shield, or — with `nft` — the collection.
   pub token: Address,
   pub amount: U256,
   pub decimals: u8,
   /// The id and the standard when an NFT is what moves.
   pub nft: Option<NftMovement>,
   /// Where the private note goes: the zk address the transfer is addressed to.
   pub recipient: String,
   pub priority: PriorityTier,
}

/// Which id of a collection moves, and how it is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NftMovement {
   pub token_id: U256,
   pub standard: NftStandard,
}

/// One way out of the private pool, through one of the two broadcast paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnshieldRequest {
   pub chain: u64,
   pub from: Address,
   /// The fungible token to unshield, or — with `nft` — the collection.
   pub token: Address,
   pub amount: U256,
   pub decimals: u8,
   pub nft: Option<NftMovement>,
   /// The public address on the other side.
   pub to: Address,
   pub broadcast: Broadcast,
   /// Unwrap WETH into ETH on the way out. A fungible choice: an NFT has nothing to unwrap.
   pub unwrap_to_eth: bool,
   /// Attached to the transfer when one is said.
   pub memo: Option<String>,
   pub priority: PriorityTier,
}

/// Which lane an unshield takes, and the bundler it takes it through.
///
/// The two are not one choice: the stored URL is what the page remembers between launches, and the
/// broadcast is picked per unshield. A call that names the URL the app already holds is not an override.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Broadcast {
   /// Submit from the wallet itself rather than through the paymaster. Links the sender to the recipient.
   pub self_broadcast: bool,
   /// The bundler the sponsored path submits through; `None` is the one the app has stored.
   pub bundler_url: Option<String>,
}

/// One merge of the wallet's own notes: a private self-transfer, so no value leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeNotesRequest {
   pub chain: u64,
   pub from: Address,
   pub currency: Address,
   pub amount: U256,
   pub decimals: u8,
   pub priority: PriorityTier,
}

/// What one swap would produce, as the router priced it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SwapQuote {
   /// Exactly what the route would produce, in the output token's smallest unit.
   pub amount_out: U256,
   /// The output token's decimals, so the amount can be read without another call.
   pub decimals_out: u8,
   /// True when the pair is one coin and its own wrapper: the amounts are one to one and there is no route.
   pub wrapped: bool,
   pub steps: Vec<QuoteStep>,
}

/// One hop of a quoted route, as `quote_swap` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteStep {
   /// The pool the hop trades in.
   pub pool: Address,
   /// The pool version: 2, 3 or 4.
   pub version: u8,
   /// The fee tier, in hundredths of a bip: 3000 is the 0.3% tier.
   pub fee: u32,
   /// A V4 pool's tick spacing.
   pub tick_spacing: i32,
   /// A V4 pool's hooks contract; the zero address is a pool with no hooks.
   pub hooks: Address,
   /// What this hop takes in, in the pool's input token.
   pub amount_in: U256,
   /// What this hop produces, in the pool's output token.
   pub amount_out: U256,
}

/// One Uniswap quote: what a swap would produce, priced now and never broadcast.
///
/// It is the swap's own request minus the fee tier, because nothing is sent: the caller asks what the same
/// call would get, which is also why a quote is a read rather than a flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteRequest {
   pub chain: u64,
   pub from: Address,
   pub currency_in: Address,
   pub currency_out: Address,
   pub amount_in: U256,
   pub decimals_in: u8,
   pub slippage_bps: u16,
   pub priority: PriorityTier,
}

/// One approval to clear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevokeRequest {
   pub chain: u64,
   pub owner: Address,
   pub target: RevokeTarget,
   pub priority: PriorityTier,
}

/// Which approval, in its own shape.
///
/// The three on-chain shapes are not interchangeable — `setApprovalForAll` against a grant made with a
/// per-token `approve` revokes nothing and still costs gas — so the caller states the shape rather than
/// leaving the flow to guess one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevokeTarget {
   /// An ERC-20 or Permit2 allowance, cleared by `approve(spender, 0)`.
   Token { token: Address, spender: Address },
   /// An NFT approval. `token_id` absent is the collection-wide `ApprovalForAll`; present is one id, and
   /// then `standard` says how that id is cleared.
   Nft {
      collection: Address,
      operator: Address,
      token_id: Option<U256>,
      standard: Option<NftStandard>,
   },
}

/// One fund-moving action, as the pipeline sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecCall {
   Send(SendRequest),
   Nft(NftSendRequest),
   Revoke(RevokeRequest),
   Delegate(DelegateRequest),
   Swap(SwapRequest),
   Bridge(BridgeRequest),
   Shield(ShieldRequest),
   Unshield(UnshieldRequest),
   MergeNotes(MergeNotesRequest),
}

impl ExecCall {
   /// The catalog capability the action needs.
   pub const fn capability(&self) -> &'static str {
      match self {
         Self::Send(_) => "wallet.write.send",
         Self::Nft(_) => "wallet.write.nft_send",
         Self::Revoke(_) => "wallet.write.approvals_revoke",
         Self::Delegate(_) => "wallet.write.delegate",
         Self::Swap(_) => "wallet.write.swap",
         Self::Bridge(_) => "wallet.write.bridge",
         Self::Shield(_) => "wallet.write.shield",
         Self::Unshield(_) => "wallet.write.unshield",
         Self::MergeNotes(_) => "wallet.write.notes_merge",
      }
   }

   pub const fn chain(&self) -> u64 {
      match self {
         Self::Send(request) => request.chain,
         Self::Nft(request) => request.chain,
         Self::Revoke(request) => request.chain,
         Self::Delegate(request) => request.chain,
         Self::Swap(request) => request.chain,
         Self::Bridge(request) => request.chain,
         Self::Shield(request) => request.chain,
         Self::Unshield(request) => request.chain,
         Self::MergeNotes(request) => request.chain,
      }
   }

   /// The fee tier the call asked for, carried to the flow through the environment.
   const fn priority(&self) -> PriorityTier {
      match self {
         Self::Send(request) => request.priority,
         Self::Nft(request) => request.priority,
         Self::Revoke(request) => request.priority,
         Self::Delegate(request) => request.priority,
         Self::Swap(request) => request.priority,
         Self::Bridge(request) => request.priority,
         Self::Shield(request) => request.priority,
         Self::Unshield(request) => request.priority,
         Self::MergeNotes(request) => request.priority,
      }
   }

   /// The wallet whose nonce the action would use.
   pub const fn wallet(&self) -> Address {
      match self {
         Self::Send(request) => request.from,
         Self::Nft(request) => request.from,
         Self::Revoke(request) => request.owner,
         Self::Delegate(request) => request.from,
         Self::Swap(request) => request.from,
         Self::Bridge(request) => request.from,
         Self::Shield(request) => request.from,
         Self::Unshield(request) => request.from,
         Self::MergeNotes(request) => request.from,
      }
   }

   /// The policy's view of the request: which asset leaves which wallet, or nothing for a revoke.
   fn intent(&self) -> Intent {
      match self {
         Self::Send(request) => Intent::AssetMove {
            capability: self.capability(),
            wallet: request.from,
            asset: Asset::Token(TokenRef {
               chain: request.chain,
               address: request.token.unwrap_or(Address::ZERO),
            }),
         },
         Self::Nft(request) => Intent::AssetMove {
            capability: self.capability(),
            wallet: request.from,
            asset: Asset::Nft {
               chain: request.chain,
               collection: request.collection,
               token_id: request.token_id,
            },
         },
         Self::Revoke(request) => Intent::WalletWrite {
            capability: self.capability(),
            wallet: request.owner,
         },
         Self::Delegate(request) => Intent::WalletWrite {
            capability: self.capability(),
            wallet: request.from,
         },
         // Every one of the five moves an asset the envelope has to allow: a swap spends its input, a
         // bridge spends the wrapped token it deposits, and the three private calls spend what they name.
         Self::Swap(request) => Intent::AssetMove {
            capability: self.capability(),
            wallet: request.from,
            asset: Asset::Token(TokenRef {
               chain: request.chain,
               address: request.currency_in,
            }),
         },
         Self::Bridge(request) => Intent::AssetMove {
            capability: self.capability(),
            wallet: request.from,
            asset: Asset::Token(TokenRef {
               chain: request.chain,
               address: request.token,
            }),
         },
         Self::Shield(request) => Intent::AssetMove {
            capability: self.capability(),
            wallet: request.from,
            asset: nft_asset(request.chain, request.token, request.nft),
         },
         // What an unshield gives up lives in the private pool, which no public allowlist describes, so
         // the asset here is the one it hands to the public side.
         Self::Unshield(request) => Intent::AssetMove {
            capability: self.capability(),
            wallet: request.from,
            asset: nft_asset(request.chain, request.token, request.nft),
         },
         // Merging a wallet's own notes spends nothing: it rewrites them where they already are, so the
         // asset envelope has nothing to decide.
         Self::MergeNotes(request) => Intent::WalletWrite {
            capability: self.capability(),
            wallet: request.from,
         },
      }
   }

   /// The recipient a funds screen is about, when the action has one.
   ///
   /// A bridge's recipient is deliberately absent: it lives on the destination chain, which the pipeline
   /// cannot read without being told which one, so the screen is asked for separately.
   const fn recipient(&self) -> Option<Address> {
      match self {
         Self::Send(request) => Some(request.to),
         Self::Nft(request) => Some(request.to),
         Self::Unshield(request) => Some(request.to),
         Self::Revoke(_)
         | Self::Delegate(_)
         | Self::Swap(_)
         | Self::Bridge(_)
         | Self::Shield(_)
         | Self::MergeNotes(_) => None,
      }
   }

   fn chain_id(&self) -> Result<ChainId, ExecError> {
      ChainId::new(self.chain()).map_err(|_| {
         ExecError::Invalid(format!(
            "chain {} is not one Zeus supports",
            self.chain()
         ))
      })
   }

   /// What the call would put on chain: the contract it goes to, its calldata and its value.
   ///
   /// A dry run needs this and nothing else from the flow — which is why the pipeline can preview an action
   /// it cannot perform itself.
   fn target(&self) -> Result<(Address, Bytes, U256), ExecError> {
      match self {
         Self::Send(request) => Ok(match request.token {
            None => (request.to, Bytes::new(), request.amount),
            Some(token) => (
               token,
               erc20::encode_transfer(request.to, request.amount),
               U256::ZERO,
            ),
         }),
         Self::Nft(request) => {
            let call_data = match request.standard {
               NftStandard::Erc721 => {
                  if request.amount != U256::from(1) {
                     return Err(ExecError::Invalid(
                        "an ERC-721 token moves one id, so amount has to be 1".to_owned(),
                     ));
                  }

                  erc721::encode_safe_transfer_from(request.from, request.to, request.token_id)
               }
               NftStandard::Erc1155 => erc1155::encode_safe_transfer_from(
                  request.from,
                  request.to,
                  request.token_id,
                  request.amount,
                  Bytes::new(),
               ),
            };

            Ok((request.collection, call_data, U256::ZERO))
         }
         Self::Revoke(request) => {
            let (target, call_data) = match &request.target {
               RevokeTarget::Token { token, spender } => (
                  *token,
                  erc20::encode_approve(*spender, U256::ZERO),
               ),
               RevokeTarget::Nft {
                  collection,
                  operator,
                  token_id,
                  standard,
               } => {
                  let call_data = match (token_id, standard) {
                     (None, _) => erc721::encode_set_approval_for_all(*operator, false),
                     (Some(id), Some(NftStandard::Erc721)) => {
                        erc721::encode_approve(Address::ZERO, *id)
                     }
                     (Some(id), Some(NftStandard::Erc1155)) => {
                        erc1155::encode_approve(*operator, *id, U256::ZERO)
                     }
                     (Some(_), None) => {
                        return Err(ExecError::Invalid(
                           "a per-token revoke needs the collection's standard: erc721 or erc1155"
                              .to_owned(),
                        ));
                     }
                  };

                  (*collection, call_data)
               }
            };

            Ok((target, call_data, U256::ZERO))
         }
         // The authorization a delegation carries is a signature, so there is nothing to simulate: the
         // mutating flow signs it, and the pipeline only ever routes the call.
         Self::Delegate(_) => Err(ExecError::Invalid(
            "a delegation cannot be previewed: the authorization it carries is a signature, so there is \
             nothing to simulate"
               .to_owned(),
         )),
         // Each of these builds its transaction inside its flow, from state only the flow can read: the
         // route from the pool manager, the deposit from the Across API, the proof from the private state.
         Self::Swap(_) | Self::Bridge(_) | Self::Shield(_) | Self::Unshield(_) | Self::MergeNotes(_) => {
            Err(ExecError::Invalid(
               "this action cannot be previewed: its transaction is built inside its flow, from state \
                only the flow can read"
                  .to_owned(),
            ))
         }
      }
   }
}

/// What a private call is about: one id when it named one, the fungible token otherwise.
fn nft_asset(chain: u64, token: Address, nft: Option<NftMovement>) -> Asset {
   match nft {
      Some(nft) => Asset::Nft {
         chain,
         collection: token,
         token_id: nft.token_id,
      },
      None => Asset::Token(TokenRef {
         chain,
         address: token,
      }),
   }
}

/// Why a fund-moving action did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecError {
   /// Zeus is locked, or was never unlocked in this run.
   Locked,
   /// Another action on this wallet is in flight, and did not finish in time.
   Busy,
   /// The user declined the confirmation.
   Rejected,
   /// The profile wants to see every change, and this run has no window to ask in.
   NoUi,
   /// The request cannot describe an action: no amount, a wrong decimals count, a shape that does not exist.
   Invalid(String),
   /// The grant does not carry it: the capability, the wallet or the asset.
   Denied(Denial),
   /// This build cannot perform the action at all.
   Unavailable,
   /// The flow started and refused: the simulation, the signature or the node.
   Failed(String),
}

impl ExecError {
   /// The stable code, as the audit record carries it and a client branches on it.
   pub const fn code(&self) -> &'static str {
      match self {
         Self::Locked => "VAULT_LOCKED",
         Self::Busy => "BUSY",
         Self::Rejected => "USER_REJECTED",
         Self::NoUi => "USER_ACTION_REQUIRED",
         Self::Invalid(_) => "VALIDATION",
         Self::Denied(denial) => denial.code(),
         Self::Unavailable => "UNSUPPORTED",
         Self::Failed(_) => "INTERNAL",
      }
   }
}

impl fmt::Display for ExecError {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      match self {
         Self::Locked => f.write_str("Zeus is locked"),
         Self::Busy => f.write_str("another action on this wallet is in flight"),
         Self::Rejected => f.write_str("the user declined the confirmation"),
         Self::NoUi => f.write_str(ui::NO_UI_MSG),
         Self::Invalid(reason) => f.write_str(reason),
         Self::Denied(denial) => write!(f, "{denial}"),
         Self::Unavailable => f.write_str("this build cannot send transactions"),
         Self::Failed(reason) => f.write_str(reason),
      }
   }
}

impl std::error::Error for ExecError {}

impl From<Decision> for ExecError {
   fn from(decision: Decision) -> Self {
      match decision {
         // A caller converts only a decision that refused; an allow is a decision not to convert.
         Decision::Allowed { .. } => {
            Self::Failed("an allowed request was reported as a refusal".to_owned())
         }
         Decision::NeedsUser { reason } => Self::Failed(reason.to_owned()),
         Decision::Denied(denial) => Self::Denied(denial),
      }
   }
}

/// A flow's answer, boxed: `async fn` in a trait is not dyn-compatible, and the pipeline holds it as
/// `&dyn TxFlows`.
pub type TxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A quote's answer, boxed like every other port future.
pub type QuoteFuture<'a> = Pin<Box<dyn Future<Output = Result<SwapQuote, ExecError>> + Send + 'a>>;

/// The flows the pipeline drives.
///
/// Implemented by the binary, which owns them — the same seam `zeus_mcp::AgentOps` is, one crate further in,
/// because the guard order is the same for every edge and the flow is not. A flow **screens its own funds
/// recipient first** (`send_eth` and `send_token` both do); the NFT flow never has, and the pipeline is what
/// screens it.
///
/// Every method refuses by default, so a build that installs no flows still answers instead of hanging.
pub trait TxFlows: Send + Sync + 'static {
   fn send(
      &self,
      env: AgentEnv,
      request: SendRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let _ = (env, request);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   fn send_nft(
      &self,
      env: AgentEnv,
      request: NftSendRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let _ = (env, request);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   fn revoke(
      &self,
      env: AgentEnv,
      request: RevokeRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let _ = (env, request);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   fn delegate(
      &self,
      env: AgentEnv,
      request: DelegateRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let _ = (env, request);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   fn swap(
      &self,
      env: AgentEnv,
      request: SwapRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let _ = (env, request);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   fn bridge(
      &self,
      env: AgentEnv,
      request: BridgeRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let _ = (env, request);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   fn shield(
      &self,
      env: AgentEnv,
      request: ShieldRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let _ = (env, request);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   fn unshield(
      &self,
      env: AgentEnv,
      request: UnshieldRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let _ = (env, request);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   fn merge_notes(
      &self,
      env: AgentEnv,
      request: MergeNotesRequest,
   ) -> TxFuture<'_, Result<TransactionRich, ExecError>> {
      let _ = (env, request);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   /// Price a swap without making one, and answer the route it would take.
   ///
   /// A quote is not an action: it builds no transaction, so it takes no wallet lock and leaves no record.
   fn quote(&self, request: QuoteRequest) -> QuoteFuture<'_> {
      let _ = request;

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   /// Whether an address can receive funds on one chain.
   ///
   /// The app's own screen, through the port because it reads a chain the pipeline may not be able to open:
   /// a bridge screens its recipient where that recipient lives.
   fn screen(&self, chain: ChainId, recipient: Address) -> TxFuture<'_, Result<(), ExecError>> {
      let _ = (chain, recipient);

      Box::pin(async { Err(ExecError::Unavailable) })
   }

   /// The bundler URL the app has stored for its own unshields, when it has one.
   ///
   /// Read through the port because it is a sealed file the binary owns the key to. It is the value an
   /// unshield uses when the call names none, so a call can be told whether it changed anything.
   fn stored_bundler_url(&self) -> Option<String> {
      None
   }
}

/// One wallet's gate: at most one action holds it, and it is released when that action ends.
type WalletGate = Arc<tokio::sync::Mutex<()>>;

/// The wallets currently in use, one gate each.
static WALLETS: LazyLock<Mutex<HashMap<(u64, Address), WalletGate>>> =
   LazyLock::new(Default::default);

/// One wallet, held for the length of one state-changing action (D20).
///
/// A guard rather than a pair of calls: every exit — a refusal, an error, a dropped future — releases it.
pub struct WalletLock {
   _held: tokio::sync::OwnedMutexGuard<()>,
}

impl WalletLock {
   /// Take this wallet's gate, waiting briefly for a previous action to finish.
   pub async fn take(chain: u64, wallet: Address) -> Result<Self, ExecError> {
      let gate = {
         let mut wallets = WALLETS.lock().unwrap();

         Arc::clone(wallets.entry((chain, wallet)).or_default())
      };

      match tokio::time::timeout(WALLET_WAIT, gate.lock_owned()).await {
         Ok(held) => Ok(Self { _held: held }),
         Err(_) => Err(ExecError::Busy),
      }
   }
}

/// Whether an unshield would change the broadcast options the app has stored.
///
/// A call that names the URL the app already holds is not an override: it says what the app says, which is
/// what a caller repeating the settings it read does. A call that changes either one is asking the app to
/// behave differently from its own configuration, which is the write the grant has to carry.
pub fn overrides_broadcast(flows: &dyn TxFlows, broadcast: &Broadcast) -> bool {
   match &broadcast.bundler_url {
      None => false,
      Some(url) => flows.stored_bundler_url().is_none_or(|stored| stored != *url),
   }
}

/// Where a call came from, for the record it leaves.
#[derive(Clone, Copy, Debug)]
pub struct Origin<'a> {
   /// The tool the request arrived through.
   pub tool: &'a str,
   /// What the harness called itself, when it said.
   pub client_info: Option<&'a str>,
}

/// Run one fund-moving action: guard, run the flow, record what happened, answer.
pub async fn execute(
   ctx: ZeusCtx,
   profile: &AgentProfile,
   call: ExecCall,
   origin: &Origin<'_>,
   flows: &dyn TxFlows,
) -> Result<Value, ExecError> {
   let started = Instant::now();
   let result = run(&ctx, profile, &call, flows).await;

   match &result {
      Ok(done) => record_done(&ctx, profile, &call, origin, done, started),
      Err(error) => record_refusal(&ctx, profile, &call, origin, error, started),
   }

   result.map(|done| answer(&ctx, profile, &call, &done))
}

/// The guard order, then the flow.
async fn run(
   ctx: &ZeusCtx,
   profile: &AgentProfile,
   call: &ExecCall,
   flows: &dyn TxFlows,
) -> Result<TransactionRich, ExecError> {
   let chain = call.chain_id()?;

   if !vault_open(ctx) {
      return Err(ExecError::Locked);
   }

   // One state-changing action per wallet at a time, taken before anything is built: the nonce is read
   // inside the flow, and two builds of the same one would replace or fail each other (D20).
   let _lock = WalletLock::take(call.chain(), call.wallet()).await?;

   match precheck(ctx, profile, &call.intent()) {
      Decision::Allowed { .. } => {}
      decision => return Err(decision.into()),
   }

   // `send_eth` and `send_token` screen their own recipient before anything is simulated (D19). The NFT flow
   // deliberately never did — a collection is a contract and its transfers were never screened — so this is
   // where that gap is closed, on the same address that goes into the calldata.
   if let ExecCall::Nft(request) = call {
      screen(ctx, chain, request.to).await?;
   }

   // A bridge hands funds to an address on another chain, and the pipeline has no way to read that one, so
   // the screen is asked through the port. (An unshield's recipient is on this chain and its flow screens it
   // before anything is proved, the same way a send's does.)
   if let ExecCall::Bridge(request) = call {
      screen_on(flows, request.dest_chain, request.to).await?;
   }

   let env = AgentEnv::agent(profile.id.clone()).with_priority(call.priority());

   match call {
      ExecCall::Send(request) => flows.send(env, request.clone()).await,
      ExecCall::Nft(request) => flows.send_nft(env, request.clone()).await,
      ExecCall::Revoke(request) => flows.revoke(env, request.clone()).await,
      ExecCall::Delegate(request) => flows.delegate(env, request.clone()).await,
      ExecCall::Swap(request) => {
         admit_slippage(profile, request.slippage_bps)?;

         flows.swap(env, request.clone()).await
      }
      ExecCall::Bridge(request) => flows.bridge(env, request.clone()).await,
      ExecCall::Shield(request) => flows.shield(env, request.clone()).await,
      ExecCall::Unshield(request) => flows.unshield(env, request.clone()).await,
      ExecCall::MergeNotes(request) => flows.merge_notes(env, request.clone()).await,
   }
}

/// Whether the grant allows this much slippage.
///
/// The cap is the grant's, not the UI's: an agent must not be able to set a slippage a human cannot, and the
/// grant is where that maximum is written down. A zero cap is no slippage at all rather than no limit.
fn admit_slippage(profile: &AgentProfile, slippage_bps: u16) -> Result<(), ExecError> {
   if slippage_bps > profile.max_slippage_bps {
      return Err(ExecError::Invalid(format!(
         "this grant caps slippage at {} basis points, and the call asked for {}",
         profile.max_slippage_bps, slippage_bps
      )));
   }

   Ok(())
}

/// What an action would do, without doing it.
///
/// The pipeline's simulation stage and the policy's reading of it, and nothing after: no wallet lock (a
/// preview builds no nonce), no key, no window, no broadcast. Deliberately not a re-run of the flow — the
/// flow's own transfers, approvals and main event are not built here — so the answer says it is a preview.
pub async fn preview(
   ctx: ZeusCtx,
   profile: &AgentProfile,
   call: &ExecCall,
) -> Result<Value, ExecError> {
   let chain = call.chain_id()?;

   if !vault_open(&ctx) {
      return Err(ExecError::Locked);
   }

   match precheck(&ctx, profile, &call.intent()) {
      Decision::Allowed { .. } => {}
      decision => return Err(decision.into()),
   }

   // A swap, a bridge and the three private calls build their transaction inside their flow, so the target
   // is where their preview stops — with the sentence that says this is not a preview of anything.
   let (interact_to, call_data, value) = call.target()?;

   if let Some(recipient) = call.recipient() {
      screen(&ctx, chain, recipient).await?;
   }

   let mut analysis = analyse(
      ctx.clone(),
      chain,
      call.wallet(),
      interact_to,
      call_data,
      value,
      Vec::new(),
   )
   .await
   .map_err(|error| ExecError::Failed(error.to_string()))?;

   price_analysis(&ctx, chain, &mut analysis).await;

   match authorise(&ctx, profile, &analysis) {
      Decision::Allowed { .. } => {}
      decision => return Err(decision.into()),
   }

   Ok(json!({
       "preview": true,
       "simulation": simulation_row(call.chain(), &analysis),
       "policy": policy_row(call, profile),
   }))
}

/// The funds screen, as a refusal the caller can act on.
async fn screen(ctx: &ZeusCtx, chain: ChainId, recipient: Address) -> Result<(), ExecError> {
   ensure_recipient_is_not_contract(ctx.clone(), chain, recipient)
      .await
      .map_err(|error| ExecError::Invalid(error.to_string()))
}

/// The funds screen for a bridge, on the chain the money lands on.
///
/// A bridge's recipient is only a recipient there, so this is asked port-side: the pipeline has no way to
/// read a second chain. A chain the port cannot read is a refusal, not a reason to send unscreened.
async fn screen_on(
   flows: &dyn TxFlows,
   dest_chain: u64,
   recipient: Address,
) -> Result<(), ExecError> {
   let dest_chain = ChainId::new(dest_chain).map_err(|_| {
      ExecError::Invalid(format!(
         "chain {dest_chain} is not one Zeus supports"
      ))
   })?;

   flows.screen(dest_chain, recipient).await
}

/// What a completed action answers with.
fn answer(ctx: &ZeusCtx, profile: &AgentProfile, call: &ExecCall, done: &TransactionRich) -> Value {
   json!({
       "tx": ctx.read(|ctx| read::transaction_row(ctx, done)),
       "simulation": simulation_row(call.chain(), &done.analysis),
       "policy": policy_row(call, profile),
   })
}

/// How the grant decided, in the words a client branches on.
fn policy_row(call: &ExecCall, profile: &AgentProfile) -> Value {
   json!({
       "capability": call.capability(),
       "wallet": format!("{:#x}", call.wallet()),
       "chain": call.chain(),
       "decision": if profile.mode == ProfileMode::ConfirmEach { "prompted" } else { "allowed" },
   })
}

/// What the simulation measured, in the shape every answer carries amounts in.
fn simulation_row(chain: u64, analysis: &TransactionAnalysis) -> Value {
   let tokens: Vec<Value> = analysis
      .balance_diff
      .tokens
      .iter()
      .map(|change| {
         let delta = change.abs_delta();
         let usd = NumericValue::value(delta.f64(), change.price.f64());

         json!({
             "token": read::currency_row(&change.currency),
             "direction": if change.is_increase() { "in" } else { "out" },
             "delta": read::amount(
                 &delta,
                 change.currency.decimals(),
                 change.currency.symbol(),
                 Some(&usd),
             ),
             "before": change.before.flatten(),
             "after": change.after.flatten(),
         })
      })
      .collect();

   let nfts: Vec<Value> = analysis
      .balance_diff
      .nfts
      .iter()
      .map(|change| {
         json!({
             "collection": format!("{:#x}", change.collection),
             "token_id": change.token_id.to_string(),
             "standard": if change.standard.is_erc721() { "erc721" } else { "erc1155" },
             "direction": if change.is_received() { "in" } else { "out" },
             "amount": change.abs_delta().to_string(),
         })
      })
      .collect();

   let approvals: Vec<Value> = analysis
      .approval_diff
      .sorted()
      .iter()
      .map(|change| {
         json!({
             "kind": match change.kind {
                ApprovalKind::Erc20 => "erc20",
                ApprovalKind::Permit2 => "permit2",
             },
             "token": read::currency_row(&change.token),
             "spender": format!("{:#x}", change.spender),
             "before": change.before.flatten(),
             "after": change.after.flatten(),
             "revoked": change.is_revoke(),
         })
      })
      .collect();

   json!({
       "chain": chain,
       "gas_used": analysis.gas_used,
       "gas_limit": crate::default_gas_limit(analysis.gas_used),
       "value": read::amount(&NumericValue::format_wei(analysis.value, 18), 18, &read::native_symbol(chain), None),
       "tokens": tokens,
       "nfts": nfts,
       "approvals": approvals,
   })
}

/// One sentence for the user, spoken when nothing asked them first.
fn sentence(call: &ExecCall, done: &TransactionRich) -> String {
   let display = |address: Address| crate::platform::truncate_address(format!("{address:#x}"));

   match call {
      ExecCall::Send(request) => {
         let symbol = read::event_currency(&done.main_event)
            .map(|currency| currency.symbol().to_owned())
            .unwrap_or_else(|| read::native_symbol(request.chain));
         let amount = NumericValue::format_wei(request.amount, request.decimals).flatten();

         format!(
            "Agent sent {amount} {symbol} to {}",
            display(request.to)
         )
      }
      ExecCall::Nft(request) => format!(
         "Agent sent an NFT from {} to {}",
         display(request.collection),
         display(request.to)
      ),
      ExecCall::Revoke(request) => format!(
         "Agent revoked an approval on chain {}",
         request.chain
      ),
      ExecCall::Delegate(request) if request.delegate_to.is_zero() => {
         "Agent removed its wallet's delegation".to_owned()
      }
      ExecCall::Delegate(request) => format!(
         "Agent delegated its wallet to {}",
         display(request.delegate_to)
      ),
      ExecCall::Swap(request) => {
         let symbol = read::event_currency(&done.main_event)
            .map(|currency| currency.symbol().to_owned())
            .unwrap_or_else(|| {
               crate::platform::truncate_address(format!("{:#x}", request.currency_in))
            });
         let amount = NumericValue::format_wei(request.amount_in, request.decimals_in).flatten();

         format!("Agent swapped {amount} {symbol}")
      }
      ExecCall::Bridge(request) => {
         let amount = NumericValue::format_wei(request.amount, request.decimals).flatten();

         format!(
            "Agent bridged {amount} to {} on chain {}",
            display(request.to),
            request.dest_chain
         )
      }
      ExecCall::Shield(request) => {
         let amount = NumericValue::format_wei(request.amount, request.decimals).flatten();

         format!(
            "Agent shielded {amount} on chain {}",
            request.chain
         )
      }
      // The app's own warning, verbatim: a self-broadcast is the one thing an unshield can do that the user
      // would not expect to hear about afterwards.
      ExecCall::Unshield(request) if request.broadcast.self_broadcast => {
         format!(
            "{SELF_BROADCAST_NOTICE} Agent unshielded to {}",
            display(request.to)
         )
      }
      ExecCall::Unshield(request) => {
         let amount = NumericValue::format_wei(request.amount, request.decimals).flatten();

         format!(
            "Agent unshielded {amount} to {}",
            display(request.to)
         )
      }
      ExecCall::MergeNotes(request) => {
         let amount = NumericValue::format_wei(request.amount, request.decimals).flatten();

         format!("Agent merged {amount} of its private notes")
      }
   }
}

/// Record a completed action, and say it out loud.
///
/// This is the only trace an autonomous action leaves: nothing asked the user first, so there is no prompt
/// to have read and no window they saw it in.
fn record_done(
   ctx: &ZeusCtx,
   profile: &AgentProfile,
   call: &ExecCall,
   origin: &Origin<'_>,
   done: &TransactionRich,
   started: Instant,
) {
   let sentence = sentence(call, done);
   let mut entry = AuditEntry::new(
      profile,
      origin.tool,
      call.capability(),
      now_secs(),
   );

   entry.wallet = Some(call.wallet());
   entry.chain = Some(call.chain());
   entry.decision = if profile.mode == ProfileMode::ConfirmEach {
      AuditDecision::Prompted
   } else {
      AuditDecision::Allowed
   };
   entry.tx_hash = Some(done.hash);
   entry.usd_value = (!done.value_sent_usd.is_zero()).then(|| done.value_sent_usd.f64());
   entry.detail = Some(sentence.clone());
   entry.latency_ms = started.elapsed().as_millis() as u64;
   entry.client_info = origin.client_info.map(str::to_owned);

   ctx.agent_audit().record(entry);
   ui::announce(&sentence);
}

/// Record an action that was refused after the gate in front of the tool let it through.
///
/// A refusal the *policy* makes at the entry is already recorded by that gate; what lands here is the
/// screen, the lock and the flow's own no — the three a caller cannot see coming. A denial is also
/// **said out loud**: the user was not asked first, so the sentence is the only trace they can hear.
/// The rest answer the caller without a word, because a wallet that is busy or a window nobody
/// answered is not news about what the agent tried to do.
fn record_refusal(
   ctx: &ZeusCtx,
   profile: &AgentProfile,
   call: &ExecCall,
   origin: &Origin<'_>,
   error: &ExecError,
   started: Instant,
) {
   let mut entry = AuditEntry::new(
      profile,
      origin.tool,
      call.capability(),
      now_secs(),
   );

   entry.wallet = Some(call.wallet());
   entry.chain = Some(call.chain());
   entry.decision = AuditDecision::Denied;
   entry.denial = Some(error.code().to_owned());
   entry.detail = Some(error.to_string());
   entry.latency_ms = started.elapsed().as_millis() as u64;
   entry.client_info = origin.client_info.map(str::to_owned);

   ctx.agent_audit().record(entry);

   if let ExecError::Denied(denial) = error {
      announce_denial(denial);
   }
}

/// Unix seconds, for the record.
fn now_secs() -> u64 {
   crate::platform::TimeStamp::now_as_secs().unwrap_or_default().timestamp()
}

#[cfg(test)]
mod tests {
   use super::*;

   fn wallet() -> Address {
      Address::repeat_byte(0xa1)
   }

   fn recipient() -> Address {
      Address::repeat_byte(0xb0)
   }

   fn token() -> Address {
      Address::repeat_byte(0x5c)
   }

   fn send(token: Option<Address>, amount: U256) -> ExecCall {
      ExecCall::Send(SendRequest {
         chain: 1,
         from: wallet(),
         to: recipient(),
         token,
         amount,
         decimals: 6,
         priority: PriorityTier::Normal,
      })
   }

   #[test]
   fn a_preview_builds_the_call_the_flow_would() {
      // Native: the whole call is the value the transaction declares.
      let (to, call_data, value) = send(None, U256::from(7)).target().expect("a native send");

      assert_eq!(to, recipient());
      assert!(call_data.is_empty());
      assert_eq!(value, U256::from(7));

      // ERC-20: a transfer to the recipient, and no ether beside it.
      let (to, call_data, value) =
         send(Some(token()), U256::from(1_000_000u64)).target().expect("a token send");

      assert_eq!(to, token());
      assert_eq!(
         erc20::encode_transfer(recipient(), U256::from(1_000_000u64)),
         call_data
      );
      assert!(value.is_zero());

      // An NFT goes to the collection, and an ERC-721 moves exactly one id.
      let nft = |standard, amount| {
         ExecCall::Nft(NftSendRequest {
            chain: 1,
            from: wallet(),
            to: recipient(),
            collection: recipient(),
            token_id: U256::from(7),
            standard,
            amount,
            priority: PriorityTier::Normal,
         })
      };

      let (to, call_data, value) =
         nft(NftStandard::Erc721, U256::from(1)).target().expect("one id of an ERC-721");
      assert_eq!(to, recipient());
      assert_eq!(
         erc721::encode_safe_transfer_from(wallet(), recipient(), U256::from(7)),
         call_data
      );
      assert!(value.is_zero());

      assert!(
         matches!(
            nft(NftStandard::Erc721, U256::from(2)).target(),
            Err(ExecError::Invalid(_))
         ),
         "an ERC-721 transfer has no amount to say"
      );

      // The three revoke shapes are not interchangeable, so the caller states one.
      let revoke = |target| {
         ExecCall::Revoke(RevokeRequest {
            chain: 1,
            owner: wallet(),
            target,
            priority: PriorityTier::Normal,
         })
      };

      let (to, call_data, _) = revoke(RevokeTarget::Token {
         token: token(),
         spender: recipient(),
      })
      .target()
      .expect("an ERC-20 revoke");

      assert_eq!(to, token());
      assert_eq!(
         erc20::encode_approve(recipient(), U256::ZERO),
         call_data
      );

      let (_, call_data, _) = revoke(RevokeTarget::Nft {
         collection: recipient(),
         operator: recipient(),
         token_id: None,
         standard: None,
      })
      .target()
      .expect("a collection-wide revoke");

      assert_eq!(
         erc721::encode_set_approval_for_all(recipient(), false),
         call_data
      );

      assert!(
         matches!(
            revoke(RevokeTarget::Nft {
               collection: recipient(),
               operator: recipient(),
               token_id: Some(U256::from(7)),
               standard: None,
            })
            .target(),
            Err(ExecError::Invalid(_))
         ),
         "one id cannot be cleared without knowing how the collection models it"
      );
   }

   #[test]
   fn the_sentence_says_what_left_and_where_it_went() {
      let done = TransactionRich::dummy_clear_signed();

      let line = sentence(
         &send(None, U256::from(1_500_000_000_000_000u64)),
         &done,
      );

      assert!(line.starts_with("Agent sent "), "{line}");
      assert!(
         line.contains("ETH"),
         "the sentence names the asset: {line}"
      );
      assert!(
         line.ends_with(" to 0xb0b0...b0b0b0"),
         "and says where it went, short enough to say out loud: {line}"
      );
   }

   #[test]
   fn a_self_broadcast_names_what_it_costs() {
      let done = TransactionRich::dummy_clear_signed();
      let unshield = |self_broadcast| {
         ExecCall::Unshield(UnshieldRequest {
            chain: 1,
            from: wallet(),
            token: token(),
            amount: U256::from(1_000_000u64),
            decimals: 6,
            nft: None,
            to: recipient(),
            broadcast: Broadcast {
               self_broadcast,
               bundler_url: None,
            },
            unwrap_to_eth: false,
            memo: None,
            priority: PriorityTier::Normal,
         })
      };

      // The sponsored path is the quiet one: it says what happened and nothing more.
      let quiet = sentence(&unshield(false), &done);
      assert!(!quiet.contains("Breaks anonymity"), "{quiet}");
      assert!(quiet.ends_with(" to 0xb0b0...b0b0b0"), "{quiet}");

      // A self-broadcast is not quiet: the app's own warning comes first, because it is the part of the
      // sentence a user has to be able to act on.
      let loud = sentence(&unshield(true), &done);
      assert!(loud.starts_with(SELF_BROADCAST_NOTICE), "{loud}");
      assert!(loud.ends_with(" to 0xb0b0...b0b0b0"), "{loud}");
   }

   #[test]
   fn a_private_asset_is_an_id_only_when_one_was_named() {
      let asset = |nft: bool| {
         nft_asset(
            1,
            token(),
            nft.then_some(NftMovement {
               token_id: U256::from(7),
               standard: NftStandard::Erc721,
            }),
         )
      };

      assert_eq!(
         asset(false),
         Asset::Token(TokenRef {
            chain: 1,
            address: token()
         })
      );
      assert_eq!(
         asset(true),
         Asset::Nft {
            chain: 1,
            collection: token(),
            token_id: U256::from(7)
         },
         "a collection with an id is the envelope's NFT shape, not its token shape"
      );
   }
}
