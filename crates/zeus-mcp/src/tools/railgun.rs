//! The private side's status for one wallet.
//!
//! The numbers come from the status the app keeps for the services screen, so an agent reads what the
//! user sees. Note counts and merge advice arrive with the Railgun tools that act on them.

use super::{Call, ToolFuture, ToolOutput, ToolSpec, read_error, wallet_argument};
use crate::protocol::types::Annotations;
use serde_json::Value;
use zeus_core::agent::read;

pub const STATUS: ToolSpec = ToolSpec {
   name: "railgun_status",
   title: "Private side",
   description: "Whether one wallet's private side is enabled, synced and idle on each Railgun \
      chain, and the zk address it is registered with. A null zk address means the wallet cannot \
      derive one.",
   schema: r#"{"type": "object", "properties": {"wallet": {"type": "string", \
      "description": "The wallet's address, as wallet_list shows it."}}, "required": ["wallet"], \
      "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("wallet.read.private"),
   call: status,
};

fn status(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let wallet = wallet_argument(&arguments)?;

      let value = call
         .ctx
         .read(|ctx| read::railgun(ctx, &call.profile, wallet))
         .map_err(read_error)?;

      Ok(ToolOutput::new(value))
   })
}
