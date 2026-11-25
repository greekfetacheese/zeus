//! The chains, their endpoints and their health.
//!
//! An endpoint is what an agent's reads run through, so this is the vocabulary a caller needs to explain
//! a failure: a disabled chain is why a call said unsupported, an unavailable node is why it said network.

use super::{Call, ToolFuture, ToolOutput, ToolSpec};
use crate::protocol::types::Annotations;
use serde_json::Value;
use zeus_core::agent::read;

pub const LIST: ToolSpec = ToolSpec {
   name: "networks_list",
   title: "Networks",
   description: "Every supported chain with its name, whether it is disabled, whether a working \
      endpoint is available, and the endpoints themselves with their health. An endpoint URL is \
      infrastructure the user configured; it is not a wallet secret.",
   schema: r#"{"type": "object", "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("settings.networks.read"),
   call: list,
};

fn list(call: &Call, _arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let networks = call.ctx.read(read::networks);
      Ok(ToolOutput::new(networks))
   })
}
