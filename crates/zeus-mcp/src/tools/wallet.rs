//! The wallets a grant gives this profile, and what may be done with one.
//!
//! A wallet the grant does not name does not exist as far as an agent can tell: it is not listed, it is
//! not addressable, and no tool will mention it. That is the point of granting per wallet rather than per
//! profile — a profile that may read one address cannot enumerate the others.

use super::{Call, ToolFuture, ToolOutput, ToolSpec, chain_argument, read_error, wallet_argument};
use crate::error::{ErrorCode, ToolError};
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use zeus_core::agent::read;

pub const LIST: ToolSpec = ToolSpec {
   name: "wallet_list",
   title: "Wallets",
   description: "Every wallet this profile was granted, with its address, its name and the zk \
      address of its private side. A wallet that was not granted does not appear at all.",
   schema: r#"{"type": "object", "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("settings.wallets.read"),
   call: list,
};

pub const GET: ToolSpec = ToolSpec {
   name: "wallet_get",
   title: "One wallet",
   description: "One granted wallet, plus the capabilities this profile holds on it — so a caller \
      can tell what it may do with this address without trying.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string", \
      "description": "The wallet's address, as wallet_list shows it."}}, "required": ["wallet"], \
      "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("settings.wallets.read"),
   call: get,
};

pub const DELEGATE: ToolSpec = ToolSpec {
   name: "delegate_status",
   title: "Delegation",
   description: "Whether one wallet is EIP-7702 delegated on a chain, and to which contract. The \
      answer is read from the chain when the cached one is stale; a chain with no node answers an \
      error rather than a guess.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string", \
      "description": "The wallet's address, as wallet_list shows it."}, "chain": {"type": "integer", \
      "description": "The chain to ask about; the chain selected in Zeus otherwise."}}, \
      "required": ["wallet"], "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("settings.wallets.read"),
   call: delegate,
};

fn list(call: &Call, _arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move { Ok(ToolOutput::new(json!({"wallets": rows(call)}))) })
}

fn get(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wallet = wallet_argument(&arguments)?;

      let value = call
         .ctx
         .read(|ctx| read::wallet(ctx, &call.profile, wallet))
         .map_err(read_error)?;

      Ok(ToolOutput::new(value))
   })
}

fn delegate(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wallet = wallet_argument(&arguments)?;

      // An address the grant does not name is refused before a node is asked about it.
      if !read::wallet_granted(&call.profile, wallet) {
         return Err(read_error(read::unknown_wallet(wallet)));
      }

      let chain = chain_argument(&arguments)?.unwrap_or_else(|| call.ctx.chain().id());

      // The cached answer is what the account panel shows; it is re-read when stale, and a node that
      // cannot answer is reported rather than turned into "not delegated".
      if call.ctx.should_check_delegated_wallet_status(chain, wallet) {
         call.ctx.check_delegated_wallet_status(chain, wallet).await.map_err(|error| {
            ToolError::new(
               ErrorCode::Network,
               format!("could not read the account on chain {chain}: {error}"),
            )
         })?;
      }

      let delegated = call.ctx.read(|ctx| ctx.delegated_wallets.get(chain, wallet));

      let value = call
         .ctx
         .read(|ctx| read::delegate(ctx, &call.profile, wallet, chain, delegated))
         .map_err(read_error)?;

      Ok(ToolOutput::new(value))
   })
}

/// The granted wallets, as both this tool and the zeus://wallets resource answer.
pub fn rows(call: &Call) -> Vec<Value> {
   call.ctx.read(|ctx| read::wallets(ctx, &call.profile))
}
