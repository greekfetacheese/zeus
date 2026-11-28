//! The approvals one wallet has granted, and the one approval an agent can clear.
//!
//! A row that names an unlimited allowance is the one a user most needs to hear about, so the read answers
//! it plainly. The revoke is a transaction under the hood — the same simulation, the same confirmation, the
//! same record — and it moves no value, so it carries no allowance to check.
//!
//! A Permit2 row is deliberately not revocable here: clearing one signs a message with the wallet's key
//! rather than sending a call, and no grant covers signing. The tool says so instead of opening a window the
//! grant does not know about.

use super::tx::perform;
use super::{
   Call, ToolFuture, ToolOutput, ToolSpec, action, chain_argument, dry_run_argument, exec_error,
   priority_argument, read_error, required_address, required_chain, string_argument, u256_argument,
   wallet_argument,
};
use crate::error::{ErrorCode, ToolError};
use crate::protocol::types::Annotations;
use serde_json::Value;
use zeus_core::agent::exec::{self, ExecCall, RevokeRequest, RevokeTarget};
use zeus_core::agent::read;
use zeus_eth::nft::NftStandard;

pub const LIST: ToolSpec = ToolSpec {
   name: "approvals_list",
   title: "Approvals",
   description: "The live ERC-20, Permit2 and NFT approvals one granted wallet has granted, with \
      the allowance and, where a price exists, its USD exposure. An unlimited row is one an operator \
      may spend in full.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string",
      "description": "The wallet's address, as wallet_list shows it."}, "chain": {"type": "integer",
      "description": "Only this chain; every chain otherwise."}}, "required": ["wallet"],
      "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("wallet.read.approvals"),
   call: list,
};

pub const REVOKE: ToolSpec = ToolSpec {
   name: "approvals_revoke",
   title: "Revoke an approval",
   description: "Clear one approval a granted wallet has given: an ERC-20 allowance, a \
      collection-wide operator, or one id of a collection. It is a transaction under the hood, with the \
      same simulation and confirmation as a send, and it moves no value. With dry_run it answers the \
      simulation and sends nothing.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string", "description": "The wallet that granted the approval."}, "chain": {"type": "integer", "description": "The chain the approval is on."}, "kind": {"type": "string", "enum": ["token", "nft"], "description": "Which shape to clear: an ERC-20 allowance, or an NFT operator or id."}, "token": {"type": "string", "description": "The ERC-20 token, when kind is token."}, "spender": {"type": "string", "description": "The address allowed to spend, when kind is token."}, "collection": {"type": "string", "description": "The collection contract, when kind is nft."}, "operator": {"type": "string", "description": "The approved operator, when kind is nft."}, "token_id": {"type": "string", "description": "One id to clear, when kind is nft. Omit it to clear the collection-wide approval instead."}, "standard": {"type": "string", "enum": ["erc721", "erc1155"], "description": "How the collection models one id. Required when kind is nft and token_id is given."}, "dry_run": {"type": "boolean", "description": "Simulate and answer what it measured, without signing or broadcasting anything."}, "priority": {"type": "string", "enum": ["normal", "fast", "very_fast"], "description": "The fee tier; fast by default."}, "action_id": {"type": "string", "description": "A client-chosen id. Repeating a call with the same id and arguments replays the first answer instead of sending twice."}}, "required": ["wallet", "chain", "kind"], "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some("wallet.write.approvals_revoke"),
   call: revoke,
};

fn list(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wallet = wallet_argument(&arguments)?;
      let chain = chain_argument(&arguments)?;

      let value = call
         .ctx
         .read(|ctx| read::approvals(ctx, &call.profile, wallet, chain))
         .map_err(read_error)?;

      Ok(ToolOutput::new(value))
   })
}

fn revoke(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let request = revoke_request(&arguments)?;
      let wanted = ExecCall::Revoke(request);
      let capability = wanted.capability();

      if dry_run_argument(&arguments)? {
         let value = exec::preview(call.ctx.clone(), &call.profile, &wanted)
            .await
            .map_err(|error| exec_error(error, capability))?;

         return Ok(action::preview(value, &arguments));
      }

      let held = call.clone();

      action::run(call, &arguments, async move {
         Ok(perform(&held, "approvals_revoke", capability, wanted).await?)
      })
      .await
   })
}

/// The request an `approvals_revoke` describes.
fn revoke_request(arguments: &Value) -> Result<RevokeRequest, ToolError> {
   let target = match string_argument(arguments, "kind")? {
      "token" => RevokeTarget::Token {
         token: required_address(arguments, "token")?,
         spender: required_address(arguments, "spender")?,
      },
      "nft" => RevokeTarget::Nft {
         collection: required_address(arguments, "collection")?,
         operator: required_address(arguments, "operator")?,
         token_id: match arguments.get("token_id") {
            None | Some(Value::Null) => None,
            Some(_) => Some(u256_argument(arguments, "token_id")?),
         },
         standard: match arguments.get("standard") {
            None | Some(Value::Null) => None,
            Some(_) => Some(match string_argument(arguments, "standard")? {
               "erc721" => NftStandard::Erc721,
               "erc1155" => NftStandard::Erc1155,
               other => {
                  return Err(ToolError::validation(format!(
                     "standard is erc721 or erc1155; not {other}"
                  )));
               }
            }),
         },
      },
      "permit" => {
         return Err(ToolError::new(
            ErrorCode::Unsupported,
            "a Permit2 allowance cannot be revoked by an agent: clearing one signs a message with the \
             wallet's key, and no grant covers signing",
         )
         .with_hint("Ask the app owner to clear the Permit2 row in Zeus's approvals page."));
      }
      other => {
         return Err(ToolError::validation(format!(
            "kind is token or nft; not {other}"
         )));
      }
   };

   Ok(RevokeRequest {
      chain: required_chain(arguments)?,
      owner: wallet_argument(arguments)?,
      target,
      priority: priority_argument(arguments)?,
   })
}
