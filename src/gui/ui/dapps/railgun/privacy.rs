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

/// A withdrawal taking more than this share of the pool is a signal of its own.
pub const LARGE_POOL_SHARE: f64 = 0.25;

/// How deep the pool is for the asset being unshielded, and how much of it this withdrawal is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoolDepth {
   /// What the Railgun contract holds of this token: everything shielded, in the token's units.
   pub balance_wei: U256,
   /// What that is worth, when Zeus can price the token at all.
   pub usd: Option<f64>,
   /// The share of the pool this withdrawal would take, when there is a pool to speak of.
   pub share: Option<f64>,
}

impl PoolDepth {
   /// Too small to hide in, unvalued, or mostly this one withdrawal.
   ///
   /// An unpriceable token is deliberately treated as shallow: if its size cannot be established,
   /// nothing here can say it is deep, and saying nothing would read as reassurance.
   pub fn is_shallow(&self) -> bool {
      self.usd.is_none_or(|usd| usd < SHALLOW_POOL_USD)
         || self.share.is_some_and(|share| share > LARGE_POOL_SHARE)
   }
}

/// Whether the address receiving the unshield has been used before.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecipientAdvice {
   /// `Some(true)` when the address has no on-chain history at all, `None` when the chain could not
   /// be asked — which is not the same answer as "fresh".
   pub fresh: Option<bool>,
   /// How many times this wallet has already unshielded to this address.
   pub prior_unshields: usize,
}

impl RecipientAdvice {
   /// An address that has been used — by anyone, or by this wallet's earlier unshields.
   pub fn is_reused(&self) -> bool {
      self.fresh == Some(false) || self.prior_unshields > 0
   }
}

/// What the unshield form shows above its button.
#[derive(Debug, Clone, PartialEq)]
pub struct UnshieldPrivacy {
   /// The amount verdict, when the pool could be read and scored.
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
   pub hash: TxHash,
   pub block: u64,
   pub timestamp: u64,
   pub event: &'a DecodedEvent,
}

/// The wallet's own activity, split into what the amount check needs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OwnHistory {
   /// Shields of the asset being unshielded.
   pub shields: Vec<Deposit>,
   /// Unshields of the asset being unshielded, from the tx history and the private history.
   pub unshields: Vec<Deposit>,
   /// How many unshields each address has received.
   pub unshields_to: HashMap<Address, usize>,
}

impl OwnHistory {
   pub fn prior_unshields_to(&self, recipient: Address) -> usize {
      self.unshields_to.get(&recipient).copied().unwrap_or(0)
   }
}

/// Fold the wallet's recorded transactions into the deposits its own history implies.
///
/// Only the asset being unshielded is kept — another token's amounts say nothing about this one —
/// while recipient reuse is counted across every asset, since the address is public either way.
pub fn own_history<'a>(txs: impl IntoIterator<Item = OwnTx<'a>>, asset: &AssetId) -> OwnHistory {
   let mut history = OwnHistory::default();

   for tx in txs {
      match tx.event {
         DecodedEvent::Shield(params) => {
            if params.asset != *asset {
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
            *history.unshields_to.entry(params.recipient).or_default() += 1;

            let unshielded: AssetId = params.token_data.clone().into();
            if unshielded != *asset {
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
   let mut privacy = UnshieldPrivacy {
      recipient: recipient_advice(&ctx, chain, recipient, owner).await,
      ..UnshieldPrivacy::empty()
   };

   let Some(token) = fungible_token(asset) else {
      return privacy;
   };
   let asset_id = AssetId::Erc20(token.address);

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

   let own = own_activity(&ctx, chain, &provider, &asset_id, owner).await;

   match u128::try_from(amount_wei) {
      Ok(amount_wei) => {
         match assess_amount(
            &pool,
            amount_wei,
            token.decimals,
            now,
            &own.shields,
            &own.unshields,
         ) {
            Ok(advice) => privacy.amount = Some(advice),
            Err(e) => privacy.unavailable = Some(format!("{e}")),
         }

         privacy.pool = pool_depth(&ctx, chain, &provider, &token, amount_wei).await;
      }
      Err(_) => privacy.unavailable = Some("enter an amount first".to_string()),
   }

   privacy
}

/// The wallet's own shields and unshields of `asset`, from the tx history and the private history.
///
/// The two sources overlap — a Zeus-made unshield is in both — so the private side only adds what the
/// tx history does not already have. A withdrawal counted twice would be read as a larger partial
/// withdrawal than it was, which is exactly the number the remainder check depends on.
async fn own_activity(
   ctx: &ZeusCtx,
   chain: ChainId,
   provider: &RailgunProvider<RpcClient>,
   asset: &AssetId,
   owner: Address,
) -> OwnHistory {
   let txs = ctx.tx_db().get_txs(chain.id(), owner).unwrap_or_default();

   let mut history = own_history(
      txs.iter().map(|tx| OwnTx {
         hash: tx.hash,
         block: tx.block,
         timestamp: tx.timestamp.timestamp(),
         event: &tx.main_event,
      }),
      asset,
   );

   let Some(railgun_address) = ctx.current_wallet_info().railgun_address else {
      return history;
   };

   // The transactions the tx history already accounts for: a Zeus-made unshield spends its note in
   // the very transaction the receipt recorded, so the hashes are the dedupe key. A private-history
   // row without a hash can only be matched on what it holds.
   let mut counted: HashSet<TxHash> = txs
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
   })
}

/// Whether the chain says this address has ever transacted, and what this wallet remembers.
///
/// Two independent signals: the nonce (any activity at all, by anyone) and this wallet's own record
/// of unshielding to it. Either one makes the address a link.
async fn recipient_advice(
   ctx: &ZeusCtx,
   chain: ChainId,
   recipient: Address,
   owner: Address,
) -> RecipientAdvice {
   let nonce = ctx.get_transaction_count(recipient).await.ok();

   let prior_unshields = ctx
      .tx_db()
      .get_txs(chain.id(), owner)
      .map(|txs| {
         txs.iter()
            .filter(|tx| match &tx.main_event {
               DecodedEvent::Unshield(params) => params.recipient == recipient,
               _ => false,
            })
            .count()
      })
      .unwrap_or(0);

   RecipientAdvice {
      fresh: nonce.map(|nonce| nonce == 0),
      prior_unshields,
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::alloy_primitives::{U256, address};
   use zeus_eth::currency::Currency;
   use zeus_railgun::abi::railgun::{TokenData, TokenType};

   use crate::core::tx::events::{ShieldParams, UnshieldParams};

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

   fn tx<'a>(block: u64, timestamp: u64, event: &'a DecodedEvent) -> OwnTx<'a> {
      OwnTx {
         hash: TxHash::from([block as u8; 32]),
         block,
         timestamp,
         event,
      }
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
            tx(10, 1_000, &our_shield),
            tx(11, 1_001, &other_shield),
            tx(12, 1_002, &our_unshield),
            tx(13, 1_003, &other_unshield),
            tx(14, 1_004, &elsewhere),
         ],
         &WETH,
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
      };
      assert!(empty.is_shallow());
   }

   /// A recipient is reused when the chain says it has a history, or when this wallet has already
   /// unshielded to it — and unknown is not the same answer as fresh.
   #[test]
   fn a_recipient_is_reused_by_history_or_by_prior_unshields() {
      assert!(
         !RecipientAdvice {
            fresh: Some(true),
            prior_unshields: 0,
         }
         .is_reused()
      );
      assert!(
         RecipientAdvice {
            fresh: Some(false),
            prior_unshields: 0,
         }
         .is_reused()
      );
      assert!(
         RecipientAdvice {
            fresh: Some(true),
            prior_unshields: 2,
         }
         .is_reused(),
         "an address this wallet already unshielded to is not fresh in any useful sense"
      );
      assert!(
         !RecipientAdvice {
            fresh: None,
            prior_unshields: 0,
         }
         .is_reused(),
         "an unanswered chain call is not a verdict"
      );
   }
}
