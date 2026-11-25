//! JSON-RPC 2.0 as this transport sees it: one object in, one object out, no batching.
//!
//! Two rules of the dialect differ from stock JSON-RPC and are checked here rather than left to a
//! caller: the id must not be `null`, and `jsonrpc` must say `"2.0"`. Everything else is shape work.

use crate::error::ProtocolError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A request id: a string or a number, and never anything else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
   Number(i64),
   Text(String),
}

impl From<&Id> for Value {
   fn from(id: &Id) -> Self {
      match id {
         Id::Number(number) => json!(number),
         Id::Text(text) => json!(text),
      }
   }
}

/// One message, read far enough to know which method it is for.
#[derive(Clone, Debug, PartialEq)]
pub struct Incoming {
   /// `None` for a notification, which the protocol owes no answer.
   pub id: Option<Id>,
   pub method: String,
   pub params: Option<Value>,
}

impl Incoming {
   /// Whether nothing at all is owed back.
   pub const fn is_notification(&self) -> bool {
      self.id.is_none()
   }

   /// Read one JSON body as a request or a notification.
   pub fn parse(body: Value) -> Result<Self, ProtocolError> {
      let Value::Object(mut object) = body else {
         return Err(ProtocolError::invalid_request(
            "a message must be a JSON object",
         ));
      };

      match object.get("jsonrpc") {
         Some(Value::String(version)) if version == "2.0" => {}
         _ => {
            return Err(ProtocolError::invalid_request(
               "the jsonrpc field must be \"2.0\"",
            ));
         }
      }

      let method = match object.remove("method") {
         Some(Value::String(method)) if !method.is_empty() => method,
         _ => {
            return Err(ProtocolError::invalid_request(
               "a message must name a method",
            ));
         }
      };

      // A present-but-null id is a protocol violation, not a notification: a notification has no id
      // member at all, and confusing the two would let a client wait for an answer that never comes.
      let id =
         match object.remove("id") {
            None => None,
            Some(Value::Null) => {
               return Err(ProtocolError::invalid_request(
                  "the id must not be null",
               ));
            }
            Some(value) => Some(serde_json::from_value(value).map_err(|_| {
               ProtocolError::invalid_request("the id must be a string or a number")
            })?),
         };

      let params = match object.remove("params") {
         None => None,
         Some(value @ (Value::Object(_) | Value::Array(_))) => Some(value),
         Some(_) => {
            return Err(ProtocolError::invalid_request(
               "params must be an object or an array",
            ));
         }
      };

      Ok(Self { id, method, params })
   }
}

/// A successful answer.
pub fn result(id: Id, value: Value) -> Value {
   json!({"jsonrpc": "2.0", "id": Value::from(&id), "result": value})
}

/// A failed answer.
///
/// The id is absent when the body could not be read far enough to have one; the protocol's own example
/// bodies carry `null` there, so the field is written rather than dropped.
pub fn error(id: Option<&Id>, error: &ProtocolError) -> Value {
   let mut payload = json!({
      "code": error.code,
      "message": error.message,
   });

   if let Some(data) = &error.data {
      payload["data"] = data.clone();
   }

   json!({
      "jsonrpc": "2.0",
      "id": id.map(Value::from).unwrap_or(Value::Null),
      "error": payload,
   })
}
