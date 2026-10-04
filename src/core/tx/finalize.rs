//! Turn a mined transaction into the recorded model the app displays.

use super::analysis::TransactionAnalysis;
use super::approval_diff::ApprovalDiff;
use super::balance_diff::BalanceDiff;
use super::events::{DecodedEvent, NftTransferParams};
use super::rich::TransactionRich;
use crate::core::ZeusCtx;
use crate::core::clear_signing::{self, ClearDisplay};
use crate::gui::{SHARED_GUI, ui::NotificationType};
use crate::utils::{RT, TimeStamp, estimate_tx_cost};
use alloy_consensus::TxType;
use alloy_eips::eip7702::SignedAuthorization;
use anyhow::anyhow;
use zeus_eth::{
   alloy_primitives::{Address, Bytes, Log, TxHash, U256},
   types::ChainId,
   utils::NumericValue,
};

/// A mined transaction about to be recorded.
///
/// Deliberately holds the values finalize needs rather than an
/// [`alloy_rpc_types::TransactionReceipt`]: a sponsored unshield's receipt comes
/// from the bundler in a different flavour, and this layer has no business
/// knowing about either.
pub struct MinedTx {
   pub from: Address,
   pub interact_to: Address,
   pub call_data: Bytes,
   pub value: U256,
   /// The receipt's logs, already converted to [`Log`].
   pub logs: Vec<Log>,
   pub eth_balance_before: U256,
   pub eth_balance_after: U256,
   pub contract_interact: Option<bool>,
   pub authorization_list: Vec<SignedAuthorization>,
   /// `Eip7702` for a sponsored unshield, otherwise the receipt's own type.
   pub tx_type: TxType,
   pub block: u64,
   pub gas_used: u64,
   pub hash: TxHash,
   pub success: bool,
}

/// How the recorded main event is chosen.
pub enum MainEvent {
   /// Infer it from the receipt logs.
   Inferred,
   /// Use this event — the caller knows what it asked for, and Railgun Transact
   /// logs are not public ERC-20 transfers.
   Intent(DecodedEvent),
   /// Resolve it against the built analysis. The sponsored unshield merges the
   /// broadcaster fee into the receipt-decoded events before choosing.
   Resolved(Box<dyn FnOnce(&mut TransactionAnalysis) -> DecodedEvent + Send>),
}

impl MainEvent {
   /// [`MainEvent::Intent`] when the caller has an event to record, otherwise
   /// [`MainEvent::Inferred`].
   pub fn intent_or_inferred(event: Option<DecodedEvent>) -> Self {
      match event {
         Some(event) => Self::Intent(event),
         None => Self::Inferred,
      }
   }
}

/// How to finish a [`MinedTx`].
///
/// The default is the plain case — attach no diffs, do no clear-signing lookup —
/// which is enough for a first-party call whose logs tell the whole story.
#[derive(Default)]
pub struct RecordPolicy {
   /// Signer diffs to attach, resolved by the caller from the receipt or from the
   /// pre-send simulation.
   pub diffs: Option<(BalanceDiff, ApprovalDiff)>,
   /// ERC-7730 display already resolved by the confirm window.
   pub confirm_clear: Option<ClearDisplay>,
   /// Try ERC-7730 clear signing when the logs do not explain the call.
   pub clear_signing: bool,
   /// Feed the on-chain swap output back into the analysis (Zeus swaps only).
   pub onchain_swap_received: bool,
   /// Priority fee the user confirmed, for the cost estimate.
   pub priority_fee: NumericValue,
}

/// The recorded transaction plus what its notification needs.
pub struct TxOutcome {
   pub tx_rich: TransactionRich,
   pub main_event_name: String,
   pub notification: NotificationType,
}

/// Build the recorded model and its notification from a mined transaction.
pub async fn build_tx_outcome(
   ctx: ZeusCtx,
   chain: ChainId,
   tx: MinedTx,
   main_event: MainEvent,
   policy: RecordPolicy,
) -> Result<TxOutcome, anyhow::Error> {
   let MinedTx {
      from,
      interact_to,
      call_data,
      value,
      logs,
      eth_balance_before,
      eth_balance_after,
      contract_interact,
      authorization_list,
      tx_type,
      block,
      gas_used,
      hash,
      success,
   } = tx;

   let timestamp = TimeStamp::now_as_secs()?;

   let mut analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      call_data.clone(),
      value,
      logs,
      gas_used,
      eth_balance_before,
      eth_balance_after,
      authorization_list,
   )
   .await?;

   // NFTs that arrived in this transaction join the catalog — the same way a newly seen ERC-20 lands
   // in the currency DB — so the picker can offer them without the user pasting a collection first.
   if analysis.nft_transfers_len() > 0 {
      let transfers = analysis.nft_transfers().clone();
      let ctx2 = ctx.clone();

      RT.spawn(async move {
         discover_nfts(ctx2, chain.id(), from, &transfers).await;
      });
   }

   if let Some((balance, approval)) = policy.diffs {
      analysis.set_diffs(balance, approval);
   }

   if policy.onchain_swap_received {
      if let Err(e) = analysis.apply_onchain_swap_received(ctx.clone(), block).await {
         tracing::warn!("Failed to apply on-chain swap received: {:?}", e);
      }
   }

   let main_event = match main_event {
      MainEvent::Inferred => analysis.infer_main_event(ctx.clone(), chain.id()),
      MainEvent::Intent(event) => event,
      MainEvent::Resolved(resolve) => resolve(&mut analysis),
   };

   let clear_display = if main_event.is_other() {
      if policy.confirm_clear.is_some() {
         policy.confirm_clear
      } else if policy.clear_signing && analysis.contract_interact && call_data.len() >= 4 {
         clear_signing::try_clear_sign_calldata(
            ctx.clone(),
            chain.id(),
            from,
            interact_to,
            analysis.value,
            &call_data,
         )
         .await
      } else {
         None
      }
   } else {
      None
   };

   let main_event_name = if main_event.is_known() {
      main_event.name()
   } else if let Some(display) = &clear_display {
      display.heading.clone()
   } else {
      "Transaction successful".to_string()
   };

   let notification = NotificationType::from_main_event(main_event.clone());

   let (tx_cost, tx_cost_usd) = ctx.write(|ctx| {
      estimate_tx_cost(
         ctx,
         chain.id(),
         gas_used,
         policy.priority_fee.wei(),
      )
   });

   // `tx_rich` carries the main event; leaving it on the analysis too renders it twice.
   analysis.remove_main_event();

   let eth_received_usd = ctx.write(|ctx| analysis.eth_received_usd(ctx));

   let tx_rich = TransactionRich {
      tx_type,
      success,
      chain: chain.id(),
      block,
      timestamp,
      value_sent: analysis.value_sent(),
      value_sent_usd: analysis.value_sent_usd(ctx.clone()),
      eth_received: analysis.eth_received(),
      eth_received_usd,
      tx_cost,
      tx_cost_usd,
      hash,
      contract_interact: analysis.contract_interact,
      analysis,
      main_event,
      clear_display,
   };

   Ok(TxOutcome {
      tx_rich,
      main_event_name,
      notification,
   })
}

/// Track the NFTs that arrived at `owner` in a transaction.
///
/// This is the NFT half of what the currency DB does for ERC-20s: discovery feeds a catalog, and the
/// catalog is what the picker lists. The portfolio is deliberately not written — being in the catalog
/// is enough to see and select a token, and whether it belongs in the portfolio is the user's own
/// call.
///
/// Best-effort: this resolves metadata over the network, and failing to reach it must not fail the
/// transaction record, which is the part the user actually needs.
async fn discover_nfts(ctx: ZeusCtx, chain: u64, owner: Address, transfers: &[NftTransferParams]) {
   for (collection, token_id) in arrived_tokens(owner, transfers) {
      // Already-cached tokens cost no network: `get_nft` answers from the db.
      if let Err(e) = ctx.get_nft(chain, collection, token_id).await {
         tracing::warn!(
            "Failed to track NFT {} #{} from the transaction: {:?}",
            collection,
            token_id,
            e
         );
      }
   }
}

/// The tokens a transaction brought *into* `owner`'s wallet: one entry per moved token, so an
/// ERC-1155 `TransferBatch` contributes every id it carries.
///
/// A burn takes the token away and a transfer to anybody else is not ours to track. An approval is
/// not here at all: it does not arrive as an [`NftTransferParams`], because it changes no ownership
/// — see `NftApproveParams`.
fn arrived_tokens(owner: Address, transfers: &[NftTransferParams]) -> Vec<(Address, U256)> {
   transfers
      .iter()
      .filter(|params| !params.is_burn && params.to == owner)
      .filter_map(|params| params.token_id.map(|token_id| (params.collection, token_id)))
      .collect()
}

/// Persist `outcome` and open its progress-bar notification.
///
/// The transaction is recorded even when it failed, and that failure is then
/// returned so the caller can surface it.
pub fn record_and_notify(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   outcome: &TxOutcome,
) -> Result<(), anyhow::Error> {
   let ctx_clone = ctx.clone();
   let tx = outcome.tx_rich.clone();
   RT.spawn_blocking(move || {
      ctx_clone.add_transaction(chain.id(), from, tx);
   });

   if !outcome.tx_rich.success {
      return Err(anyhow!("Transaction Failed"));
   }

   let now = TimeStamp::now_as_millis()?.timestamp();
   let finish = now + 6000;

   SHARED_GUI.write(|gui| {
      gui.notification.open_with_progress_bar(
         now,
         finish,
         outcome.main_event_name.clone(),
         outcome.notification.clone(),
         Some(outcome.tx_rich.clone()),
      );
      gui.loading_window.reset();
      gui.request_repaint();
   });

   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::{alloy_primitives::address, nft::NftStandard};

   const ME: Address = address!("0x1111111111111111111111111111111111111111");
   const OTHER: Address = address!("0x2222222222222222222222222222222222222222");
   const COLLECTION: Address = address!("0x3333333333333333333333333333333333333333");

   /// A transfer of `token_id` to `to`, as the decoder hands them over.
   fn transfer(to: Address, token_id: Option<u64>) -> NftTransferParams {
      NftTransferParams {
         chain: 1,
         standard: NftStandard::Erc721,
         collection: COLLECTION,
         token_id: token_id.map(U256::from),
         amount: U256::ONE,
         from: OTHER,
         to,
         is_mint: false,
         is_burn: false,
      }
   }

   /// What a transaction brings in is what gets tracked.
   #[test]
   fn a_transfer_to_this_wallet_is_tracked() {
      let arrived = arrived_tokens(ME, &[transfer(ME, Some(7))]);

      assert_eq!(arrived, vec![(COLLECTION, U256::from(7))]);
   }

   /// Everything that is not a token arriving here: a transfer to somebody else, and a burn.
   ///
   /// An approval used to be on this list, and no longer needs to be: it cannot reach this function
   /// at all, since only transfers are handed to it.
   #[test]
   fn nothing_else_is_tracked() {
      let mut burned = transfer(ME, Some(7));
      burned.is_burn = true;

      let transfers = [transfer(OTHER, Some(7)), burned];

      let arrived = arrived_tokens(ME, &transfers);

      assert!(arrived.is_empty(), "{arrived:?}");
   }

   /// An ERC-1155 batch is one log carrying many tokens, so every id has to come through — including
   /// the ones that follow an entry that was skipped.
   #[test]
   fn a_batch_contributes_every_id() {
      let mut first = transfer(ME, Some(1));
      first.standard = NftStandard::Erc1155;

      let mut second = transfer(ME, Some(2));
      second.standard = NftStandard::Erc1155;

      let arrived = arrived_tokens(ME, &[first, transfer(OTHER, Some(3)), second]);

      assert_eq!(
         arrived,
         vec![(COLLECTION, U256::from(1)), (COLLECTION, U256::from(2))]
      );
   }
}
