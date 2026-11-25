//! The approvals one wallet has granted, so a caller can see what is exposed before revoking.
//!
//! Nothing here revokes: this slice is the read surface, and the revoke tool arrives with the money
//! tools. A row that names an unlimited allowance is the one a user most needs to hear about.

use super::{Call, ToolFuture, ToolOutput, ToolSpec, chain_argument, read_error, wallet_argument};
use crate::protocol::types::Annotations;
use serde_json::Value;
use zeus_core::agent::read;

pub const LIST: ToolSpec = ToolSpec {
   name: "approvals_list",
   title: "Approvals",
   description: "The live ERC-20, Permit2 and NFT approvals one granted wallet has granted, with \
      the allowance and, where a price exists, its USD exposure. An unlimited row is one an operator \
      may spend in full.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string", \
      "description": "The wallet's address, as wallet_list shows it."}, "chain": {"type": "integer", \
      "description": "Only this chain; every chain otherwise."}}, "required": ["wallet"], \
      "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("wallet.read.approvals"),
   call: list,
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
