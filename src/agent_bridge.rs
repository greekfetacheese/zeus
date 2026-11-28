//! The Across bridge, as the agent pipeline calls it.
//!
//! A deposit is priced from the bridge's own fee source — the Across API's relay fee when the page asks it,
//! the user's stored cut when it does not — and sent through the app's deposit screen, which reads the
//! destination chain before anything is built.
//!
//! What is deliberately not here is the page's five-minute wait for the funds to land: an agent's call is
//! answered when the deposit is out, and the app records and watches it from there. A tool call that held
//! the connection open for the crossing would look hung and could be cut off after the money had left.

use crate::core::agent::exec::{BridgeRequest, ExecError};
use crate::core::{AgentEnv, TransactionRich, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::gui::ui::dapps::across::{FILL_WAIT_SECS, get_suggested_fees, send_bridge_deposit};
use std::str::FromStr;
use zeus_eth::abi::protocols::across::{DepositV3Args, encode_deposit_v3};
use zeus_eth::alloy_primitives::{Address, Bytes, U256};
use zeus_eth::currency::ERC20Token;
use zeus_eth::utils::{NumericValue, address_book};

/// Bridge the chain's own coin to another chain.
pub async fn bridge(
   ctx: ZeusCtx,
   env: AgentEnv,
   request: BridgeRequest,
) -> Result<TransactionRich, ExecError> {
   let from_chain = super::agent_tx::chain(request.chain)?;
   let to_chain = super::agent_tx::chain(request.dest_chain)?;

   // The page's bridge deposits the wrapped native token of the source chain, and nothing else. An
   // allowlist entry for another asset names something this flow cannot send.
   let input_token = ERC20Token::wrapped_native_token(from_chain.id());

   if request.token != input_token.address {
      return Err(ExecError::Invalid(format!(
         "a bridge deposits the chain's wrapped native token ({:#x}), and the call named {:#x}",
         input_token.address, request.token
      )));
   }

   if request.amount.is_zero() {
      return Err(ExecError::Invalid(
         "amount is zero: there is nothing to bridge".to_owned(),
      ));
   }

   let input_amount = NumericValue::format_wei(request.amount, request.decimals);
   let output_token = ERC20Token::wrapped_native_token(to_chain.id());
   let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map_err(|error| ExecError::Failed(error.to_string()))?
      .as_secs();
   let deadline = now.saturating_add(FILL_WAIT_SECS).min(u32::MAX as u64) as u32;

   let (use_api, fee_to_pay) = SHARED_GUI.read(|gui| {
      (
         gui.across_bridge.agent_use_api(),
         gui.across_bridge.agent_fee_to_pay(),
      )
   });

   // What the destination gets, and the quote the deposit carries. The API is the only place an exclusive
   // relayer and its timestamp can come from, so a bridge that waits on it is asked for one.
   let (output_amount, relayer, quote_timestamp, exclusivity_deadline) = if use_api {
      let quote = get_suggested_fees(
         input_token.address,
         output_token.address,
         from_chain.id(),
         to_chain.id(),
         request.amount,
      )
      .await
      .map_err(|error| {
         ExecError::Failed(format!(
            "the bridge's fee quote did not answer: {error}"
         ))
      })?;

      let fees = &quote.suggested_fees;
      let pct = U256::from_str(&fees.total_relay_fee.pct).unwrap_or_default();
      let scale = U256::from(10).pow(U256::from(request.decimals));
      let fee = (request.amount * pct) / scale;

      let timestamp = u32::from_str(&fees.timestamp).map_err(|_| {
         ExecError::Failed("the bridge's fee quote carried no timestamp".to_owned())
      })?;

      (
         request.amount.saturating_sub(fee),
         fees.exclusive_relayer,
         timestamp,
         fees.exclusivity_deadline,
      )
   } else {
      // The page's manual mode: the user's own cut stands in for the API's relay fee.
      let minimum = input_amount.calc_slippage(fee_to_pay, request.decimals);

      (
         minimum.wei(),
         Address::ZERO,
         now.saturating_sub(60) as u32,
         0,
      )
   };

   if output_amount.is_zero() {
      return Err(ExecError::Failed(
         "the bridge fee leaves nothing to deposit".to_owned(),
      ));
   }

   let call_data = encode_deposit_v3(DepositV3Args {
      depositor: request.from,
      recipient: request.to,
      input_token: input_token.address,
      output_token: output_token.address,
      input_amount: request.amount,
      output_amount,
      destination_chain_id: to_chain.id(),
      exclusive_relayer: relayer,
      quote_timestamp,
      fill_deadline: deadline,
      exclusivity_deadline,
      message: Bytes::default(),
   });

   let interact_to = address_book::across_spoke_pool_v2(from_chain.id())
      .map_err(|error| ExecError::Failed(error.to_string()))?;

   send_bridge_deposit(
      ctx,
      env,
      from_chain,
      to_chain,
      request.from,
      request.to,
      interact_to,
      call_data,
      input_amount,
      None,
   )
   .await
   .map_err(super::agent_tx::classify)
}
