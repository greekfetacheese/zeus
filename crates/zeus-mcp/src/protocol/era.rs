//! Which revision one request speaks, decided per request and never per connection.
//!
//! The stateless revision removed the handshake, so a client tells the server what it speaks every time
//! it asks for something. The handshake revision has no such field: it negotiated once, at `initialize`,
//! and every message after that is plain. One endpoint therefore has to recognise both, and the only
//! honest signal is the request itself — the presence of the modern `_meta` fields, or the method name
//! `initialize`. Keying the fallback to one error code, as some clients do, is exactly what the protocol
//! warns against.

use super::{LEGACY_VERSION, LEGACY_VERSIONS, MODERN_VERSION, meta};
use crate::error::ProtocolError;
use serde_json::Value;

/// What one request speaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Era {
   /// A client that opened with `initialize`.
   Legacy,
   /// A client that states its revision on every request.
   Modern {
      /// What the client called itself, when it said — the audit entry's client field.
      client_info: Option<String>,
   },
}

impl Era {
   pub const fn is_modern(&self) -> bool {
      matches!(self, Self::Modern { .. })
   }

   /// The word the status tool and the log use.
   pub const fn label(&self) -> &'static str {
      match self {
         Self::Legacy => "legacy",
         Self::Modern { .. } => "modern",
      }
   }

   /// What the harness called itself, if it said.
   pub fn client_info(&self) -> Option<&str> {
      match self {
         Self::Legacy => None,
         Self::Modern { client_info } => client_info.as_deref(),
      }
   }
}

/// The headers the modern era defines, as the transport read them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Headers {
   pub protocol_version: Option<String>,
   pub method: Option<String>,
   pub name: Option<String>,
}

/// Decide which era a request speaks, and refuse a modern one this build cannot serve.
pub fn detect(method: &str, params: Option<&Value>) -> Result<Era, ProtocolError> {
   // The handshake only exists in the legacy era: a client that sends it has not met a modern server.
   if method == "initialize" {
      return Ok(Era::Legacy);
   }

   // A legacy client may still send `_meta` for its progress token, so the marker is the protocol
   // version inside it, not `_meta` itself.
   let Some(version) = params
      .and_then(|params| params.get("_meta"))
      .and_then(|meta| meta.get(meta::PROTOCOL_VERSION))
   else {
      return Ok(Era::Legacy);
   };

   let Some(version) = version.as_str() else {
      return Err(ProtocolError::missing_meta(
         meta::PROTOCOL_VERSION,
      ));
   };

   if !version.eq_ignore_ascii_case(MODERN_VERSION) {
      return Err(ProtocolError::unsupported_version(
         version,
         &[MODERN_VERSION],
      ));
   }

   let client_capabilities = params
      .and_then(|params| params.get("_meta"))
      .and_then(|meta| meta.get(meta::CLIENT_CAPABILITIES))
      .ok_or_else(|| ProtocolError::missing_meta(meta::CLIENT_CAPABILITIES))?;

   if !client_capabilities.is_object() {
      return Err(ProtocolError::missing_meta(
         meta::CLIENT_CAPABILITIES,
      ));
   }

   let client_info = params
      .and_then(|params| params.get("_meta"))
      .and_then(|meta| meta.get(meta::CLIENT_INFO))
      .and_then(describe_client);

   Ok(Era::Modern { client_info })
}

/// Check the headers against the body they arrived with.
///
/// A modern request must carry the revision and the method, and the name header for the two methods
/// that take one. A legacy client is told its negotiated revision back, so a header naming a revision no
/// handshake could have produced is a client bug worth naming. The version comparison is
/// case-insensitive, which is what the protocol asks of a version header.
pub fn check_headers(
   era: &Era,
   headers: &Headers,
   method: &str,
   name: Option<&str>,
) -> Result<(), ProtocolError> {
   match era {
      Era::Legacy => {
         if let Some(version) = &headers.protocol_version
            && !LEGACY_VERSIONS.iter().any(|known| known.eq_ignore_ascii_case(version))
         {
            return Err(ProtocolError::header_mismatch(
               "MCP-Protocol-Version",
               LEGACY_VERSION,
               Some(version),
            ));
         }
      }
      Era::Modern { .. } => {
         let version = headers.protocol_version.as_deref().ok_or_else(|| {
            ProtocolError::header_mismatch("MCP-Protocol-Version", MODERN_VERSION, None)
         })?;

         if !version.eq_ignore_ascii_case(MODERN_VERSION) {
            return Err(ProtocolError::header_mismatch(
               "MCP-Protocol-Version",
               MODERN_VERSION,
               Some(version),
            ));
         }

         match headers.method.as_deref() {
            Some(found) if found == method => {}
            found => {
               return Err(ProtocolError::header_mismatch(
                  "Mcp-Method",
                  method,
                  found,
               ));
            }
         }

         if let Some(name) = name {
            match headers.name.as_deref() {
               Some(found) if found == name => {}
               found => {
                  return Err(ProtocolError::header_mismatch(
                     "Mcp-Name", name, found,
                  ));
               }
            }
         }
      }
   }

   Ok(())
}

/// Whether `method` is one whose `params` carry a name the `Mcp-Name` header must repeat.
pub fn names_an_object(method: &str) -> bool {
   matches!(
      method,
      "tools/call" | "resources/read" | "prompts/get"
   )
}

/// The client as one word: `name/version`, or whichever half it sent.
fn describe_client(info: &Value) -> Option<String> {
   let name = info.get("name").and_then(Value::as_str);
   let version = info.get("version").and_then(Value::as_str);

   match (name, version) {
      (Some(name), Some(version)) => Some(format!("{name}/{version}")),
      (Some(name), None) => Some(name.to_owned()),
      (None, Some(version)) => Some(version.to_owned()),
      (None, None) => None,
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use serde_json::json;

   fn modern_params() -> Value {
      json!({"_meta": {
         meta::PROTOCOL_VERSION: MODERN_VERSION,
         meta::CLIENT_CAPABILITIES: {},
         meta::CLIENT_INFO: {"name": "harness", "version": "1.2"},
      }})
   }

   #[test]
   fn a_plain_request_is_legacy_and_a_stated_version_is_modern() {
      assert_eq!(detect("tools/list", None), Ok(Era::Legacy));

      let era = detect("tools/list", Some(&modern_params())).expect("a modern request");
      assert!(era.is_modern());
      assert_eq!(era.client_info(), Some("harness/1.2"));
   }

   #[test]
   fn initialize_is_always_legacy() {
      let params = modern_params();
      assert_eq!(
         detect("initialize", Some(&params)),
         Ok(Era::Legacy)
      );
   }

   #[test]
   fn a_meta_without_a_version_is_legacy_so_a_progress_token_still_works() {
      let params = json!({"_meta": {"progressToken": "p-1"}});
      assert_eq!(
         detect("tools/call", Some(&params)),
         Ok(Era::Legacy)
      );
   }

   #[test]
   fn an_unknown_version_is_refused_with_the_list_it_does_speak() {
      let params = json!({"_meta": {meta::PROTOCOL_VERSION: "2020-01-01"}});
      let error = detect("tools/list", Some(&params)).expect_err("an unknown revision");
      assert_eq!(error.code, ProtocolError::UNSUPPORTED_VERSION);
      assert!(error.rejected);
      assert_eq!(
         error.data.as_ref().and_then(|data| data["supported"].as_array()).map(Vec::len),
         Some(1)
      );
   }

   #[test]
   fn a_missing_client_capabilities_field_is_a_rejection() {
      let params = json!({"_meta": {meta::PROTOCOL_VERSION: MODERN_VERSION}});
      let error = detect("tools/list", Some(&params)).expect_err("a required field is missing");
      assert_eq!(error.code, ProtocolError::INVALID_PARAMS);
      assert!(error.rejected);
   }

   #[test]
   fn a_modern_request_must_repeat_its_method_and_name() {
      let era = Era::Modern { client_info: None };
      let headers = Headers {
         protocol_version: Some(MODERN_VERSION.to_owned()),
         method: Some("tools/call".to_owned()),
         name: Some("zeus_status".to_owned()),
      };

      assert!(check_headers(&era, &headers, "tools/call", Some("zeus_status")).is_ok());
      assert!(check_headers(&era, &headers, "tools/list", None).is_err());
      assert!(check_headers(&era, &headers, "tools/call", Some("wallet_list")).is_err());

      let bare = Headers {
         protocol_version: Some(MODERN_VERSION.to_owned()),
         ..Default::default()
      };
      let error = check_headers(&era, &bare, "tools/list", None).expect_err("no method header");
      assert_eq!(error.code, ProtocolError::HEADER_MISMATCH);
      assert!(error.rejected);
   }

   #[test]
   fn a_legacy_client_may_name_any_revision_it_could_have_negotiated() {
      let headers = Headers {
         protocol_version: Some("2024-11-05".to_owned()),
         ..Default::default()
      };
      assert!(check_headers(&Era::Legacy, &headers, "tools/list", None).is_ok());

      let headers = Headers {
         protocol_version: Some("1999-01-01".to_owned()),
         ..Default::default()
      };
      assert!(check_headers(&Era::Legacy, &headers, "tools/list", None).is_err());
   }
}
