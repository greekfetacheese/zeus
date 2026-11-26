//! The three tools that describe the server itself: what it is, what exists, and what it has done.
//!
//! They are the only tools with no capability, and the reason is the same for all three: an agent that
//! cannot find out what it is allowed to do will guess, and a guess in a wallet is expensive. The status
//! answers "what am I, and what may I do with it"; the help answers "what exists, and what does a refusal
//! mean"; the activity answers "what have you been doing with that".
//!
//! None of them touches a wallet or a chain, and the activity reads the same ring the Agents page shows —
//! never another credential's rows, because a profile's own work is the only work it is shown.

use super::{Call, TOOLS, ToolFuture, ToolOutput, catalog, granted, integer_argument};
use crate::error::{ErrorCode, ToolError};
use crate::protocol::types::Annotations;
use crate::protocol::{GRANT_LOCATION, LEGACY_VERSIONS, MODERN_VERSION, SERVER_NAME};
use crate::tools::ToolSpec;
use serde_json::{Value, json};
use zeus_core::agent::{AgentProfile, CapabilityScope, ProfileMode, read};

/// A tool that takes nothing.
const EMPTY: &str = r#"{"type": "object", "additionalProperties": false}"#;

/// What this server is and what the caller may do with it.
pub const STATUS: ToolSpec = ToolSpec {
   name: "zeus_status",
   title: "Zeus status",
   description: "What this server is, which profile is calling, what its grant allows, and whether \
      the wallet is unlocked. Read it first: it is the only reliable answer to 'what may I do here'.",
   schema: EMPTY,
   annotations: Annotations::READ,
   capability: None,
   call: status,
};

/// The capability catalog, the tool list, and what each refusal means.
pub const HELP: ToolSpec = ToolSpec {
   name: "zeus_help",
   title: "Zeus help",
   description: "Every capability Zeus can grant with whether this profile holds it, every tool with \
      the capability it needs, and the error codes a refusal can carry. Use it to explain a refusal \
      without guessing.",
   schema: EMPTY,
   annotations: Annotations::READ,
   capability: None,
   call: help,
};

/// What this credential did recently, read back from the ring the Agents page shows.
pub const ACTIVITY: ToolSpec = ToolSpec {
   name: "agent_activity",
   title: "Your recent activity",
   description: "What this profile has done lately, newest first: the tool, the wallet, the decision, \
      the refusal's code and the transaction it produced. It is the trace an autonomous action leaves, \
      and the answer to 'what have you done in the last hour'.",
   schema: r#"{"type": "object", "properties": {"limit": {"type": "integer", "description": "How many entries to answer, 1 to 100; 20 by default."}, "since": {"type": "integer", "description": "Only entries at or after this instant, in unix seconds."}}, "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: None,
   call: activity,
};

fn status(call: &Call, _arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let app = call.ops.app_info();
      let unlocked = call.ctx.read(|ctx| ctx.vault_unlocked && !ctx.locked);
      let chain = call.ctx.chain().id();
      let profile = &call.profile;

      Ok(ToolOutput::new(json!({
         "server": {
            "name": SERVER_NAME,
            "version": app.version,
            "port": call.port,
            "started_at": call.started_at,
            "era": call.era.label(),
            "protocol_versions": {
               "modern": [MODERN_VERSION],
               "legacy": LEGACY_VERSIONS,
            },
         },
         "profile": {
            "id": profile.id.as_str(),
            "label": &profile.label,
            "mode": profile.mode,
            "enabled": profile.enabled,
            "expires_at": profile.expires_at,
         },
         "grant": grant_summary(profile),
         "vault": {"unlocked": unlocked},
         "chain": {"selected": chain},
      })))
   })
}

fn activity(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let limit = integer_argument(&arguments, "limit", 20)?;

      if !(1..=100).contains(&limit) {
         return Err(ToolError::validation(
            "limit is between 1 and 100 entries",
         ));
      }

      let since = since_argument(&arguments)?;
      let value = call.ctx.read(|ctx| read::activity(ctx, &call.profile, limit as usize, since));

      Ok(ToolOutput::new(value))
   })
}

fn help(call: &Call, _arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let profile = &call.profile;

      let tools: Vec<Value> = TOOLS
         .iter()
         .map(|spec| {
            json!({
               "name": spec.name,
               "capability": spec.capability,
               "granted": spec.capability.is_none_or(|name| granted(profile, name)),
               "read_only": spec.annotations.read_only,
            })
         })
         .collect();

      let errors: Vec<Value> = ErrorCode::ALL
         .iter()
         .map(|code| {
            json!({
               "code": code.as_str(),
               "meaning": code.meaning(),
               "hint": code.hint(),
            })
         })
         .collect();

      Ok(ToolOutput::new(json!({
         "catalog": catalog(profile),
         "tools": tools,
         "errors": errors,
         "switch": GRANT_LOCATION,
      })))
   })
}

/// The optional start of the window, in unix seconds.
fn since_argument(arguments: &Value) -> Result<Option<u64>, ToolError> {
   match arguments.get("since") {
      None | Some(Value::Null) => Ok(None),
      Some(value) => value
         .as_u64()
         .map(Some)
         .ok_or_else(|| ToolError::validation("since is unix seconds, as a whole number")),
   }
}

/// The live grant, as a model needs to read it: which wallets, and which capabilities where.
fn grant_summary(profile: &AgentProfile) -> Value {
   let wallets: Vec<Value> = profile
      .wallets
      .iter()
      .filter(|(_, grant)| grant.enabled)
      .map(|(address, grant)| {
         json!({
            "address": format!("{address:#x}"),
            "capabilities": grant.caps.iter().collect::<Vec<_>>(),
         })
      })
      .collect();

   let capabilities: Vec<&str> = zeus_core::agent::CAPABILITIES
      .iter()
      .filter(|entry| entry.scope == CapabilityScope::Global && profile.allows_global(entry.name))
      .map(|entry| entry.name)
      .collect();

   json!({
      "wallets": wallets,
      "capabilities": capabilities,
      "mode": match profile.mode {
         ProfileMode::Authorized => "authorized",
         ProfileMode::ConfirmEach => "confirm_each",
      },
      "max_slippage_bps": profile.max_slippage_bps,
      "export_dir": profile.export_dir.as_ref().map(|dir| dir.display().to_string()),
   })
}
