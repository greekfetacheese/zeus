//! The tracked Uniswap pools.
//!
//! A pool row is what a quote would run against: the pair, the fee and the pool's identity. It is a read
//! even though adding one is a grantable write, because seeing a pool changes nothing.

use super::{Call, ToolFuture, ToolOutput, ToolSpec, chain_argument};
use crate::protocol::types::Annotations;
use serde_json::Value;
use zeus_core::agent::read;

pub const LIST: ToolSpec = ToolSpec {
   name: "pools_list",
   title: "Pools",
   description: "The Uniswap pools Zeus tracks, with their pair, fee, DEX and identity, on one chain \
      or on every chain that has any.",
   schema: r#"{"type": "object", "properties": {"chain": {"type": "integer", \
      "description": "Only this chain; every chain otherwise."}}, "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("settings.pools.read"),
   call: list,
};

fn list(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let chain = chain_argument(&arguments)?;
      let pools = call.ctx.read(|ctx| read::pools(ctx, chain));
      Ok(ToolOutput::new(pools))
   })
}
