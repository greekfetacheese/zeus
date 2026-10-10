//! The unshield privacy check: what this withdrawal would reveal, and what to do instead.
//!
//! Three questions, all answered from data Zeus already has:
//!
//! - **Is the amount distinctive?** The persisted Railgun events snapshot holds every deposit the
//!   protocol has taken ([`crate::privacy`] in `zeus-railgun` scores the requested amount against
//!   the last 180 days of them), and the tx history plus the active 0zk's private history hold the
//!   wallet's own shields and unshields — the deposits this amount could be a *repeat* of.
//! - **Is the pool deep enough to hide in?** The token's balance held by the Railgun contract is
//!   how much of that asset is shielded at all: a withdrawal out of a small or unvalued pool is
//!   visible whatever the amount looks like.
//! - **Has the recipient been used?** A fresh `0x` address is the cheapest thing to get right and
//!   the most common way to undo everything else.
//!
//! All of it is I/O — snapshot reads, an RPC balance, a nonce — so it runs off the frame path and
//! its verdict is written back through `SHIELD_GUI`. Nothing here refuses an unshield: a check that
//! cannot run reports why, and the form keeps the static advice it always had.

use std::collections::{HashMap, HashSet};

use zeus_eth::{
   alloy_primitives::{Address, TxHash, U256},
   currency::ERC20Token,
   types::ChainId,
   utils::NumericValue,
};
use zeus_railgun::{
   PrivateHistoryKind, RailgunProvider,
   caip::AssetId,
   privacy::{
      ACTIVITY_WINDOW_SECONDS, Deposit, DepositWindow, UnshieldAmountAdvice, assess_amount,
   },
};

use zeus_eth::utils::client::RpcClient;

use crate::core::{DecodedEvent, ZeusCtx};
use crate::utils::TimeStamp;

use super::RailgunAsset;

/// A pool holding less than this is too shallow to hide a withdrawal in.
pub const SHALLOW_POOL_USD: f64 = 10_000.0;

/// A pool with fewer deposits than this has too little activity to judge a size against.
///
/// The value test says how *much* was shielded; this says how *many* shielded it. A testnet pool can
/// pass the first and fail this one, which is exactly where a confident "low risk" misleads most.
pub const MIN_POOL_DEPOSITS: u32 = 1_500;

/// A withdrawal taking more than this share of the pool is a signal of its own.
pub const LARGE_POOL_SHARE: f64 = 0.25;

/// The protocol's fee as basis points: 0.25% is 25.
fn basis_points(percent: f64) -> u128 {
   (percent * 100.0).round().clamp(0.0, 10_000.0) as u128
}

/// What the chain records for a typed amount.
///
/// The contract keeps its fee before it emits the event, so the amount in the pool — and in the
/// wallet's own history — is smaller than the amount in the form. Comparing one against the other
/// matches nothing, which is why an exact-match check has to convert first.
fn onchain_amount(typed_wei: u128, fee_percent: f64) -> u128 {
   let kept = 10_000u128.saturating_sub(basis_points(fee_percent));
   typed_wei.saturating_mul(kept) / 10_000
}

/// The number to type for the chain to record `onchain_wei` — [`onchain_amount`] inverted, rounded up
/// so the recorded amount is never below the one that was asked for.
fn typed_amount(onchain_wei: u128, fee_percent: f64) -> u128 {
   let kept = 10_000u128.saturating_sub(basis_points(fee_percent)).max(1);
   onchain_wei.saturating_mul(10_000).div_ceil(kept)
}

/// How deep the pool is for the asset being unshielded, and how much of it this withdrawal is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoolDepth {
   /// What the Railgun contract holds of this token: everything shielded, in the token's units.
   pub balance_wei: U256,
   /// What that is worth, when Zeus can price the token at all.
   pub usd: Option<f64>,
   /// The share of the pool this withdrawal would take, when there is a pool to speak of.
   pub share: Option<f64>,
   /// How many deposits the window held, when the amount check got far enough to count them.
   pub deposits: Option<u32>,
}

impl PoolDepth {
   /// Too small to hide in, unvalued, or mostly this one withdrawal.
   ///
   /// An unpriceable token is deliberately treated as shallow: if its size cannot be established,
   /// nothing here can say it is deep, and saying nothing would read as reassurance.
   pub fn is_shallow(&self) -> bool {
      self.deposits.is_some_and(|deposits| deposits < MIN_POOL_DEPOSITS)
         || self.usd.is_none_or(|usd| usd < SHALLOW_POOL_USD)
         || self.share.is_some_and(|share| share > LARGE_POOL_SHARE)
   }
}

/// How the recipient is tied to this wallet — the part of "has it been used" that actually links a
/// withdrawal back to the deposits behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientLink {
   /// The recipient is one of the wallets on this machine: the withdrawal lands on the address the
   /// deposits may well have come from.
   OwnWallet,
   /// This wallet has transacted with the address before, in either direction.
   Transacted,
   /// This wallet has unshielded to it before, this many times.
   PriorUnshields(usize),
}

/// What the chain and this wallet's own history say about the address receiving the unshield.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecipientAdvice {
   /// `Some(true)` when the address has no on-chain history at all, `None` when the chain could not
   /// be asked — which is not the same answer as "fresh".
   pub fresh: Option<bool>,
   /// The recipient is one of the wallets on this machine.
   pub is_own_wallet: bool,
   /// This wallet's history contains a transaction with the address, in either direction.
   pub transacted_with: bool,
   /// Unshields to this address, counted on every chain this wallet has history on.
   pub prior_unshields: usize,
}

impl RecipientAdvice {
   /// The loudest link this address has to the wallet, if any.
   ///
   /// Having on-chain history is not one: plenty of addresses have paid gas once, and that ties the
   /// withdrawal to nothing. Only these three tie it back to the deposits this wallet made.
   pub fn link(&self) -> Option<RecipientLink> {
      if self.is_own_wallet {
         return Some(RecipientLink::OwnWallet);
      }
      if self.transacted_with {
         return Some(RecipientLink::Transacted);
      }
      match self.prior_unshields {
         0 => None,
         times => Some(RecipientLink::PriorUnshields(times)),
      }
   }
}

/// What the unshield form shows above its button.
#[derive(Debug, Clone, PartialEq)]
pub struct UnshieldPrivacy {
   /// The block the activity was read up to — the snapshot's tip, which trails the chain.
   ///
   /// The verdict speaks about the protocol as of this block and nothing later, so the form states it
   /// rather than letting the reader imagine it saw the last few minutes.
   pub checked_block: u64,
   /// The amount verdict, when the pool could be read and scored.
   ///
   /// Its `suggestion`, and only it, is already in the units the form takes — what to type — rather
   /// than the units the pool holds, which are net of the protocol's fee.
   pub amount: Option<UnshieldAmountAdvice>,
   /// Why there is no amount verdict — shown with the tips, never as a refusal.
   pub unavailable: Option<String>,
   pub pool: Option<PoolDepth>,
   pub recipient: RecipientAdvice,
}

impl UnshieldPrivacy {
   /// Nothing to say yet: the form renders its static advice until a check lands.
   pub fn empty() -> Self {
      Self {
         checked_block: 0,
         amount: None,
         unavailable: None,
         pool: None,
         recipient: RecipientAdvice::default(),
      }
   }
}

/// The recorded transactions the check reads, without the rest of a [`crate::core::TransactionRich`].
///
/// Taking just the four fields the check needs keeps the fold below a pure function: no context, no
/// wallet, no chain.
pub struct OwnTx<'a> {
   /// Which chain the transaction happened on: an amount only means something on its own chain.
   pub chain: u64,
   pub hash: TxHash,
   pub block: u64,
   pub timestamp: u64,
   /// The transaction's main event — what the amount checks read.
   pub event: &'a DecodedEvent,
   /// Everything the transaction decoded, so a counterparty that is not the main event is not missed.
   pub events: &'a [DecodedEvent],
   /// Who sent it, and what it called.
   pub sender: Address,
   pub interact_to: Address,
}

/// The wallet's own activity, split into what the amount check needs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OwnHistory {
   /// Shields of the asset being unshielded.
   pub shields: Vec<Deposit>,
   /// Unshields of the asset being unshielded, from the tx history and the private history.
   pub unshields: Vec<Deposit>,
   /// How many unshields each address has received, on every chain.
   pub unshields_to: HashMap<Address, usize>,
   /// Whether the wallet's own history contains a transaction with the recipient it was asked about.
   pub transacted_with: bool,
}

impl OwnHistory {
   pub fn prior_unshields_to(&self, recipient: Address) -> usize {
      self.unshields_to.get(&recipient).copied().unwrap_or(0)
   }
}

/// Fold the wallet's recorded transactions into the deposits its own history implies.
///
/// The two questions have different scopes, so the fold reads them differently. An **amount** only
/// means something on its own chain — another chain's pool is a different pool, and the same token
/// address can be a different token — and only for the asset being unshielded. A **recipient**, on
/// the other hand, is the same public address everywhere, so reuse and prior transactions with it
/// are counted across every chain and every asset.
///
/// `asset` is `None` when the asset cannot be checked at all (an NFT): the recipient signals are
/// still collected, because the address links back to the wallet whatever is being unshielded.
pub fn own_history<'a>(
   txs: impl IntoIterator<Item = OwnTx<'a>>,
   asset: Option<&AssetId>,
   chain: u64,
   recipient: Address,
) -> OwnHistory {
   let mut history = OwnHistory::default();

   for tx in txs {
      if let DecodedEvent::Unshield(params) = tx.event {
         *history.unshields_to.entry(params.recipient).or_default() += 1;
      }

      if tx.sender == recipient
         || tx.interact_to == recipient
         || counterparty(tx.event, recipient)
         || tx.events.iter().any(|event| counterparty(event, recipient))
      {
         history.transacted_with = true;
      }

      if tx.chain != chain {
         continue;
      }

      match tx.event {
         DecodedEvent::Shield(params) => {
            if asset.is_none_or(|asset| params.asset != *asset) {
               continue;
            }
            if let Ok(amount_wei) = u128::try_from(params.amount_wei) {
               history.shields.push(Deposit {
                  amount_wei,
                  timestamp: tx.timestamp,
                  block: tx.block,
               });
            }
         }
         DecodedEvent::Unshield(params) => {
            if asset.is_none_or(|asset| AssetId::from(params.token_data.clone()) != *asset) {
               continue;
            }
            if let Ok(amount_wei) = u128::try_from(params.amount_wei) {
               history.unshields.push(Deposit {
                  amount_wei,
                  timestamp: tx.timestamp,
                  block: tx.block,
               });
            }
         }
         _ => {}
      }
   }

   history
}

/// Whether a decoded event names `address` as a participant.
///
/// Only the transfers that move value to an address count. An approval names the spender without
/// anything moving, and a seed of funds is not the link a withdrawal is judged on.
fn counterparty(event: &DecodedEvent, address: Address) -> bool {
   match event {
      DecodedEvent::Transfer(params) => params.sender == address || params.recipient == address,
      DecodedEvent::NftTransfer(params) => params.from == address || params.to == address,
      DecodedEvent::Unshield(params) => params.recipient == address,
      _ => false,
   }
}

/// The fungible token a check can be run for, if any.
///
/// An NFT is out of scope by construction: its amount is always one, so there is nothing distinctive
/// about it, and it has no pool price to judge depth by. A native asset cannot be unshielded at all.
pub fn fungible_token(asset: &RailgunAsset) -> Option<ERC20Token> {
   match asset {
      RailgunAsset::Fungible(currency) if !currency.is_native() => {
         Some(currency.to_erc20().into_owned())
      }
      _ => None,
   }
}

/// Everything the unshield form needs to advise on this withdrawal.
///
/// Never fails: a check that could not run explains itself in [`UnshieldPrivacy::unavailable`] and
/// the unshield proceeds as it always did.
pub async fn assess_unshield(
   ctx: ZeusCtx,
   chain: ChainId,
   asset: &RailgunAsset,
   amount_wei: U256,
   recipient: Address,
   owner: Address,
) -> UnshieldPrivacy {
   // The recipient is judged however the amount turns out: an NFT has no distinctive amount, but it
   // still lands on an address that may link back to this wallet.
   let token = fungible_token(asset);
   let asset_id = token.as_ref().map(|token| AssetId::Erc20(token.address));
   let own = own_tx_history(&ctx, chain, asset_id.as_ref(), recipient, owner);

   let mut privacy = UnshieldPrivacy {
      recipient: recipient_advice(&ctx, recipient, &own).await,
      ..UnshieldPrivacy::empty()
   };

   let (Some(token), Some(asset_id)) = (token, asset_id) else {
      return privacy;
   };

   let provider = match ctx.get_railgun_provider(chain.id(), false).await {
      Ok(provider) => provider,
      Err(e) => {
         privacy.unavailable = Some(format!("Railgun is not ready: {e}"));
         return privacy;
      }
   };

   let tip = match provider.snapshot_tip().await {
      Ok(tip) => tip,
      Err(e) => {
         privacy.unavailable = Some(format!(
            "could not read the activity snapshot: {e}"
         ));
         return privacy;
      }
   };

   // Every verdict below describes the protocol up to this block, and says so.
   privacy.checked_block = tip;

   if tip == 0 {
      privacy.unavailable = Some("no Railgun activity snapshot on this machine yet".to_string());
      return privacy;
   }

   let now = TimeStamp::now_as_secs()
      .map(|timestamp| timestamp.timestamp())
      .unwrap_or_default();
   let block_time = provider.block_time_secs().max(1);

   // The window is stated twice on purpose: blocks select what to read out of the snapshot (cheap),
   // times decide what belongs in it (exact). Its end block is the snapshot's tip, so the activity
   // is judged against what this machine has actually seen.
   let window = DepositWindow::new(
      now.saturating_sub(ACTIVITY_WINDOW_SECONDS),
      now,
      tip.saturating_sub(ACTIVITY_WINDOW_SECONDS / block_time),
      tip,
   );

   let pool = match provider.shield_activity(asset_id, window).await {
      Ok(pool) => pool,
      Err(e) => {
         privacy.unavailable = Some(format!("could not read Railgun activity: {e}"));
         return privacy;
      }
   };

   let own = own_private_activity(&ctx, chain, &provider, &asset_id, own, owner).await;

   // The pool holds what the chain recorded — every amount net of the protocol's fee — while the form
   // holds what the user typed. One is not comparable with the other, so the request is converted
   // into chain units before it is scored, and the suggestion is converted back for the form.
   let fee_percent = provider.unshield_fee();

   match u128::try_from(amount_wei) {
      Ok(typed_wei) => {
         let onchain_wei = onchain_amount(typed_wei, fee_percent);

         match assess_amount(
            &pool,
            onchain_wei,
            token.decimals,
            now,
            &own.shields,
            &own.unshields,
         ) {
            Ok(mut advice) => {
               advice.suggestion = advice.suggestion.map(|we| typed_amount(we, fee_percent));
               privacy.amount = Some(advice);
            }
            Err(e) => privacy.unavailable = Some(format!("{e}")),
         }

         let deposits = privacy.amount.as_ref().map(|advice| advice.pool_size);
         privacy.pool = pool_depth(
            &ctx,
            chain,
            &provider,
            &token,
            onchain_wei,
            deposits,
         )
         .await;
      }
      Err(_) => privacy.unavailable = Some("enter an amount first".to_string()),
   }

   privacy
}

/// The wallet's own activity as the tx history records it.
///
/// Nothing here needs the Railgun provider, so the recipient verdict is available even when Railgun
/// is not ready — which is when a user is most likely to be shown something useful instead of nothing.
fn own_tx_history(
   ctx: &ZeusCtx,
   chain: ChainId,
   asset: Option<&AssetId>,
   recipient: Address,
   owner: Address,
) -> OwnHistory {
   // Rows from every chain: the recipient signals are read across all of them, while the fold keeps
   // the amounts to this chain.
   ctx.tx_db().visit_own_txs(owner, |txs| {
      own_history(
         txs.into_iter().map(|tx| OwnTx {
            chain: tx.chain,
            hash: tx.hash,
            block: tx.block,
            timestamp: tx.timestamp.timestamp(),
            event: &tx.main_event,
            events: &tx.analysis.decoded_events,
            sender: tx.analysis.sender,
            interact_to: tx.analysis.interact_to,
         }),
         asset,
         chain.id(),
         recipient,
      )
   })
}

/// Add the unshields the private history holds and the tx history does not.
///
/// The two sources overlap — a Zeus-made unshield is in both — so only what the tx history lacks is
/// added: a withdrawal counted twice would be read as a larger partial withdrawal than it was, which
/// is exactly the number the remainder check depends on.
async fn own_private_activity(
   ctx: &ZeusCtx,
   chain: ChainId,
   provider: &RailgunProvider<RpcClient>,
   asset: &AssetId,
   mut history: OwnHistory,
   owner: Address,
) -> OwnHistory {
   let Some(railgun_address) = ctx.current_wallet_info().railgun_address else {
      return history;
   };

   // The transactions the tx history already accounts for: a Zeus-made unshield spends its note in
   // the very transaction the receipt recorded, so the hashes are the dedupe key. A private-history
   // row without a hash can only be matched on what it holds.
   let mut counted: HashSet<TxHash> = ctx
      .tx_db()
      .get_txs(chain.id(), owner)
      .unwrap_or_default()
      .iter()
      .filter(|tx| match &tx.main_event {
         DecodedEvent::Unshield(params) => AssetId::from(params.token_data.clone()) == *asset,
         _ => false,
      })
      .map(|tx| tx.hash)
      .collect();

   for entry in provider.private_history(railgun_address).await {
      if entry.kind != PrivateHistoryKind::Unshield || entry.asset != *asset {
         continue;
      }

      if entry.tx_hash != TxHash::ZERO {
         if !counted.insert(entry.tx_hash) {
            continue;
         }
      } else if history.unshields.iter().any(|withdrawal| {
         withdrawal.amount_wei == entry.amount && withdrawal.timestamp == entry.spent_timestamp
      }) {
         continue;
      }

      history.unshields.push(Deposit {
         amount_wei: entry.amount,
         timestamp: entry.spent_timestamp,
         block: entry.spent_block,
      });
   }

   history
}

/// How deep the pool is for `token`, and what share of it this withdrawal would take.
async fn pool_depth(
   ctx: &ZeusCtx,
   chain: ChainId,
   provider: &RailgunProvider<RpcClient>,
   token: &ERC20Token,
   amount_wei: u128,
   deposits: Option<u32>,
) -> Option<PoolDepth> {
   let client = ctx.get_client(chain.id()).await.ok()?;
   let balance = token.balance_of(client, provider.railgun_address(), None).await.ok()?;

   // A price of zero is "not priced", not "worth nothing": the two must not be conflated, or every
   // unpriced token would look like a dead pool.
   let price = ctx.get_token_price(token);
   let usd = if price.is_zero() {
      None
   } else {
      let amount = NumericValue::format_wei(balance, token.decimals);
      Some(ctx.get_token_value_for_amount(amount.f64(), token).f64())
   };

   let share = if balance.is_zero() {
      None
   } else {
      let balance = balance.to_string().parse::<f64>().unwrap_or(f64::MAX);
      let amount = amount_wei as f64;
      Some(amount / balance)
   };

   Some(PoolDepth {
      balance_wei: balance,
      usd,
      share,
      deposits,
   })
}

/// What the chain says about this address, and what this wallet already knows of it.
///
/// The links come first: the address being one of this machine's own wallets, a transaction with it
/// in this wallet's history, and earlier unshields to it. The nonce answers a fourth, different
/// question — has *anyone* used it — which is worth reporting and is not a link by itself.
async fn recipient_advice(ctx: &ZeusCtx, recipient: Address, own: &OwnHistory) -> RecipientAdvice {
   let nonce = ctx.get_transaction_count(recipient).await.ok();

   RecipientAdvice {
      fresh: nonce.map(|nonce| nonce == 0),
      is_own_wallet: ctx.get_all_wallets_info().iter().any(|wallet| wallet.address == recipient),
      transacted_with: own.transacted_with,
      prior_unshields: own.prior_unshields_to(recipient),
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::alloy_primitives::{U256, address};
   use zeus_eth::currency::Currency;
   use zeus_railgun::abi::railgun::{TokenData, TokenType};

   use crate::core::tx::events::{ShieldParams, TransferParams, UnshieldParams};

   const WETH: AssetId = AssetId::Erc20(address!(
      "C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
   ));
   const USDC: AssetId = AssetId::Erc20(address!(
      "A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
   ));
   const RECIPIENT: Address = address!("1111111111111111111111111111111111111111");

   fn shield_event(asset: AssetId, amount_wei: u128) -> DecodedEvent {
      DecodedEvent::Shield(ShieldParams {
         chain: 1,
         recipient: None,
         asset,
         amount_wei: U256::from(amount_wei),
         erc20: None,
         nft: None,
         amount: None,
         amount_usd: None,
         fee: None,
         fee_usd: None,
      })
   }

   fn unshield_event(asset: Address, amount_wei: u128, recipient: Address) -> DecodedEvent {
      DecodedEvent::Unshield(UnshieldParams {
         chain: 1,
         recipient,
         token_data: TokenData {
            tokenType: TokenType::ERC20,
            tokenAddress: asset,
            tokenSubID: U256::ZERO,
         },
         erc20: None,
         nft: None,
         amount_wei: U256::from(amount_wei),
         amount: None,
         amount_usd: None,
         fee: None,
         fee_usd: None,
         is_self_broadcast: false,
         fee_token: None,
         broadcaster_fee: None,
         broadcaster_fee_usd: None,
      })
   }

   /// A recorded transaction with everything the fold needs beyond the event left neutral.
   fn tx<'a>(chain: u64, block: u64, timestamp: u64, event: &'a DecodedEvent) -> OwnTx<'a> {
      OwnTx {
         chain,
         hash: TxHash::from([block as u8; 32]),
         block,
         timestamp,
         event,
         events: std::slice::from_ref(event),
         sender: address!("9999999999999999999999999999999999999999"),
         interact_to: address!("8888888888888888888888888888888888888888"),
      }
   }

   fn transfer(to: Address) -> DecodedEvent {
      DecodedEvent::Transfer(TransferParams {
         currency: Currency::native(1),
         amount: NumericValue::format_wei(U256::from(1_000u64), 18),
         amount_usd: None,
         real_amount_sent: None,
         real_amount_sent_usd: None,
         sender: address!("7777777777777777777777777777777777777777"),
         recipient: to,
      })
   }

   /// The wallet's own history is read for the asset being unshielded, and recipient reuse is
   /// counted whatever asset the unshield moved.
   #[test]
   fn own_history_keeps_the_asset_and_counts_every_recipient_use() {
      let our_shield = shield_event(WETH, 1_000);
      let other_shield = shield_event(USDC, 2_000);
      let our_unshield = unshield_event(WETH.erc20_address().unwrap(), 400, RECIPIENT);
      let other_unshield = unshield_event(USDC.erc20_address().unwrap(), 500, RECIPIENT);
      let elsewhere = unshield_event(
         WETH.erc20_address().unwrap(),
         600,
         address!("2222222222222222222222222222222222222222"),
      );

      let history = own_history(
         [
            tx(1, 10, 1_000, &our_shield),
            tx(1, 11, 1_001, &other_shield),
            tx(1, 12, 1_002, &our_unshield),
            tx(1, 13, 1_003, &other_unshield),
            tx(1, 14, 1_004, &elsewhere),
         ],
         Some(&WETH),
         1,
         RECIPIENT,
      );

      assert_eq!(history.shields.len(), 1);
      assert_eq!(history.shields[0].amount_wei, 1_000);
      assert_eq!(
         history.unshields.len(),
         2,
         "both WETH unshields, any recipient"
      );
      assert_eq!(history.prior_unshields_to(RECIPIENT), 2);
      assert_eq!(
         history.prior_unshields_to(address!(
            "2222222222222222222222222222222222222222"
         )),
         1
      );
      assert_eq!(
         history.prior_unshields_to(address!(
            "3333333333333333333333333333333333333333"
         )),
         0
      );
   }

   /// An NFT has no amount to be distinctive and no pool price to judge; a native asset cannot be
   /// unshielded at all. Both are out of scope rather than scored wrongly.
   #[test]
   fn only_a_fungible_token_is_checked() {
      use zeus_eth::nft::{NftStandard, NftToken};

      let weth = RailgunAsset::Fungible(Currency::from(ERC20Token::weth()));
      assert!(fungible_token(&weth).is_some());

      let native = RailgunAsset::Fungible(Currency::native(1));
      assert!(
         fungible_token(&native).is_none(),
         "native is never unshielded"
      );

      let nft = RailgunAsset::Nft(NftToken {
         chain_id: 1,
         collection: address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D"),
         token_id: U256::from(1),
         standard: NftStandard::Erc721,
         metadata_uri: None,
      });
      assert!(fungible_token(&nft).is_none());
   }

   /// A shallow pool is one that is small, unvalued, or mostly this withdrawal — and an unvalued
   /// token is shallow on purpose, because silence would read as reassurance.
   #[test]
   fn a_pool_is_shallow_when_small_unvalued_or_mostly_this_withdrawal() {
      let deep = PoolDepth {
         balance_wei: U256::from(1_000u64),
         usd: Some(50_000.0),
         share: Some(0.01),
         deposits: Some(40_000),
      };
      assert!(!deep.is_shallow());

      let small = PoolDepth {
         usd: Some(400.0),
         ..deep
      };
      assert!(small.is_shallow());

      let unvalued = PoolDepth { usd: None, ..deep };
      assert!(unvalued.is_shallow());

      let most_of_it = PoolDepth {
         share: Some(0.4),
         ..deep
      };
      assert!(most_of_it.is_shallow());

      let empty = PoolDepth {
         balance_wei: U256::ZERO,
         usd: Some(0.0),
         share: None,
         ..deep
      };
      assert!(empty.is_shallow());

      // Deep by value, thin by traffic: a testnet pool that a size verdict cannot be trusted on.
      let thin = PoolDepth {
         deposits: Some(1_224),
         ..deep
      };
      assert!(thin.is_shallow());

      // No count is not a thin count: the amount check did not run, which is not evidence of anything.
      let uncounted = PoolDepth {
         deposits: None,
         ..deep
      };
      assert!(!uncounted.is_shallow());
   }

   /// What links a withdrawal back to the deposits is not history: plenty of addresses have paid gas
   /// once. Only the wallet's own addresses, its own transactions, and its own earlier unshields do.
   #[test]
   fn a_recipient_links_only_when_it_ties_back_to_the_wallet() {
      let used = RecipientAdvice {
         fresh: Some(false),
         ..RecipientAdvice::default()
      };
      assert_eq!(used.link(), None, "history alone is not a link");

      let unshielded = RecipientAdvice {
         prior_unshields: 3,
         ..used
      };
      assert_eq!(
         unshielded.link(),
         Some(RecipientLink::PriorUnshields(3))
      );

      let transacted = RecipientAdvice {
         transacted_with: true,
         ..used
      };
      assert_eq!(transacted.link(), Some(RecipientLink::Transacted));

      let ours = RecipientAdvice {
         is_own_wallet: true,
         ..transacted
      };
      assert_eq!(
         ours.link(),
         Some(RecipientLink::OwnWallet),
         "the loudest link is the one to report"
      );

      let unknown = RecipientAdvice {
         fresh: None,
         ..RecipientAdvice::default()
      };
      assert_eq!(
         unknown.link(),
         None,
         "an unanswered chain call is not a verdict"
      );
   }

   /// The pool holds what the chain recorded; the form holds what the user typed. The two are not the
   /// same number, and a check that compares them matches nothing.
   #[test]
   fn a_typed_amount_is_converted_to_what_the_chain_records() {
      assert_eq!(
         onchain_amount(5_500_000_000_000_000, 0.25),
         5_486_250_000_000_000,
         "a 0.0055 shield is recorded as 0.00548625"
      );
      assert_eq!(
         typed_amount(5_985_000_000_000_000, 0.25),
         6_000_000_000_000_000,
         "what to type for the pool to see 0.006"
      );

      for typed in [
         5_486_250_000_000_000u128,
         100_000_000_000_000_000,
         1_000_000_000_000_000_000,
      ] {
         let back = typed_amount(onchain_amount(typed, 0.25), 0.25);
         assert!(
            back >= typed,
            "{back} is below the {typed} that was typed"
         );
      }
   }

   /// An amount belongs to its chain; a recipient address does not.
   #[test]
   fn an_amount_is_read_on_its_chain_and_a_recipient_on_every_chain() {
      let on_mainnet = shield_event(WETH, 1_000);
      let on_sepolia = unshield_event(WETH.erc20_address().unwrap(), 400, RECIPIENT);

      let history = own_history(
         [
            tx(1, 10, 1_000, &on_mainnet),
            tx(11_155_111, 11, 1_001, &on_sepolia),
         ],
         Some(&WETH),
         11_155_111,
         RECIPIENT,
      );

      assert!(
         history.shields.is_empty(),
         "a mainnet deposit says nothing about a sepolia withdrawal"
      );
      assert_eq!(history.unshields.len(), 1);
      assert_eq!(
         history.prior_unshields_to(RECIPIENT),
         1,
         "the address is public on every chain"
      );
      assert!(
         history.transacted_with,
         "the unshield names the recipient"
      );
   }

   /// A transfer buried in a transaction still ties the address to the wallet.
   #[test]
   fn a_transfer_that_is_not_the_main_event_still_counts_as_a_link() {
      let shield = shield_event(WETH, 1_000);
      let paid = transfer(RECIPIENT);

      let mut paid_to_them = tx(1, 10, 1_000, &shield);
      paid_to_them.events = std::slice::from_ref(&paid);

      assert!(own_history([paid_to_them], Some(&WETH), 1, RECIPIENT).transacted_with);

      let elsewhere = transfer(address!(
         "4444444444444444444444444444444444444444"
      ));
      let mut unrelated = tx(1, 10, 1_000, &shield);
      unrelated.events = std::slice::from_ref(&elsewhere);

      assert!(!own_history([unrelated], Some(&WETH), 1, RECIPIENT).transacted_with);
   }
}
