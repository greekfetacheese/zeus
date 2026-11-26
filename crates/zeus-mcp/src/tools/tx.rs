//! The transactions a wallet recorded, and the two ways an agent starts one.
//!
//! History is the app's own record of what it sent, not a chain explorer: a transaction Zeus did not
//! broadcast is not here. That is why an answer names it "recorded" and why the hash is the identity —
//! the receipt status is the one the app stored when it watched the transaction land.
//!
//! Sending is the first place an agent spends money. The calldata is built from named fields and never
//! from a caller's own bytes, the token or collection is checked against the wallet's allowlist before
//! anything is simulated, and the flow behind the port runs the app's own screens, its own confirmation
//! and its own record. `dry_run` runs the same simulation and stops there.

use super::{
   Call, ToolFuture, ToolOutput, ToolSpec, action, chain_argument, dry_run_argument, exec_error,
   integer_argument, optional_address, priority_argument, read_error, required_address,
   required_chain, string_argument, u256_argument, wallet_argument,
};
use crate::error::ToolError;
use crate::protocol::types::Annotations;
use serde_json::Value;
use zeus_core::agent::exec::{self, ExecCall, NftSendRequest, SendRequest, TxFlows};
use zeus_core::agent::read;
use zeus_eth::alloy_primitives::utils::parse_units;
use zeus_eth::alloy_primitives::{TxHash, U256};
use zeus_eth::nft::NftStandard;

pub const HISTORY: ToolSpec = ToolSpec {
   name: "tx_history",
   title: "Transaction history",
   description: "The transactions one granted wallet recorded, newest first, paged. Amounts are in \
      the token that moved, with its raw integer beside the readable form.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string",
      "description": "The wallet's address, as wallet_list shows it."}, "chain": {"type": "integer",
      "description": "Only this chain; every chain otherwise."}, "page": {"type": "integer",
      "description": "Zero-based page number; 0 by default."}, "per_page": {"type": "integer",
      "description": "Rows per page, 1 to 100; 25 by default."}}, "required": ["wallet"],
      "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("wallet.read.history"),
   call: history,
};

pub const GET: ToolSpec = ToolSpec {
   name: "tx_get",
   title: "One transaction",
   description: "One recorded transaction of one granted wallet, by hash: its block, its outcome, \
      the decoded main event and what it moved.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string",
      "description": "The wallet that recorded the transaction."}, "hash": {"type": "string",
      "description": "The transaction hash, full and 0x-prefixed."}}, "required": ["wallet", "hash"],
      "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("wallet.read.history"),
   call: get,
};

pub const SEND: ToolSpec = ToolSpec {
   name: "tx_send",
   title: "Send funds",
   description: "Send the chain's own coin, or one ERC-20 token, from a granted wallet. The token has \
      to be in that wallet's allowlist and the recipient must not be a contract, both checked before \
      anything is simulated. Answers what the simulation measured, the transaction that was broadcast, \
      and how the grant decided. With dry_run it answers the simulation and sends nothing.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string", "description": "The wallet to send from, as wallet_list shows it."}, "chain": {"type": "integer", "description": "The chain to send on."}, "to": {"type": "string", "description": "The recipient, full and 0x-prefixed. A contract address is refused."}, "token": {"type": "string", "description": "The ERC-20 token to send. Omit it for the chain's own coin."}, "decimals": {"type": "integer", "description": "The token's decimals, as wallet_get reports them. Required with token; 18, or absent, for the coin."}, "amount": {"type": "string", "description": "How much to send, in whole units, as a decimal string such as 1.25. Never a JSON number."}, "dry_run": {"type": "boolean", "description": "Simulate and answer what it measured, without signing or broadcasting anything."}, "priority": {"type": "string", "enum": ["normal", "fast", "very_fast"], "description": "The fee tier; fast by default."}, "action_id": {"type": "string", "description": "A client-chosen id. Repeating a call with the same id and arguments replays the first answer instead of sending twice."}}, "required": ["wallet", "chain", "to", "amount"], "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some("wallet.write.send"),
   call: send,
};

pub const SEND_NFT: ToolSpec = ToolSpec {
   name: "tx_send_nft",
   title: "Send an NFT",
   description: "Send one NFT from a granted wallet: a collection, one id, and how many units for an \
      ERC-1155. The collection and the id have to be in that wallet's NFT allowlist, and the recipient \
      must not be a contract. With dry_run it answers the simulation and sends nothing.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string", "description": "The wallet the NFT belongs to."}, "chain": {"type": "integer", "description": "The chain the collection is on."}, "to": {"type": "string", "description": "The recipient, full and 0x-prefixed. A contract address is refused."}, "collection": {"type": "string", "description": "The collection contract."}, "token_id": {"type": "string", "description": "The token's id, as a whole number written as a string."}, "standard": {"type": "string", "enum": ["erc721", "erc1155"], "description": "Which standard the collection answers, as portfolio_get reports it."}, "amount": {"type": "string", "description": "How many units of an ERC-1155 id to send; 1 by default, and 1 for an ERC-721."}, "dry_run": {"type": "boolean", "description": "Simulate and answer what it measured, without signing or broadcasting anything."}, "priority": {"type": "string", "enum": ["normal", "fast", "very_fast"], "description": "The fee tier; fast by default."}, "action_id": {"type": "string", "description": "A client-chosen id. Repeating a call with the same id and arguments replays the first answer instead of sending twice."}}, "required": ["wallet", "chain", "to", "collection", "token_id", "standard"], "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some("wallet.write.nft_send"),
   call: send_nft,
};

fn history(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wallet = wallet_argument(&arguments)?;
      let chain = chain_argument(&arguments)?;
      let page = integer_argument(&arguments, "page", 0)? as usize;
      let per_page = integer_argument(&arguments, "per_page", 25)? as usize;

      let value = call
         .ctx
         .read(|ctx| read::history(ctx, &call.profile, wallet, chain, page, per_page))
         .map_err(read_error)?;

      Ok(ToolOutput::new(value))
   })
}

fn get(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wallet = wallet_argument(&arguments)?;

      let hash: TxHash = string_argument(&arguments, "hash")?
         .parse()
         .map_err(|_| ToolError::validation("hash is not a transaction hash"))?;

      let value = call
         .ctx
         .read(|ctx| read::transaction(ctx, &call.profile, wallet, hash))
         .map_err(read_error)?;

      Ok(ToolOutput::new(value))
   })
}

fn send(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let request = send_request(&arguments)?;
      let wanted = ExecCall::Send(request);
      let capability = wanted.capability();

      if dry_run_argument(&arguments)? {
         let value = exec::preview(call.ctx.clone(), &call.profile, &wanted)
            .await
            .map_err(|error| exec_error(error, capability))?;

         return Ok(ToolOutput::new(value));
      }

      let held = call.clone();

      action::run(call, &arguments, async move {
         Ok(ToolOutput::new(
            perform(&held, "tx_send", capability, wanted).await?,
         ))
      })
      .await
   })
}

fn send_nft(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let request = send_nft_request(&arguments)?;
      let wanted = ExecCall::Nft(request);
      let capability = wanted.capability();

      if dry_run_argument(&arguments)? {
         let value = exec::preview(call.ctx.clone(), &call.profile, &wanted)
            .await
            .map_err(|error| exec_error(error, capability))?;

         return Ok(ToolOutput::new(value));
      }

      let held = call.clone();

      action::run(call, &arguments, async move {
         Ok(ToolOutput::new(
            perform(&held, "tx_send_nft", capability, wanted).await?,
         ))
      })
      .await
   })
}

/// Run one action through the pipeline and answer its data.
///
/// Shared by the two send tools and the revoke one: the only things that differ are the tool's name in the
/// record and the call the port receives.
pub(super) async fn perform(
   call: &Call,
   tool: &str,
   capability: &str,
   wanted: ExecCall,
) -> Result<Value, ToolError> {
   let client = call.client_info();
   let origin = exec::Origin {
      tool,
      client_info: client.as_deref(),
   };
   let flows: &dyn TxFlows = call.ops.as_ref();

   exec::execute(
      call.ctx.clone(),
      &call.profile,
      wanted,
      &origin,
      flows,
   )
   .await
   .map_err(|error| exec_error(error, capability))
}

/// The request a `tx_send` describes.
fn send_request(arguments: &Value) -> Result<SendRequest, ToolError> {
   let chain = required_chain(arguments)?;
   let token = optional_address(arguments, "token")?;
   let decimals = decimals_argument(arguments, token.is_some())?;

   Ok(SendRequest {
      chain,
      from: wallet_argument(arguments)?,
      to: required_address(arguments, "to")?,
      token,
      amount: wei_amount(string_argument(arguments, "amount")?, decimals)?,
      decimals,
      priority: priority_argument(arguments)?,
   })
}

/// The decimals an amount is scaled by.
///
/// A token's come from the contract and the caller states them, because the amount's scale is the one thing
/// that cannot be guessed: one digit wrong is three orders of magnitude. The coin's are always 18.
pub(super) fn decimals_argument(arguments: &Value, token: bool) -> Result<u8, ToolError> {
   let stated =
      match arguments.get("decimals") {
         None | Some(Value::Null) => None,
         Some(value) => Some(
            value.as_u64().filter(|decimals| *decimals <= u8::MAX as u64).ok_or_else(|| {
               ToolError::validation("decimals is a whole number between 0 and 255")
            })? as u8,
         ),
      };

   match (token, stated) {
      (false, None) => Ok(18),
      (false, Some(18)) => Ok(18),
      (false, Some(_)) => Err(ToolError::validation(
         "the chain's own coin has 18 decimals, so decimals is 18 or absent",
      )),
      (true, Some(decimals)) => Ok(decimals),
      (true, None) => Err(ToolError::validation(
         "decimals is required with token: it is the scale the amount is read in",
      )),
   }
}

/// An amount in whole units, as the smallest unit the chain moves.
pub(super) fn wei_amount(text: &str, decimals: u8) -> Result<U256, ToolError> {
   if text.starts_with('-') || text.starts_with('+') {
      return Err(ToolError::validation(
         "amount is a plain decimal number: no sign and no exponent",
      ));
   }

   let units = parse_units(text, decimals)
      .map_err(|error| ToolError::validation(format!("amount is not a decimal number: {error}")))?;

   let wei = units.get_absolute();

   if wei.is_zero() {
      return Err(ToolError::validation(
         "amount is zero: there is nothing to send",
      ));
   }

   Ok(wei)
}

/// The request a `tx_send_nft` describes.
fn send_nft_request(arguments: &Value) -> Result<NftSendRequest, ToolError> {
   let standard = match string_argument(arguments, "standard")? {
      "erc721" => NftStandard::Erc721,
      "erc1155" => NftStandard::Erc1155,
      other => {
         return Err(ToolError::validation(format!(
            "standard is erc721 or erc1155; not {other}"
         )));
      }
   };

   let amount = match arguments.get("amount") {
      None | Some(Value::Null) => U256::from(1),
      Some(_) => u256_argument(arguments, "amount")?,
   };

   if amount.is_zero() {
      return Err(ToolError::validation(
         "amount is zero: there is nothing to send",
      ));
   }

   Ok(NftSendRequest {
      chain: required_chain(arguments)?,
      from: wallet_argument(arguments)?,
      to: required_address(arguments, "to")?,
      collection: required_address(arguments, "collection")?,
      token_id: u256_argument(arguments, "token_id")?,
      standard,
      amount,
      priority: priority_argument(arguments)?,
   })
}
