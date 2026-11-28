//! The shapes the wire carries: tool and resource descriptors, and every result this server can send.
//!
//! Two eras, one set of answers. The fields the stateless revision added — `resultType`, `ttlMs` and
//! `cacheScope` — are written only for a modern client, so a handshake-era client sees exactly what a
//! 2025 server would have sent it and nothing it has to ignore.

use crate::error::ToolError;
use crate::ops::AgentOps;
use crate::protocol::{
   INSTRUCTIONS, LEGACY_VERSION, LEGACY_VERSIONS, LIST_TTL_MS, MODERN_VERSION, SERVER_NAME, meta,
};
use serde_json::{Value, json};

/// The annotations a client may use to decide when to ask its own user about a call.
///
/// They are hints and never a control: the protocol says a client must treat them as untrusted, and the
/// grant is enforced on this side. They are emitted because a harness that refuses a destructive tool
/// without asking its user costs nothing and asks again — a second gate for free.
#[derive(Clone, Copy, Debug, Default)]
pub struct Annotations {
   pub read_only: bool,
   pub destructive: bool,
   pub open_world: bool,
}

impl Annotations {
   /// A tool that changes nothing.
   pub const READ: Self = Self {
      read_only: true,
      destructive: false,
      open_world: false,
   };

   /// A tool that changes app state without moving funds or reaching a chain.
   pub const WRITE: Self = Self {
      read_only: false,
      destructive: false,
      open_world: false,
   };

   /// A tool that changes something and is hard to undo.
   pub const DESTRUCTIVE: Self = Self {
      read_only: false,
      destructive: true,
      open_world: true,
   };

   fn to_value(self) -> Value {
      json!({
         "readOnlyHint": self.read_only,
         "destructiveHint": self.destructive,
         "openWorldHint": self.open_world,
      })
   }
}

/// One tool as `tools/list` shows it.
pub fn tool(
   name: &str,
   title: &str,
   description: &str,
   schema: &str,
   annotations: Annotations,
) -> Value {
   // A descriptor whose schema does not parse would ship an object with no fields at all, which a model
   // reads as "this tool takes nothing". Say so rather than pretend; the schema tests fail the build on
   // one, so this is the runtime half of that rule.
   let schema: Value = serde_json::from_str(schema).unwrap_or_else(|error| {
      tracing::warn!("Tool {name} has an unparseable input schema: {error}");
      empty_schema()
   });

   json!({
      "name": name,
      "title": title,
      "description": description,
      "inputSchema": schema,
      "annotations": annotations.to_value(),
   })
}

/// One resource as `resources/list` shows it.
pub fn resource(uri: &str, name: &str, description: &str) -> Value {
   json!({
      "uri": uri,
      "name": name,
      "description": description,
      "mimeType": "application/json",
   })
}

/// A `tools/list` result.
pub fn tool_list(tools: Vec<Value>, modern: bool) -> Value {
   cacheable(json!({"tools": tools}), modern)
}

/// One resource template as `resources/templates/list` shows it.
pub fn resource_template(uri_template: &str, name: &str, description: &str) -> Value {
   json!({
      "uriTemplate": uri_template,
      "name": name,
      "description": description,
      "mimeType": "application/json",
   })
}

/// A `resources/list` result.
pub fn resource_list(resources: Vec<Value>, modern: bool) -> Value {
   cacheable(json!({"resources": resources}), modern)
}

/// A `resources/templates/list` result.
pub fn resource_template_list(templates: Vec<Value>, modern: bool) -> Value {
   cacheable(json!({"resourceTemplates": templates}), modern)
}

/// A `resources/read` result: one JSON document, addressed by its own URI.
pub fn resource_contents(uri: &str, document: &Value, modern: bool) -> Value {
   let text = serde_json::to_string(document).unwrap_or_default();

   cacheable(
      json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": text}]}),
      modern,
   )
}

/// A `tools/call` result that ran.
pub fn call_result(envelope: Value, modern: bool) -> Value {
   // The protocol asks for the structured answer and, for compatibility, the same JSON in a text block:
   // a client that cannot read `structuredContent` still gets the whole document.
   let text = serde_json::to_string(&envelope).unwrap_or_default();

   let mut result = json!({
      "content": [{"type": "text", "text": text}],
      "structuredContent": envelope,
   });

   if modern {
      result["resultType"] = json!("complete");
   }

   result
}

/// A `tools/call` result that refused: an execution error, which a model can read and act on.
pub fn call_error(error: &ToolError, modern: bool) -> Value {
   let envelope = json!({"ok": false, "error": error.to_value()});
   let text = serde_json::to_string(&envelope).unwrap_or_default();

   let mut result = json!({
      "content": [{"type": "text", "text": text}],
      "structuredContent": envelope,
      "isError": true,
   });

   if modern {
      result["resultType"] = json!("complete");
   }

   result
}

/// The handshake's answer: the revision this client may assume, and what it can call.
///
/// A client that names a revision this build speaks is answered with its own. Every handshake revision is
/// a superset of the one before it, so answering a newer number would tell a client to assume fields it
/// never asked about; a revision this build does not know falls back to the newest one it does.
pub fn initialize(ops: &dyn AgentOps, requested: Option<&str>) -> Value {
   let version = requested
      .filter(|version| LEGACY_VERSIONS.iter().any(|known| known.eq_ignore_ascii_case(version)))
      .unwrap_or(LEGACY_VERSION);

   json!({
      "protocolVersion": version,
      "capabilities": capabilities(),
      "serverInfo": server_info(ops),
   })
}

/// The stateless revision's discovery answer, which this server has to implement.
pub fn discover(ops: &dyn AgentOps) -> Value {
   json!({
      "supportedVersions": [MODERN_VERSION],
      "capabilities": capabilities(),
      "instructions": INSTRUCTIONS,
      "ttlMs": LIST_TTL_MS,
      "cacheScope": "private",
      "_meta": {meta::SERVER_INFO: server_info(ops)},
      "resultType": "complete",
   })
}

/// What this server offers: tools and resources, and nothing deprecated.
pub fn capabilities() -> Value {
   json!({
      "tools": {"listChanged": false},
      "resources": {"subscribe": false, "listChanged": false},
   })
}

/// The server's own name and version.
pub fn server_info(ops: &dyn AgentOps) -> Value {
   json!({"name": SERVER_NAME, "version": ops.app_info().version})
}

/// The schema of a tool that takes nothing.
pub fn empty_schema() -> Value {
   json!({"type": "object", "additionalProperties": false})
}

/// Add the fields the stateless revision requires of a list result.
fn cacheable(mut result: Value, modern: bool) -> Value {
   if modern {
      result["resultType"] = json!("complete");
      result["ttlMs"] = json!(LIST_TTL_MS);
      result["cacheScope"] = json!("private");
   }

   result
}
