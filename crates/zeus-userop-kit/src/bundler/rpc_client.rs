use std::sync::atomic::{AtomicU64, Ordering};

use reqwest::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

pub struct RpcClient {
   client: Client,
   url: reqwest::Url,
   id: AtomicU64,
}

#[derive(Debug)]
pub enum RpcClientError {
   Http(reqwest::Error),
   Json(serde_json::Error),
   Rpc { code: i64, message: String },
}

impl std::fmt::Display for RpcClientError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::Http(err) => write!(f, "HTTP error: {err}"),
         Self::Json(err) => write!(f, "JSON error: {err}"),
         Self::Rpc { code, message } => write!(f, "RPC error {code}: {message}"),
      }
   }
}

impl std::error::Error for RpcClientError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::Http(err) => Some(err),
         Self::Json(err) => Some(err),
         Self::Rpc { .. } => None,
      }
   }
}

impl From<reqwest::Error> for RpcClientError {
   fn from(err: reqwest::Error) -> Self {
      Self::Http(err)
   }
}

impl From<serde_json::Error> for RpcClientError {
   fn from(err: serde_json::Error) -> Self {
      Self::Json(err)
   }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RpcResponse<T> {
   Success { result: T },
   Failure { error: RpcError },
}

#[derive(Debug, Deserialize)]
struct RpcError {
   code: i64,
   message: String,
}

impl RpcClient {
   pub fn new(url: reqwest::Url) -> Self {
      Self {
         client: Client::new(),
         url,
         id: AtomicU64::new(1),
      }
   }

   pub async fn request<P, R>(&self, method: &str, params: P) -> Result<R, RpcClientError>
   where
      P: Serialize,
      R: DeserializeOwned,
   {
      let id = self.id.fetch_add(1, Ordering::Relaxed);
      let body = json!({
          "jsonrpc": "2.0",
          "id": id,
          "method": method,
          "params": params,
      });

      let text = self.client.post(self.url.clone()).json(&body).send().await?.text().await?;
      let resp: RpcResponse<Value> = serde_json::from_str(&text)?;

      let resp = match resp {
         RpcResponse::Success { result } => result,
         RpcResponse::Failure { error } => {
            return Err(RpcClientError::Rpc {
               code: error.code,
               message: error.message,
            });
         }
      };

      let result = serde_json::from_value(resp)?;
      Ok(result)
   }
}

#[cfg(test)]
mod tests {
   use std::error::Error as _;

   use super::*;

   fn http_error() -> reqwest::Error {
      reqwest::Client::new()
         .get("not a url")
         .build()
         .expect_err("the input is not a URL")
   }

   fn json_error() -> serde_json::Error {
      serde_json::from_str::<serde_json::Value>("not json")
         .expect_err("the input is not valid JSON")
   }

   #[test]
   fn renders_the_pinned_messages() {
      let http = RpcClientError::Http(http_error());
      assert_eq!(
         http.to_string(),
         format!("HTTP error: {}", http_error())
      );

      let json = RpcClientError::Json(json_error());
      assert_eq!(
         json.to_string(),
         format!("JSON error: {}", json_error())
      );

      let rpc = RpcClientError::Rpc {
         code: -32601,
         message: "method not found".to_owned(),
      };
      assert_eq!(
         rpc.to_string(),
         "RPC error -32601: method not found"
      );
   }

   #[test]
   fn only_the_from_variants_carry_a_source() {
      assert!(RpcClientError::Http(http_error()).source().is_some());
      assert!(RpcClientError::Json(json_error()).source().is_some());
      assert!(
         RpcClientError::Rpc {
            code: 0,
            message: String::new(),
         }
         .source()
         .is_none()
      );
   }
}
