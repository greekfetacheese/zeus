//! The tool surface: one table, one gate in front of every call, and one answer shape.
//!
//! A tool descriptor is data — a name, a schema, a hint set, the capability it needs — and the handler
//! beside it. The table is the answer to `tools/list`, filtered to what the caller's grant allows, and
//! the same table is the dispatcher, so a tool cannot be listed without being gated: the gate reads the
//! descriptor's own capability.
//!
//! Every refusal that is not a protocol error is recorded. An allow is not: the ring answers what this
//! thing has been *doing*, and a granted read changes nothing. A write is recorded once the first one
//! lands, and a refusal always is, because "it tried to send outside its allowlist" is the line a user
//! most needs to see.

pub mod approvals;
pub mod contacts;
pub mod dapps;
pub mod meta;
pub mod networks;
pub mod pools;
pub mod portfolio;
pub mod railgun;
pub mod settings;
pub mod tx;
pub mod wallet;

use crate::error::{ErrorCode, ProtocolError, ToolError};
use crate::now_secs;
use crate::ops::AgentOps;
use crate::protocol::era::Era;
use crate::protocol::{GRANT_LOCATION, types};
use serde::Serialize;
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use zeus_core::ZeusCtx;
use zeus_core::agent::read::ReadError;
use zeus_core::agent::{
   AgentProfile, AuditEntry, CAPABILITIES, CapabilityScope, Decision, Denial, Intent, PrivacyBadge,
   capability, precheck,
};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::ChainId;

/// Everything one call is given.
#[derive(Clone)]
pub struct Call {
   pub ctx: ZeusCtx,
   /// The profile whose credential arrived. Read fresh on every request, never cached.
   pub profile: AgentProfile,
   pub ops: Arc<dyn AgentOps>,
   pub era: Era,
   pub port: u16,
   pub started_at: u64,
}

impl Call {
   /// What the harness called itself, for the audit entry.
   pub fn client_info(&self) -> Option<String> {
      self.era.client_info().map(str::to_owned)
   }
}

/// A note a successful answer carries beside its data.
#[derive(Clone, Debug, Serialize)]
pub struct Warning {
   /// The stable code, e.g. `USD_UNAVAILABLE`.
   pub code: &'static str,
   pub message: String,
   /// What the warning means for the data, so a model does not have to guess.
   pub effect: &'static str,
}

/// What a tool answers with.
#[derive(Clone, Debug, Default)]
pub struct ToolOutput {
   pub data: Value,
   pub warnings: Vec<Warning>,
}

impl ToolOutput {
   pub fn new(data: Value) -> Self {
      Self {
         data,
         warnings: Vec::new(),
      }
   }

   /// The envelope every answer carries, successful or refused.
   pub fn envelope(&self) -> Value {
      json!({
         "ok": true,
         "data": self.data,
         "warnings": self.warnings,
      })
   }
}

/// A handler's answer, boxed so the table holds one function per tool whether or not it is async.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send + 'a>>;

/// The handler a descriptor points at.
pub type Handler = for<'a> fn(&'a Call, Value) -> ToolFuture<'a>;

/// One tool: what it looks like, what it needs, and what it runs.
pub struct ToolSpec {
   pub name: &'static str,
   pub title: &'static str,
   pub description: &'static str,
   /// The input schema, as the JSON this server writes into the descriptor.
   pub schema: &'static str,
   pub annotations: types::Annotations,
   /// The capability a call needs. Nothing is a tool that only describes the caller's own credential.
   pub capability: Option<&'static str>,
   pub call: Handler,
}

impl ToolSpec {
   /// The descriptor `tools/list` shows.
   pub fn definition(&self) -> Value {
      types::tool(
         self.name,
         self.title,
         self.description,
         self.schema,
         self.annotations,
      )
   }
}

/// Every tool this build has, in the order the protocol asks for: sorted by name.
pub const TOOLS: &[ToolSpec] = &[
   approvals::LIST,
   contacts::LIST,
   dapps::LIST,
   wallet::DELEGATE,
   networks::LIST,
   pools::LIST,
   portfolio::GET,
   railgun::STATUS,
   settings::GET,
   settings::LIST,
   tx::GET,
   tx::HISTORY,
   wallet::GET,
   wallet::LIST,
   meta::HELP,
   meta::STATUS,
];

/// The tool with that name, if this build has one.
pub fn find(name: &str) -> Option<&'static ToolSpec> {
   TOOLS.iter().find(|spec| spec.name == name)
}

/// Whether the profile's grant covers what a tool needs.
///
/// A capability scoped to a wallet is held when *any* granted wallet has it: a tool that takes an
/// address decides for that address again when it runs, so visibility here is about the surface, not
/// about one wallet.
pub fn granted(profile: &AgentProfile, name: &str) -> bool {
   match capability(name).map(|entry| entry.scope) {
      Some(CapabilityScope::Global) => profile.allows_global(name),
      Some(CapabilityScope::Wallet) => {
         profile.wallets.values().any(|grant| grant.enabled && grant.caps.contains(name))
      }
      None => false,
   }
}

/// Whether the grant lets this profile see the tool at all.
///
/// The protocol permits the list to vary by the authorization presented on the request, which is what a
/// least-privilege surface wants: a profile granted only reads never learns the money tools exist.
pub fn visible(profile: &AgentProfile, spec: &ToolSpec) -> bool {
   spec.capability.is_none_or(|name| granted(profile, name))
}

/// A `tools/list` result, scoped to the caller's grant.
pub fn list(profile: &AgentProfile, modern: bool) -> Value {
   let tools = TOOLS
      .iter()
      .filter(|spec| visible(profile, spec))
      .map(ToolSpec::definition)
      .collect();

   types::tool_list(tools, modern)
}

/// Read a `tools/call` request and answer it.
///
/// Everything about the *request* — an unknown name, arguments that are not an object — is a protocol
/// error, because a model cannot fix it. Everything about the *action* is a result with `isError`, which
/// is the only shape a model can read and explain.
pub async fn call_request(
   call: &Call,
   params: Option<&Value>,
   modern: bool,
) -> Result<Value, ProtocolError> {
   let params = params.ok_or_else(|| ProtocolError::invalid_params("tools/call needs params"))?;

   let name = params
      .get("name")
      .and_then(Value::as_str)
      .ok_or_else(|| ProtocolError::invalid_params("tools/call needs a tool name"))?;

   let arguments = match params.get("arguments") {
      None | Some(Value::Null) => json!({}),
      Some(value @ Value::Object(_)) => value.clone(),
      Some(_) => {
         return Err(ProtocolError::invalid_params(
            "arguments must be an object",
         ));
      }
   };

   let spec = find(name).ok_or_else(|| ProtocolError::unknown_tool(name))?;

   Ok(match run(spec, call, arguments).await {
      Ok(output) => types::call_result(output.envelope(), modern),
      Err(error) => types::call_error(&error, modern),
   })
}

/// Gate one call and run it.
///
/// The intent is built from the capability's own scope, because the same string decides differently in
/// each: a wallet-scoped read is decided against the wallet the call names, and a global one against the
/// profile. Asking the catalog rather than the tool table keeps the two from drifting.
pub async fn run(spec: &ToolSpec, call: &Call, arguments: Value) -> Result<ToolOutput, ToolError> {
   if let Some(name) = spec.capability {
      let intent = if capability(name).is_some_and(|entry| entry.scope == CapabilityScope::Wallet) {
         Intent::WalletRead {
            capability: name,
            wallet: wallet_argument(&arguments)?,
         }
      } else {
         Intent::GlobalRead { capability: name }
      };

      guard(call, spec.name, &intent)?;
   }

   (spec.call)(call, arguments).await
}

/// The wallet address a call names.
pub fn wallet_argument(arguments: &Value) -> Result<Address, ToolError> {
   let text = arguments
      .get("wallet")
      .and_then(Value::as_str)
      .ok_or_else(|| ToolError::validation("wallet is required: the address wallet_list shows"))?;

   text
      .parse()
      .map_err(|_| ToolError::validation(format!("wallet is not an address: {text}")))
}

/// The chain a call names, if it names one.
pub fn chain_argument(arguments: &Value) -> Result<Option<u64>, ToolError> {
   let Some(value) = arguments.get("chain") else {
      return Ok(None);
   };

   if value.is_null() {
      return Ok(None);
   }

   let chain = value.as_u64().ok_or_else(|| ToolError::validation("chain must be a number"))?;

   if !ChainId::is_supported(chain) {
      return Err(ToolError::new(
         ErrorCode::Unsupported,
         format!("chain {chain} is not one Zeus supports"),
      ));
   }

   Ok(Some(chain))
}

/// A whole-number argument with a default.
pub fn integer_argument(arguments: &Value, key: &str, default: u64) -> Result<u64, ToolError> {
   match arguments.get(key) {
      None | Some(Value::Null) => Ok(default),
      Some(value) => value
         .as_u64()
         .ok_or_else(|| ToolError::validation(format!("{key} must be a whole number"))),
   }
}

/// A string argument a call cannot run without.
pub fn string_argument<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolError> {
   arguments
      .get(key)
      .and_then(Value::as_str)
      .ok_or_else(|| ToolError::validation(format!("{key} is required")))
}

/// The refusal a projection's error becomes, naming the switch the way the policy does.
pub fn read_error(error: ReadError) -> ToolError {
   match error {
      ReadError::NotGranted { capability } => ToolError::new(
         ErrorCode::NotGranted,
         format!("the grant does not include {capability}"),
      )
      .with_hint(format!(
         "Ask the app owner to grant {capability} in {GRANT_LOCATION}, then try again."
      )),
      ReadError::NotFound { what, id } => ToolError::new(
         ErrorCode::NotFound,
         format!("no {what} {id} is available to this profile"),
      ),
   }
}

/// Decide a request against the grant, and record a refusal.
///
/// This is the precheck the pipeline's own rule asks for: a guard that can stop an action runs before
/// anything is built, simulated or signed. It is not the authority — the confirmation seam decides again
/// from the simulation — but it is what makes a refusal cost nothing.
pub fn guard(call: &Call, subject: &str, intent: &Intent) -> Result<(), ToolError> {
   let started = Instant::now();
   let decision = precheck(&call.ctx, &call.profile, intent);

   if !matches!(decision, Decision::Allowed { .. }) {
      record(call, subject, intent, &decision, started);
   }

   match decision {
      Decision::Allowed { .. } => Ok(()),
      Decision::NeedsUser { reason } => Err(
         ToolError::new(ErrorCode::UserActionRequired, reason)
            .with_hint("Tell the user which window to finish in Zeus, then retry."),
      ),
      Decision::Denied(denial) => Err(denial_error(&denial, intent.capability())),
   }
}

/// The refusal a policy denial becomes.
fn denial_error(denial: &Denial, capability: &str) -> ToolError {
   let code = match denial {
      Denial::VaultLocked => ErrorCode::VaultLocked,
      Denial::NotGranted { .. } | Denial::WalletNotGranted { .. } => ErrorCode::NotGranted,
      Denial::TokenNotAllowed { .. } => ErrorCode::TokenNotAllowed,
      Denial::NftNotAllowed { .. } => ErrorCode::NftNotAllowed,
   };

   let error = ToolError::new(code, denial.to_string());

   match code {
      ErrorCode::NotGranted => error.with_hint(format!(
         "Ask the app owner to grant {capability} in {GRANT_LOCATION}, then try again."
      )),
      _ => error,
   }
}

/// Write one refusal to the audit ring.
///
/// The ring lives in memory. Sealing it to disk from here would let a caller retrying a refusal — the
/// one case the rate limit exists for — rewrite the whole log once per attempt.
fn record(call: &Call, subject: &str, intent: &Intent, decision: &Decision, started: Instant) {
   let mut entry = AuditEntry::new(
      &call.profile,
      subject,
      intent.capability(),
      now_secs(),
   )
   .decided(decision);

   entry.wallet = intent.wallet();
   entry.latency_ms = started.elapsed().as_millis() as u64;
   entry.client_info = call.client_info();

   call.ctx.agent_audit().record(entry);
}

/// The capability catalog, as the help tool and the permissions resource answer it.
pub fn catalog(profile: &AgentProfile) -> Vec<Value> {
   CAPABILITIES
      .iter()
      .map(|entry| {
         json!({
            "name": entry.name,
            "tier": entry.tier.level(),
            "scope": match entry.scope {
               CapabilityScope::Wallet => "wallet",
               CapabilityScope::Global => "global",
            },
            "privacy": entry.badge.map(|badge| match badge {
               PrivacyBadge::Reveals => "reveals",
               PrivacyBadge::BreaksAnonymity => "breaks_anonymity",
            }),
            "granted": granted(profile, entry.name),
            "switch": GRANT_LOCATION,
         })
      })
      .collect()
}
