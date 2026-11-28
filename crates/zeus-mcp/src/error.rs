//! The two failure shapes, and the rule that keeps them apart.
//!
//! A protocol error says the *request* is wrong: an unknown tool, a malformed body, an unsupported
//! revision. A model cannot fix those, so a harness should surface them. A tool error says the *action*
//! was refused — the grant does not cover it, the vault is locked, the caller is too fast — and that is
//! exactly what a model can explain and retry, so it travels inside a successful result with
//! `isError: true` rather than in the JSON-RPC envelope.

use serde_json::{Value, json};
use std::fmt;

/// The stable code one refusal carries.
///
/// A client branches on the string, so a variant may be added but never renamed. The set is closed on
/// purpose: a code a model has no sentence for is worse than no code at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
   /// The capability is off for this profile or this wallet.
   NotGranted,
   /// The asset is outside the wallet's token allowlist.
   TokenNotAllowed,
   /// The NFT is outside the wallet's NFT allowlist.
   NftNotAllowed,
   /// The simulation refused what the grant allowed: the trade lost more than its ceiling, less arrived than
   /// the flow promised, or the output could not be priced.
   SimulationRefused,
   /// Zeus is locked, or was never unlocked in this run.
   VaultLocked,
   /// Another authorising prompt is already up.
   Busy,
   /// The user declined the prompt.
   UserRejected,
   /// Only the human can finish this one, in a window on the Zeus side.
   UserActionRequired,
   /// The arguments were rejected.
   Validation,
   /// Unknown wallet, transaction or screen — or one this profile may not see.
   NotFound,
   /// The node call failed.
   Network,
   /// The chain, token or pair is not supported.
   Unsupported,
   /// Too many calls; the answer says how long to wait.
   RateLimited,
   /// A Zeus bug.
   Internal,
}

impl ErrorCode {
   pub const fn as_str(self) -> &'static str {
      match self {
         Self::NotGranted => "NOT_GRANTED",
         Self::TokenNotAllowed => "TOKEN_NOT_ALLOWED",
         Self::NftNotAllowed => "NFT_NOT_ALLOWED",
         Self::SimulationRefused => "SIMULATION_REFUSED",
         Self::VaultLocked => "VAULT_LOCKED",
         Self::Busy => "BUSY",
         Self::UserRejected => "USER_REJECTED",
         Self::UserActionRequired => "USER_ACTION_REQUIRED",
         Self::Validation => "VALIDATION",
         Self::NotFound => "NOT_FOUND",
         Self::Network => "NETWORK",
         Self::Unsupported => "UNSUPPORTED",
         Self::RateLimited => "RATE_LIMITED",
         Self::Internal => "INTERNAL",
      }
   }

   /// What the model should do about it, in one sentence it can say out loud.
   ///
   /// Written for a listener, not a reader: no field names, no paths, no code.
   pub const fn hint(self) -> &'static str {
      match self {
         Self::NotGranted => "Ask the app owner to grant this in Zeus, then try again.",
         Self::TokenNotAllowed => {
            "Ask the app owner to add this token to the wallet's allowlist in Zeus."
         }
         Self::NftNotAllowed => {
            "Ask the app owner to add this collection to the wallet's NFT allowlist in Zeus."
         }
         Self::SimulationRefused => {
            "Do not retry the same amount. Wait for the price or the route to change, or ask the app              owner to widen the grant's ceiling."
         }
         Self::VaultLocked => "Ask the app owner to unlock Zeus; an agent cannot unlock it.",
         Self::Busy => "Tell the user to finish what is open in Zeus, then try again.",
         Self::UserRejected => "The user said no. Stop, and do not ask again immediately.",
         Self::UserActionRequired => {
            "Tell the user which window to finish in Zeus, then retry the call."
         }
         Self::Validation => "Fix the arguments the message names, then retry.",
         Self::NotFound => "Check the identifier, or ask whether the app owner gave you this one.",
         Self::Network => "Retry once; if it fails again, report it.",
         Self::Unsupported => "Report it: this chain, token or pair is not one Zeus can act on.",
         Self::RateLimited => "Wait the time the answer gives, then retry.",
         Self::Internal => "Report it: this is a Zeus bug, and the audit log has the detail.",
      }
   }

   /// One line a reader can be told without the code in front of them.
   pub const fn meaning(self) -> &'static str {
      match self {
         Self::NotGranted => "The capability is off for this profile or wallet",
         Self::TokenNotAllowed => "The token is not in this wallet's allowlist",
         Self::NftNotAllowed => "The NFT is not in this wallet's allowlist",
         Self::SimulationRefused => "The simulation did not match what the flow promised",
         Self::VaultLocked => "Zeus is locked",
         Self::Busy => "Another prompt is already up in Zeus",
         Self::UserRejected => "The user declined",
         Self::UserActionRequired => "The user has to finish this in Zeus",
         Self::Validation => "The arguments were rejected",
         Self::NotFound => "There is no such object, or it was not granted",
         Self::Network => "The node did not answer",
         Self::Unsupported => "Zeus cannot act on this chain, token or pair",
         Self::RateLimited => "Too many calls in a minute",
         Self::Internal => "A Zeus bug",
      }
   }

   /// Every code, in the order a help screen reads them.
   pub const ALL: [Self; 14] = [
      Self::NotGranted,
      Self::TokenNotAllowed,
      Self::NftNotAllowed,
      Self::SimulationRefused,
      Self::VaultLocked,
      Self::Busy,
      Self::UserRejected,
      Self::UserActionRequired,
      Self::Validation,
      Self::NotFound,
      Self::Network,
      Self::Unsupported,
      Self::RateLimited,
      Self::Internal,
   ];
}

/// A refusal the model can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolError {
   pub code: ErrorCode,
   /// What went wrong, naming the switch or the field.
   pub message: String,
   /// What to do next, in one sentence.
   pub hint: String,
   /// Set by [`ErrorCode::RateLimited`]: how long to wait.
   pub retry_after_ms: Option<u64>,
}

impl ToolError {
   /// A refusal with the code's own sentence as its hint.
   pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
      Self {
         code,
         message: message.into(),
         hint: code.hint().to_owned(),
         retry_after_ms: None,
      }
   }

   /// Replace the sentence with one written for this refusal.
   pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
      self.hint = hint.into();
      self
   }

   pub fn validation(message: impl Into<String>) -> Self {
      Self::new(ErrorCode::Validation, message)
   }

   pub fn internal(message: impl Into<String>) -> Self {
      Self::new(ErrorCode::Internal, message)
   }

   /// The error as the result body carries it.
   pub fn to_value(&self) -> Value {
      let mut value = json!({
         "code": self.code.as_str(),
         "message": self.message,
         "hint": self.hint,
      });

      if let Some(retry) = self.retry_after_ms {
         value["retry_after_ms"] = json!(retry);
      }

      value
   }
}

impl fmt::Display for ToolError {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      write!(
         f,
         "{}: {} — {}",
         self.code.as_str(),
         self.message,
         self.hint
      )
   }
}

impl std::error::Error for ToolError {}

impl From<anyhow::Error> for ToolError {
   fn from(error: anyhow::Error) -> Self {
      Self::internal(error.to_string())
   }
}

/// Where one JSON-RPC error stands: a normal answer, or a request the transport must reject.
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolError {
   /// The JSON-RPC code. The protocol's own range only: Zeus's refusals never live here.
   pub code: i32,
   pub message: String,
   pub data: Option<Value>,
   /// Whether the transport owes a `400 Bad Request` rather than a `200`.
   ///
   /// The protocol names three cases that MUST be a transport failure (a missing required `_meta`
   /// field, an unsupported revision, a header that disagrees with the body) and leaves the rest in the
   /// body. It is a property of the case, not of the code, so it is carried rather than derived.
   pub rejected: bool,
}

impl ProtocolError {
   pub const PARSE_ERROR: i32 = -32700;
   pub const INVALID_REQUEST: i32 = -32600;
   pub const METHOD_NOT_FOUND: i32 = -32601;
   pub const INVALID_PARAMS: i32 = -32602;
   pub const INTERNAL_ERROR: i32 = -32603;
   /// A header disagrees with the body it arrived with.
   pub const HEADER_MISMATCH: i32 = -32020;
   /// The request asked for a capability the client never declared.
   pub const MISSING_CLIENT_CAPABILITY: i32 = -32021;
   /// The revision in the request is not one this server speaks.
   pub const UNSUPPORTED_VERSION: i32 = -32022;

   pub fn new(code: i32, message: impl Into<String>) -> Self {
      Self {
         code,
         message: message.into(),
         data: None,
         rejected: false,
      }
   }

   /// An error the transport must answer `400 Bad Request` to.
   pub fn rejecting(code: i32, message: impl Into<String>) -> Self {
      Self {
         code,
         message: message.into(),
         data: None,
         rejected: true,
      }
   }

   pub fn with_data(mut self, data: Value) -> Self {
      self.data = Some(data);
      self
   }

   pub fn invalid_request(message: impl Into<String>) -> Self {
      Self::new(Self::INVALID_REQUEST, message)
   }

   pub fn invalid_params(message: impl Into<String>) -> Self {
      Self::new(Self::INVALID_PARAMS, message)
   }

   pub fn unknown_method(method: &str) -> Self {
      Self::new(
         Self::METHOD_NOT_FOUND,
         format!("Unknown method: {method}"),
      )
   }

   pub fn unknown_tool(name: &str) -> Self {
      Self::new(
         Self::INVALID_PARAMS,
         format!("Unknown tool: {name}"),
      )
   }

   pub fn unknown_resource(uri: &str) -> Self {
      Self::new(
         Self::INVALID_PARAMS,
         format!("Unknown resource: {uri}"),
      )
   }

   pub fn header_mismatch(header: &str, expected: &str, found: Option<&str>) -> Self {
      let found = found.unwrap_or("absent");
      Self::rejecting(
         Self::HEADER_MISMATCH,
         format!("{header} says {found}, the body says {expected}"),
      )
   }

   pub fn unsupported_version(requested: &str, supported: &[&str]) -> Self {
      Self::rejecting(
         Self::UNSUPPORTED_VERSION,
         format!("Unsupported protocol version: {requested}"),
      )
      .with_data(json!({"requested": requested, "supported": supported}))
   }

   pub fn missing_meta(key: &str) -> Self {
      Self::rejecting(
         Self::INVALID_PARAMS,
         format!("Missing required _meta field: {key}"),
      )
      .with_data(json!({"required": [key]}))
   }

   pub fn internal(message: impl Into<String>) -> Self {
      Self::new(Self::INTERNAL_ERROR, message)
   }
}

impl fmt::Display for ProtocolError {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      write!(f, "{} ({})", self.message, self.code)
   }
}
