//! The transactions a wallet recorded, and one of them by hash.
//!
//! History is the app's own record of what it sent, not a chain explorer: a transaction Zeus did not
//! broadcast is not here. That is why an answer names it "recorded" and why the hash is the identity —
//! the receipt status is the one the app stored when it watched the transaction land.

use super::{
   Call, ToolFuture, ToolOutput, ToolSpec, chain_argument, integer_argument, read_error,
   string_argument, wallet_argument,
};
use crate::error::ToolError;
use crate::protocol::types::Annotations;
use serde_json::Value;
use zeus_core::agent::read;
use zeus_eth::alloy_primitives::TxHash;

pub const HISTORY: ToolSpec = ToolSpec {
   name: "tx_history",
   title: "Transaction history",
   description: "The transactions one granted wallet recorded, newest first, paged. Amounts are in \
      the token that moved, with its raw integer beside the readable form.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string", \
      "description": "The wallet's address, as wallet_list shows it."}, "chain": {"type": "integer", \
      "description": "Only this chain; every chain otherwise."}, "page": {"type": "integer", \
      "description": "Zero-based page number; 0 by default."}, "per_page": {"type": "integer", \
      "description": "Rows per page, 1 to 100; 25 by default."}}, "required": ["wallet"], \
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
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string", \
      "description": "The wallet that recorded the transaction."}, "hash": {"type": "string", \
      "description": "The transaction hash, full and 0x-prefixed."}}, "required": ["wallet", "hash"], \
      "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("wallet.read.history"),
   call: get,
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
