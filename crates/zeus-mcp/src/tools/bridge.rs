//! The Across bridge: one deposit, from one chain to an address on another.
//!
//! The recipient lives on the destination chain, which is why this is the one money tool whose screen reads a
//! chain other than the one the transaction happens on. A recipient that chain cannot be read for is refused
//! outright: sending unscreened would make the guard depend on which RPCs happen to be up.

use super::tx::wei_amount;
use super::{
   Call, ToolFuture, ToolSpec, action, dry_run_argument, exec_error, optional_address,
   priority_argument, required_address, required_chain, string_argument, wallet_argument,
};
use crate::error::ToolError;
use crate::protocol::types::Annotations;
use serde_json::Value;
use zeus_core::agent::exec::{self, BridgeRequest, ExecCall};
use zeus_eth::currency::ERC20Token;

pub const BRIDGE: ToolSpec = ToolSpec {
   name: "tx_bridge",
   title: "Bridge funds",
   description: "Bridge the chain's own coin to another chain through Across. The recipient is screened on the destination chain and the bridge is refused when that chain cannot be read, or when the recipient is a contract there. The amount is the deposit before the bridge's fee; there is no dry run, because the deposit's calldata and its fee come from the bridge itself.",
   schema: r#"{"type": "object", "properties": {
      "wallet": {"type": "string", "description": "The wallet to bridge from, as wallet_list shows it."},
      "chain": {"type": "integer", "description": "The chain the money leaves."},
      "to_chain": {"type": "integer", "description": "The chain the money lands on."},
      "to": {"type": "string", "description": "The recipient on the destination chain, full and 0x-prefixed. A contract address is refused."},
      "amount": {"type": "string", "description": "How much to deposit, in whole units, as a decimal string such as 0.25. Never a JSON number."},
      "dry_run": {"type": "boolean", "description": "Refused for a bridge: the deposit is built from the bridge's own fee quote, so there is nothing to simulate before the flow runs."},
      "priority": {"type": "string", "enum": ["normal", "fast", "very_fast"], "description": "The fee tier; fast by default."},
      "action_id": {"type": "string", "description": "A client-chosen id. Repeating a call with the same id and arguments replays the first answer instead of bridging twice."}
   }, "required": ["wallet", "chain", "to_chain", "to", "amount"], "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some("wallet.write.bridge"),
   call: bridge,
};

fn bridge(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let request = bridge_request(&arguments)?;
      let wanted = ExecCall::Bridge(request);
      let capability = wanted.capability();

      if dry_run_argument(&arguments)? {
         let value = exec::preview(call.ctx.clone(), &call.profile, &wanted)
            .await
            .map_err(|error| exec_error(error, capability))?;

         return Ok(action::preview(value, &arguments));
      }

      let held = call.clone();

      action::run(call, &arguments, async move {
         Ok(super::tx::perform(&held, "tx_bridge", capability, wanted).await?)
      })
      .await
   })
}

/// The request a bridge describes.
///
/// The deposit is made in the chain's wrapped native token, so that is the asset the grant's allowlist has to
/// carry. A call may name it, and naming anything else is refused: it is not an asset this flow can send.
fn bridge_request(arguments: &Value) -> Result<BridgeRequest, ToolError> {
   let chain = required_chain(arguments)?;
   let token = optional_address(arguments, "token")?
      .unwrap_or_else(|| ERC20Token::wrapped_native_token(chain).address);

   // The bridge only deposits the source chain's own coin, in its wrapped form.
   if token != ERC20Token::wrapped_native_token(chain).address {
      return Err(ToolError::validation(
         "a bridge deposits the chain's own coin: token is the wrapped native token or absent",
      ));
   }

   Ok(BridgeRequest {
      chain,
      dest_chain: super::required_chain_key(arguments, "to_chain")?,
      from: wallet_argument(arguments)?,
      to: required_address(arguments, "to")?,
      token,
      amount: wei_amount(string_argument(arguments, "amount")?, 18)?,
      decimals: 18,
      priority: priority_argument(arguments)?,
   })
}
