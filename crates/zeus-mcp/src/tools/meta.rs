//! The two tools that describe the server itself.
//!
//! They are the only tools with no capability, and the reason is the same for both: an agent that cannot
//! find out what it is allowed to do will guess, and a guess in a wallet is expensive. The status answers
//! "what am I, and what may I do with it"; the help answers "what exists, and what does a refusal mean".
//! Neither touches a wallet, a chain or a file.

use super::{Call, TOOLS, ToolFuture, ToolOutput, catalog, granted};
use crate::error::ErrorCode;
use crate::protocol::types::Annotations;
use crate::protocol::{GRANT_LOCATION, LEGACY_VERSIONS, MODERN_VERSION, SERVER_NAME};
use crate::tools::ToolSpec;
use serde_json::{Value, json};
use zeus_core::agent::{AgentProfile, CapabilityScope, ProfileMode};

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
