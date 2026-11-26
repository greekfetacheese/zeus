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

pub mod action;
pub mod approvals;
pub mod bridge;
pub mod contacts;
pub mod dapps;
pub mod data;
pub mod meta;
pub mod networks;
pub mod pools;
pub mod portfolio;
pub mod railgun;
pub mod settings;
pub mod swap;
pub mod tx;
pub mod wallet;

#[cfg(test)]
mod schemas;

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
use zeus_core::agent::exec::ExecError;
use zeus_core::agent::read::ReadError;
use zeus_core::agent::settings::SettingsError;
use zeus_core::agent::{
   AgentProfile, AuditDecision, AuditEntry, CAPABILITIES, CapabilityScope, CapabilityVerb,
   Decision, Denial, Intent, PrivacyBadge, announce_denial, capability, precheck,
};
use zeus_core::tx::PriorityTier;
use zeus_eth::alloy_primitives::{Address, U256};
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

   /// The same answer with the action's own block beside its data.
   ///
   /// It says which id the call carried, whether this answer is a replay of an earlier one, and — when the
   /// caller named no id — that a retry would be a second action rather than the same one.
   pub fn stamped(mut self, action: Value) -> Self {
      if let Value::Object(map) = &mut self.data {
         map.insert("action".to_owned(), action);
      }

      self
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
   meta::ACTIVITY,
   approvals::LIST,
   approvals::REVOKE,
   contacts::ADD,
   contacts::LIST,
   contacts::REMOVE,
   dapps::DISCONNECT,
   dapps::LIST,
   data::EXPORT,
   data::IMPORT,
   wallet::DELEGATE_SET,
   wallet::DELEGATE,
   networks::LIST,
   networks::SET_RPC,
   pools::ADD,
   pools::LIST,
   portfolio::GET,
   swap::QUOTE,
   railgun::MERGE_NOTES,
   railgun::SHIELD,
   railgun::STATUS,
   railgun::UNSHIELD,
   settings::DESCRIBE,
   settings::GET,
   settings::LIST,
   settings::SET,
   bridge::BRIDGE,
   tx::GET,
   tx::HISTORY,
   tx::SEND,
   tx::SEND_NFT,
   swap::SWAP,
   wallet::GET,
   wallet::LIST,
   wallet::DERIVE,
   wallet::IMPORT,
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
      let entry = capability(name);
      let scoped = entry.is_some_and(|entry| entry.scope == CapabilityScope::Wallet);
      // A wallet-scoped capability is decided against the wallet the call names. Its verb is what decides
      // *which* question: the money tools are writes, and describing one as a read here would decide the
      // wrong one — the asset envelope the handler's own precheck applies is the second half.
      let writes = entry.is_some_and(|entry| entry.verb() == CapabilityVerb::Write);

      let intent = match (scoped, writes) {
         (true, true) => Intent::WalletWrite {
            capability: name,
            wallet: wallet_argument(&arguments)?,
         },
         (true, false) => Intent::WalletRead {
            capability: name,
            wallet: wallet_argument(&arguments)?,
         },
         _ => Intent::GlobalRead { capability: name },
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

/// The refusal a settings patch becomes.
///
/// A patch of the wrong shape is the model's to fix, so it is a validation error that names the field; a
/// store that would not write is not, so it is internal.
pub fn settings_error(error: SettingsError) -> ToolError {
   match &error {
      SettingsError::Invalid { .. }
      | SettingsError::RuntimeOnly { .. }
      | SettingsError::Closed { .. } => ToolError::validation(error.to_string()),
      SettingsError::Unsupported { .. } => {
         ToolError::new(ErrorCode::Unsupported, error.to_string())
      }
      SettingsError::Busy => ToolError::new(ErrorCode::Busy, error.to_string()),
      SettingsError::Rejected => ToolError::new(ErrorCode::UserRejected, error.to_string()),
      SettingsError::NoUi => ToolError::new(ErrorCode::UserActionRequired, error.to_string()),
      SettingsError::Refused { .. } => ToolError::internal(error.to_string()),
   }
}

/// A chain a call cannot run without.
pub fn required_chain(arguments: &Value) -> Result<u64, ToolError> {
   chain_argument(arguments)?.ok_or_else(|| {
      ToolError::validation("chain is required: every transaction happens on one chain")
   })
}

/// A chain argument under another key, for the calls that name two.
pub fn required_chain_key(arguments: &Value, key: &str) -> Result<u64, ToolError> {
   let value = arguments
      .get(key)
      .ok_or_else(|| ToolError::validation(format!("{key} is required")))?;

   let chain = value
      .as_u64()
      .ok_or_else(|| ToolError::validation(format!("{key} must be a number")))?;

   if !ChainId::is_supported(chain) {
      return Err(ToolError::new(
         ErrorCode::Unsupported,
         format!("chain {chain} is not one Zeus supports"),
      ));
   }

   Ok(chain)
}

/// An address argument a call cannot run without.
pub fn required_address(arguments: &Value, key: &str) -> Result<Address, ToolError> {
   string_argument(arguments, key)?.parse().map_err(|_| {
      ToolError::validation(format!(
         "{key} is not an address: it has to be full and 0x-prefixed"
      ))
   })
}

/// An address argument the call may leave out. The zero address is read as "absent", because a caller that
/// means the chain's own coin has two ways to say so and only one of them should reach the flow.
pub fn optional_address(arguments: &Value, key: &str) -> Result<Option<Address>, ToolError> {
   match arguments.get(key) {
      None | Some(Value::Null) => Ok(None),
      Some(_) => {
         let address = required_address(arguments, key)?;

         Ok((!address.is_zero()).then_some(address))
      }
   }
}

/// A `u256` argument, as a decimal string: a JSON number does not survive the trip.
pub fn u256_argument(arguments: &Value, key: &str) -> Result<U256, ToolError> {
   string_argument(arguments, key)?.parse().map_err(|_| {
      ToolError::validation(format!(
         "{key} is a whole number, written as a string"
      ))
   })
}

/// The slippage a swap or a quote allows, in basis points.
///
/// It defaults to the swap page's own default rather than to the grant's cap: a call that says nothing gets
/// what the page offers, and only a call that asks for more is refused.
pub fn slippage_argument(arguments: &Value) -> Result<u16, ToolError> {
   let bps = match arguments.get("slippage_bps") {
      None | Some(Value::Null) => return Ok(5),
      Some(value) => value.as_u64().filter(|bps| *bps <= 10_000).ok_or_else(|| {
         ToolError::validation("slippage_bps is a whole number of basis points, at most 10000")
      })?,
   };

   Ok(bps as u16)
}

/// How long a swap stays valid, in minutes.
pub fn deadline_argument(arguments: &Value) -> Result<u64, ToolError> {
   let minutes = match arguments.get("deadline") {
      None | Some(Value::Null) => return Ok(5),
      Some(value) => value
         .as_u64()
         .ok_or_else(|| ToolError::validation("deadline is a whole number of minutes"))?,
   };

   if !(1..=60).contains(&minutes) {
      return Err(ToolError::validation(
         "deadline is between 1 and 60 minutes, the range the swap page offers",
      ));
   }

   Ok(minutes)
}

/// The `dry_run` flag every fund-moving tool carries.
pub fn dry_run_argument(arguments: &Value) -> Result<bool, ToolError> {
   match arguments.get("dry_run") {
      None | Some(Value::Null) => Ok(false),
      Some(value) => {
         value.as_bool().ok_or_else(|| ToolError::validation("dry_run is true or false"))
      }
   }
}

/// The fee tier a fund-moving tool carries.
///
/// `fast` when the call says nothing: that is the tier a granted send has always used, so a caller that
/// never mentions a tier keeps the behaviour it had. Only the grant path reads it — a confirmation the user
/// answers states their own choice.
pub fn priority_argument(arguments: &Value) -> Result<PriorityTier, ToolError> {
   match arguments.get("priority").map(|value| value.as_str()) {
      None | Some(None) | Some(Some("fast")) => Ok(PriorityTier::Fast),
      Some(Some("normal")) => Ok(PriorityTier::Normal),
      Some(Some("very_fast")) => Ok(PriorityTier::VeryFast),
      Some(Some(other)) => Err(ToolError::validation(format!(
         "priority is normal, fast or very_fast; not {other}"
      ))),
   }
}

/// The refusal a fund-moving action becomes.
///
/// The pipeline answers in its own terms — a lock, a busy wallet, a declined window, a policy denial, the
/// flow's own no — and each is one of the codes a client branches on. The one that is not a code of its own
/// is an unclassified flow refusal: the pipeline has no typed error (`ground-truth.md` C10), so it arrives
/// as the app's sentence, under the code whose meaning is "report this".
pub fn exec_error(error: ExecError, capability: &str) -> ToolError {
   let code = match &error {
      ExecError::Locked => ErrorCode::VaultLocked,
      ExecError::Busy => ErrorCode::Busy,
      ExecError::Rejected => ErrorCode::UserRejected,
      ExecError::NoUi => ErrorCode::UserActionRequired,
      ExecError::Invalid(_) => ErrorCode::Validation,
      ExecError::Denied(denial) => return denial_error(denial, capability),
      ExecError::Unavailable => ErrorCode::Unsupported,
      ExecError::Failed(_) => {
         return ToolError::internal(error.to_string()).with_hint(
            "The app refused this before broadcasting it; the message is its own words. Report it, and \
             do not retry blindly.",
         );
      }
   };

   ToolError::new(code, error.to_string())
}

/// Decide a request against the grant, and record a refusal.
///
/// This is the precheck the pipeline's own rule asks for: a guard that can stop an action runs before
/// anything is built, simulated or signed. It is not the authority — the confirmation seam decides again
/// from the simulation — but it is what makes a refusal cost nothing.
pub fn guard(call: &Call, subject: &str, intent: &Intent) -> Result<(), ToolError> {
   refusal(intent, decide(call, subject, intent))
}

/// The decision one request gets, with a refusal recorded.
///
/// Separate from `refusal` so a caller that has to *act* on `Allowed { prompt: true }` — a write in a
/// profile that confirms every change — can see it instead of having it flattened to `Ok`.
pub fn decide(call: &Call, subject: &str, intent: &Intent) -> Decision {
   let started = Instant::now();
   let decision = precheck(&call.ctx, &call.profile, intent);

   if !matches!(decision, Decision::Allowed { .. }) {
      record(call, subject, intent, &decision, started);
   }

   decision
}

/// The answer a decision that is not "run it now" becomes at a tool boundary.
pub fn refusal(intent: &Intent, decision: Decision) -> Result<(), ToolError> {
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
      // A simulation that refused is the same family: the action was allowed by the grant and stopped by
      // what it would have done. Each carries its own code so a model can say which one it was.
      Denial::PriceImpact { .. }
      | Denial::OutputShortfall { .. }
      | Denial::PriceUnavailable { .. } => ErrorCode::SimulationRefused,
   };

   let error = ToolError::new(code, denial.to_string());

   match code {
      ErrorCode::NotGranted => error.with_hint(format!(
         "Ask the app owner to grant {capability} in {GRANT_LOCATION}, then try again."
      )),
      _ => error,
   }
}

/// Ask the user before a write, and record what they answered.
///
/// `precheck` has always answered `Allowed { prompt: true }` for a write in the mode where every change
/// is confirmed, and the gate in front of a tool only looks at "allowed" — so the tool that can reach
/// such a write asks here, through the app's generic yes/no window. No answer is not "yes": a busy
/// window, a declined prompt and a window that cannot be drawn are three refusals, and each is recorded
/// before it is answered, because "it asked and nobody agreed" is exactly what a user needs to see.
pub async fn ask_user(
   call: &Call,
   subject: &str,
   capability: &str,
   title: &str,
) -> Result<(), ToolError> {
   let Err(error) = zeus_core::agent::settings::confirm(title).await else {
      return Ok(());
   };

   let code = match &error {
      SettingsError::Busy => "BUSY",
      SettingsError::Rejected => "USER_REJECTED",
      _ => "NO_UI",
   };

   record_refusal(
      call,
      subject,
      capability,
      code,
      &error.to_string(),
   );

   Err(settings_error(error))
}

/// Decide one write's own question, and ask the user when the profile asks about every change.
///
/// A tool whose descriptor names a static capability is decided by [`run`] before it executes, and that
/// decision flattens "allowed, but this profile wants to see every change" to "allowed". A write handler
/// therefore decides again through the intent that carries its real shape — the wallet, the asset, the
/// export's folder — and asks when the mode says so.
pub async fn gate_write(
   call: &Call,
   tool: &str,
   intent: &Intent,
   title: &str,
) -> Result<(), ToolError> {
   match decide(call, tool, intent) {
      Decision::Allowed { prompt: true } => ask_user(call, tool, intent.capability(), title).await,
      decision => refusal(intent, decision),
   }
}

/// Write one refusal to the audit ring, and say a denial out loud.
///
/// The ring lives in memory. Sealing it to disk from here would let a caller retrying a refusal — the
/// one case the rate limit exists for — rewrite the whole log once per attempt.
///
/// The sentence is the other half of the trace: nothing asked the user first, so a reader who never
/// sees the page hears it through the same `ui::announce` a landed write uses. The pipeline says its
/// own denials through the same function, so one refusal is one wording.
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

   if let Decision::Denied(denial) = decision {
      announce_denial(denial);
   }
}

/// Record a write that ran.
///
/// A refusal is recorded by the gate in front of every call; an allowed write is recorded here, by the
/// handler that performed it, because only it knows the write landed.
pub fn record_allowed(call: &Call, subject: &str, capability: &str) {
   let mut entry = AuditEntry::new(&call.profile, subject, capability, now_secs())
      .decided(&Decision::Allowed { prompt: false });

   entry.client_info = call.client_info();

   call.ctx.agent_audit().record(entry);
}

/// Record a write that was refused after the gate: the user said no, or no window could ask.
///
/// The gate records a policy refusal; this records the one the *user* made, which is the other half of
/// "writes are always audited".
pub fn record_refusal(call: &Call, subject: &str, capability: &str, code: &str, detail: &str) {
   let mut entry = AuditEntry::new(&call.profile, subject, capability, now_secs());

   entry.decision = AuditDecision::Denied;
   entry.denial = Some(code.to_owned());
   entry.detail = Some(detail.to_owned());
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
