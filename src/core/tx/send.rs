use crate::core::{
   TransactionAnalysis, TransactionRich, ZeusCtx, client::CLIENT_TIMEOUT_FOR_SENDING_TX,
};
use crate::utils::{state::get_base_fee, wait_confirm_window, wait_tx_confirm};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use anyhow::anyhow;
use std::time::Duration;

use super::finalize::{MainEvent, MinedTx, RecordPolicy, build_tx_outcome, record_and_notify};
use crate::core::clear_signing::ClearDisplay;
use crate::core::tx::{diffs_from_receipt, simulate_and_diff};
use crate::gui::{SHARED_GUI, ui::NotificationType};
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_network::{
      Ethereum, NetworkTransactionBuilder, TransactionBuilder, TransactionBuilder7702,
   },
   alloy_primitives::{Address, Bytes, U256},
   alloy_rpc_types::{BlockId, TransactionReceipt, TransactionRequest},
   alloy_signer::SignerSync,
   types::ChainId,
   utils::NumericValue,
};
use zeus_wallet::SecureKey;

pub(crate) fn signed_7702_authorization(
   signer: &SecureKey,
   chain_id: u64,
   delegate_to: Address,
   auth_nonce: u64,
) -> Result<SignedAuthorization, anyhow::Error> {
   let auth = Authorization {
      chain_id: U256::from(chain_id),
      address: delegate_to,
      nonce: auth_nonce,
   };
   let signature = signer.to_signer().sign_hash_sync(&auth.signature_hash())?;
   Ok(auth.into_signed(signature))
}

/// A transaction that is ready to be signed and broadcast.
///
/// Built from a [`SendTxRequest`] once the confirmation window has produced the
/// user's fee and gas edits. Several of these fields share a type, so they are
/// set through named setters instead of one eleven-argument constructor.
#[derive(Clone)]
pub struct TxParams {
   pub signer: SecureKey,
   /// The address the signed call is made to.
   pub transact_to: Address,
   pub nonce: u64,
   pub value: U256,
   pub chain: ChainId,
   /// The miner tip the user picked.
   pub miner_tip: U256,
   /// The base fee of the next block.
   pub base_fee: u64,
   pub call_data: Bytes,
   pub gas_limit: u64,
   pub authorization_list: Vec<SignedAuthorization>,
}

impl TxParams {
   /// `signer` pays and signs for `transact_to` on `chain`, spending `nonce` and
   /// moving `value` with `call_data`.
   ///
   /// Every argument has a distinct type, so the order cannot be got wrong by
   /// accident. The fee and gas inputs come from the confirmation window and are
   /// set separately — see [`Self::fees`] and [`Self::gas_limit`].
   pub fn new(
      signer: SecureKey,
      chain: ChainId,
      transact_to: Address,
      nonce: u64,
      value: U256,
      call_data: Bytes,
   ) -> Self {
      Self {
         signer,
         transact_to,
         nonce,
         value,
         chain,
         miner_tip: U256::ZERO,
         base_fee: 0,
         call_data,
         gas_limit: 0,
         authorization_list: Vec::new(),
      }
   }

   /// The two fee inputs. Both are plain numbers that cannot be told apart once
   /// they are in the struct, so they are set together and by name.
   #[must_use]
   pub fn fees(mut self, miner_tip: U256, base_fee: u64) -> Self {
      self.miner_tip = miner_tip;
      self.base_fee = base_fee;
      self
   }

   /// The gas limit the user approved. Left at zero the RPC rejects the send.
   #[must_use]
   pub fn gas_limit(mut self, gas_limit: u64) -> Self {
      self.gas_limit = gas_limit;
      self
   }

   #[must_use]
   pub fn authorization_list(mut self, authorization_list: Vec<SignedAuthorization>) -> Self {
      self.authorization_list = authorization_list;
      self
   }

   pub fn max_fee_per_gas(&self) -> U256 {
      let fee = self.miner_tip + U256::from(self.base_fee);
      // add a 10% tolerance
      fee * U256::from(110) / U256::from(100)
   }
}

/// What a flow asks Zeus to send: the transaction, plus the analysis the caller
/// already built for it.
///
/// These used to be nine positional arguments — two of them addresses, two of
/// them numbers — so a swapped pair still compiled. The fields a send cannot
/// invent are required by [`SendTxRequest::new`] and there is deliberately no
/// `Default` impl, so `from` and `interact_to` can never quietly become the zero
/// address.
///
/// ```ignore
/// let (receipt, tx_rich) = send_transaction(
///    ctx,
///    false,
///    SendTxRequest::new(chain, from, router).call_data(call_data).value(value),
///    SendTxOptions {
///       dapp: origin,
///       mev_protect: true,
///       ..Default::default()
///    },
/// )
/// .await?;
/// ```
#[derive(Clone)]
pub struct SendTxRequest {
   pub chain: ChainId,
   /// The wallet that signs and pays.
   pub from: Address,
   /// The address the call is made to.
   pub interact_to: Address,
   pub call_data: Bytes,
   pub value: U256,
   pub authorization_list: Vec<SignedAuthorization>,
   /// An analysis the caller already built. `None` makes the send simulate the
   /// call on a fork and analyze the result before opening the confirm window.
   pub analysis: Option<TransactionAnalysis>,
}

impl SendTxRequest {
   /// A call from `from` to `interact_to` on `chain`, carrying no calldata, no
   /// value, no authorization list and no analysis.
   pub fn new(chain: ChainId, from: Address, interact_to: Address) -> Self {
      Self {
         chain,
         from,
         interact_to,
         call_data: Bytes::default(),
         value: U256::ZERO,
         authorization_list: Vec::new(),
         analysis: None,
      }
   }

   #[must_use]
   pub fn call_data(mut self, call_data: Bytes) -> Self {
      self.call_data = call_data;
      self
   }

   #[must_use]
   pub fn value(mut self, value: U256) -> Self {
      self.value = value;
      self
   }

   #[must_use]
   pub fn authorization_list(mut self, authorization_list: Vec<SignedAuthorization>) -> Self {
      self.authorization_list = authorization_list;
      self
   }

   /// Skip the pre-send simulation: the caller simulated this exact call and
   /// built the analysis itself. Callers whose analysis is optional (the approve
   /// helper) assign [`Self::analysis`] directly.
   #[must_use]
   pub fn analysis(mut self, analysis: TransactionAnalysis) -> Self {
      self.analysis = Some(analysis);
      self
   }
}

/// Options for [`send_transaction`].
#[derive(Default, Clone)]
pub struct SendTxOptions {
   pub mev_protect: bool,
   /// Present the transaction as sponsored in the confirm window.
   pub sponsored: bool,
   pub dapp: String,
   /// Record the analysis' own main event instead of inferring one from the
   /// receipt logs — Railgun Transact logs are not public ERC-20 transfers.
   pub keep_intent_event: bool,
}

/// What the user left in the transaction confirmation window.
pub struct ConfirmedTx {
   pub priority_fee: NumericValue,
   pub gas_limit: u64,
   pub clear_display: Option<ClearDisplay>,
}

/// Open the transaction confirmation window, wait for the user, and hand back
/// their edits.
///
/// Flows that do not broadcast with `send_tx` — the sponsored unshield submits to
/// a bundler instead — use this directly.
pub async fn confirm_tx(
   ctx: ZeusCtx,
   source_is_zeus: bool,
   chain: ChainId,
   analysis: &TransactionAnalysis,
   opts: &SendTxOptions,
) -> Result<ConfirmedTx, anyhow::Error> {
   let priority_fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();

   SHARED_GUI.write(|gui| {
      gui.tx_confirmation_window.open(
         ctx.clone(),
         source_is_zeus,
         opts.dapp.clone(),
         chain,
         analysis.clone(),
         priority_fee.f64().to_string(),
         opts.mev_protect,
         opts.sponsored,
      );
      gui.loading_window.reset();
      gui.bring_to_front();
   });

   if !wait_tx_confirm().await {
      return Err(anyhow!("Transaction rejected"));
   }

   Ok(SHARED_GUI.read(|gui| ConfirmedTx {
      priority_fee: gui.tx_confirmation_window.get_priority_fee(),
      gas_limit: gui.tx_confirmation_window.get_gas_limit(),
      clear_display: gui.tx_confirmation_window.get_clear_display(),
   }))
}

/// Simulate the call unless the request already carries an analysis, ask the
/// user to confirm, broadcast, and record the result.
///
/// Every flow that broadcasts through a normal RPC goes through here. The
/// sponsored Railgun unshield submits to a bundler instead and confirms through
/// [`confirm_tx`].
pub async fn send_transaction(
   ctx: ZeusCtx,
   source_is_zeus: bool,
   req: SendTxRequest,
   opts: SendTxOptions,
) -> Result<(TransactionReceipt, TransactionRich), anyhow::Error> {
   let SendTxRequest {
      chain,
      from,
      interact_to,
      call_data,
      value,
      authorization_list,
      analysis,
   } = req;

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
      gui.request_repaint();
   });

   let client = ctx.get_zeus_client();

   let base_fee_fut = get_base_fee(ctx.clone(), chain.id());
   let nonce_fut = client.request(chain.id(), |client| async move {
      client.get_transaction_count(from).await.map_err(|e| anyhow!("{:?}", e))
   });

   let mut tx_analysis = if let Some(analysis) = analysis {
      analysis
   } else {
      let simulated = simulate_and_diff(
         ctx.clone(),
         chain,
         from,
         interact_to,
         call_data.clone(),
         value,
         authorization_list.clone(),
      )
      .await?;

      let mut analysis = TransactionAnalysis::new(
         ctx.clone(),
         chain.id(),
         from,
         interact_to,
         Some(simulated.contract_interact),
         call_data.clone(),
         value,
         simulated.logs,
         simulated.sim_res.tx_gas_used(),
         simulated.balance_before,
         simulated.balance_after,
         authorization_list.clone(),
      )
      .await?;
      analysis.set_diffs(simulated.balance_diff, simulated.approval_diff);
      analysis
   };

   let involved_currencies = tx_analysis.involved_currencies();
   let price_manager = ctx.price_manager();
   let pool_manager = ctx.pool_manager();

   let tokens = involved_currencies
      .iter()
      .map(|c| c.to_erc20().into_owned())
      .collect::<Vec<_>>();

   let time = std::time::Instant::now();

   if let Err(e) = price_manager
      .calculate_prices(ctx.clone(), chain.id(), pool_manager, tokens)
      .await
   {
      tracing::error!("Error updating prices: {:?}", e);
   }

   tracing::info!(
      "Updated prices in {} ms",
      time.elapsed().as_millis()
   );

   tx_analysis.refresh_usd(&ctx);

   let confirmed = confirm_tx(
      ctx.clone(),
      source_is_zeus,
      chain,
      &tx_analysis,
      &opts,
   )
   .await?;

   let main_event = tx_analysis.infer_main_event(ctx.clone(), chain.id());
   let main_event_name = if main_event.is_known() {
      main_event.name()
   } else {
      "Transaction in progress".to_string()
   };

   let notification = NotificationType::from_main_event(main_event);

   SHARED_GUI.write(|gui| {
      gui.notification.open_with_spinner(main_event_name, notification);
      gui.request_repaint();
   });

   let priority_fee = if confirmed.priority_fee.is_zero() {
      ctx.get_priority_fee(chain.id()).unwrap_or_default()
   } else {
      confirmed.priority_fee
   };

   let base_fee = base_fee_fut.await?;
   let nonce = nonce_fut.await?;
   let signer = ctx.get_wallet(from).ok_or(anyhow!("Wallet not found"))?.key;

   let tx_params = TxParams::new(
      signer,
      chain,
      interact_to,
      nonce,
      value,
      call_data.clone(),
   )
   .fees(priority_fee.wei(), base_fee.next)
   .gas_limit(confirmed.gas_limit)
   .authorization_list(authorization_list.clone());

   let rpc = client.get_best_rpc(chain.id()).ok_or(anyhow!("No available RPC found"))?;
   let tx_client = client.connect_with_timeout(&rpc, CLIENT_TIMEOUT_FOR_SENDING_TX).await?;

   // If needed use MEV protect client, if not found prompt the user to continue
   let send_client = if opts.mev_protect {
      match ctx.get_mev_protect_client(chain.id()).await {
         Ok(mev_client) => mev_client,
         Err(_) => {
            SHARED_GUI.write(|gui| {
               let msg2 = "Continue without MEV protection?";
               if source_is_zeus {
                  gui.confirm_window.open("No available MEV protect RPC found");
               } else {
                  gui.confirm_window.open_from_dapp("No available MEV protect RPC found");
               }
               gui.confirm_window.set_msg2(msg2);
               gui.request_repaint();
            });

            if !wait_confirm_window().await {
               return Err(anyhow!("Transaction rejected"));
            }

            tx_client
         }
      }
   } else {
      tx_client
   };

   let receipt = send_tx(send_client, tx_params).await?;
   let tx_block = receipt.block_number.unwrap_or(0);
   let block_id = if tx_block > 0 {
      BlockId::number(tx_block)
   } else {
      BlockId::latest()
   };

   let logs: Vec<_> = receipt.logs().iter().cloned().map(|l| l.into_inner()).collect();

   let balance_after = client
      .request(chain.id(), |client| async move {
         client
            .get_balance(from)
            .block_id(block_id)
            .await
            .map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   // Prefer the diffs the receipt proves; fall back to the pre-send simulation
   // when the receipt read comes back empty.
   let diffs = match diffs_from_receipt(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      &tx_analysis.call_data,
      &logs,
      tx_block,
      balance_after,
   )
   .await
   {
      Ok((balance, approval)) if !balance.is_empty() || !approval.is_empty() => {
         Some((balance, approval))
      }
      other => {
         if let Err(e) = other {
            tracing::warn!("receipt diffs failed: {:?}", e);
         }
         Some((
            tx_analysis.balance_diff.clone(),
            tx_analysis.approval_diff.clone(),
         ))
      }
   };

   let outcome = build_tx_outcome(
      ctx.clone(),
      chain,
      MinedTx {
         from,
         interact_to,
         call_data: tx_analysis.call_data.clone(),
         value: tx_analysis.value,
         logs,
         eth_balance_before: tx_analysis.eth_balance_before,
         eth_balance_after: balance_after,
         contract_interact: Some(tx_analysis.contract_interact),
         authorization_list,
         tx_type: receipt.transaction_type(),
         block: tx_block,
         gas_used: receipt.gas_used,
         hash: receipt.transaction_hash,
         success: receipt.status(),
      },
      // Flows that know what they asked for record that instead of a guess.
      if opts.keep_intent_event {
         MainEvent::intent_or_inferred(tx_analysis.main_event_opt().cloned())
      } else {
         MainEvent::Inferred
      },
      RecordPolicy {
         diffs,
         confirm_clear: confirmed.clear_display,
         clear_signing: true,
         // Zeus-originated swaps already have a SwapToken main-event override.
         // Connector / inferred swaps do not — those keep the log heuristic.
         onchain_swap_received: tx_analysis.main_event_opt().is_some_and(|e| e.is_swap()),
         priority_fee,
         ..Default::default()
      },
   )
   .await?;

   record_and_notify(ctx.clone(), chain, from, &outcome)?;

   Ok((receipt, outcome.tx_rich))
}

pub async fn delegate_to(
   ctx: ZeusCtx,
   source_is_zeus: bool,
   chain: ChainId,
   from: Address,
   delegate_to: Address,
) -> Result<(), anyhow::Error> {
   let wallet = ctx.get_wallet(from).ok_or(anyhow!("Wallet not found"))?.key;
   let client = ctx.get_zeus_client();

   if !delegate_to.is_zero() {
      let code = client
         .request(chain.id(), |client| async move {
            client.get_code_at(delegate_to).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if code.is_empty() {
         return Err(anyhow!(
            "Code is empty, you can only delegate to a smart contract address"
         ));
      }
   }

   let address = wallet.address();

   let nonce = client
      .request(chain.id(), |client| async move {
         client.get_transaction_count(address).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let auth_nonce = nonce + 1;

   let signed_authorization =
      signed_7702_authorization(&wallet, chain.id(), delegate_to, auth_nonce)?;

   send_transaction(
      ctx.clone(),
      source_is_zeus,
      SendTxRequest::new(chain, from, from).authorization_list(vec![signed_authorization]),
      SendTxOptions::default(),
   )
   .await?;

   if delegate_to.is_zero() {
      ctx.write(|ctx| {
         ctx.delegated_wallets.remove(chain.id(), from);
      });
   } else {
      ctx.write(|ctx| {
         ctx.delegated_wallets.add(chain.id(), from, delegate_to);
      });
   }

   Ok(())
}

pub async fn send_tx<P>(client: P, params: TxParams) -> Result<TransactionReceipt, anyhow::Error>
where
   P: Provider<Ethereum> + Clone + 'static,
{
   let tx = make_tx_request(&params);
   let wallet = params.signer.to_wallet();
   let tx_envelope = tx.build(&wallet).await?;
   drop(wallet);

   let time = std::time::Instant::now();
   let receipt = client
      .send_tx_envelope(tx_envelope)
      .await?
      .with_timeout(Some(Duration::from_secs(
         CLIENT_TIMEOUT_FOR_SENDING_TX,
      )))
      .get_receipt()
      .await?;
   tracing::info!(
      "Time take to send tx: {:?}secs",
      time.elapsed().as_secs_f32()
   );

   Ok(receipt)
}

fn make_tx_request(params: &TxParams) -> TransactionRequest {
   if params.chain.supports_type_2_tx() {
      let mut tx = TransactionRequest::default()
         .with_from(params.signer.address())
         .with_to(params.transact_to)
         .with_chain_id(params.chain.id())
         .with_value(params.value)
         .with_nonce(params.nonce)
         .with_input(params.call_data.clone())
         .with_gas_limit(params.gas_limit)
         .with_max_priority_fee_per_gas(params.miner_tip.to::<u128>())
         .max_fee_per_gas(params.max_fee_per_gas().to::<u128>());

      if !params.authorization_list.is_empty() {
         tx.set_authorization_list(params.authorization_list.clone());
      }

      tx
   } else {
      TransactionRequest::default()
         .with_from(params.signer.address())
         .with_to(params.transact_to)
         .with_value(params.value)
         .with_nonce(params.nonce)
         .with_input(params.call_data.clone())
         .with_gas_limit(params.gas_limit)
         .with_gas_price(params.base_fee.into())
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// `send_transaction` fills in nothing but the dapp and the MEV flag, so its
   /// callers keep the plain behavior: no sponsorship, no intent override.
   #[test]
   fn default_send_options_are_plain() {
      let opts = SendTxOptions::default();
      assert!(!opts.mev_protect);
      assert!(!opts.sponsored);
      assert!(!opts.keep_intent_event);
      assert!(opts.dapp.is_empty());
   }

   /// A freshly built request carries nothing the caller did not ask for, and
   /// the defaults it does carry are the inert ones.
   #[test]
   fn new_request_is_inert() {
      let req = SendTxRequest::new(
         ChainId::Ethereum,
         Address::from([1u8; 20]),
         Address::from([2u8; 20]),
      );

      assert_eq!(req.chain.id(), 1);
      assert!(req.call_data.is_empty());
      assert_eq!(req.value, U256::ZERO);
      assert!(req.authorization_list.is_empty());
      assert!(req.analysis.is_none());
   }
}
