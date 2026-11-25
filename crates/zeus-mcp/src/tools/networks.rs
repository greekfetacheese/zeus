//! The chains, their endpoints and their health.
//!
//! An endpoint is what an agent's reads run through, so this is the vocabulary a caller needs to explain
//! a failure: a disabled chain is why a call said unsupported, an unavailable node is why it said network.

use super::{
   Call, ToolFuture, ToolOutput, ToolSpec, ask_user, chain_argument, decide, record_allowed,
   string_argument,
};
use crate::error::{ErrorCode, ToolError};
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use zeus_core::agent::{Decision, Intent, read};
use zeus_core::platform::url_host;

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
pub const SET_RPC: ToolSpec = ToolSpec {
   name: "networks_set_rpc",
   title: "Change one RPC endpoint",
   description: "Add, remove, enable, disable or MEV-protect one RPC endpoint on one chain. Every read \
      and every send goes through an endpoint, so a hostile one lies about balances and censors the \
      broadcasts it was trusted with — that is why this needs its own write switch. Adding proves the \
      endpoint answers for the chain it is filed under before anything is stored.",
   schema: r#"{"type": "object", "properties": {"chain": {"type": "integer", \
      "description": "A chain id from networks_list."}, "url": {"type": "string", \
      "description": "The endpoint's URL."}, "action": {"type": "string", \
      "enum": ["add", "remove", "enable", "disable", "mev_protect"], \
      "description": "What to do with the endpoint."}, "value": {"type": "boolean", \
      "description": "Only with mev_protect: whether this endpoint should send transactions through a \
      private relay."}}, "required": ["chain", "url", "action"], "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some("settings.networks.write"),
   call: set_rpc,
};

/// The switch this tool needs, named once: the descriptor's capability and every record use it.
const CAPABILITY: &str = "settings.networks.write";

fn set_rpc(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let chain = chain_argument(&arguments)?.ok_or_else(|| {
         ToolError::validation("chain is required: networks_list names the chains")
      })?;

      let action = string_argument(&arguments, "action")?.to_owned();
      let url = string_argument(&arguments, "url")?.trim().to_owned();

      if url.is_empty() {
         return Err(ToolError::validation(
            "url is required: the endpoint to act on",
         ));
      }

      // The table's gate ran already, so this second look is only for the mode where the profile asks
      // about every change — a decision that gate flattens to "allowed".
      if matches!(
         decide(call, "networks_set_rpc", &intent()),
         Decision::Allowed { prompt: true }
      ) {
         let question = format!("Let the agent change an RPC endpoint on chain {chain}?");
         ask_user(call, "networks_set_rpc", CAPABILITY, &question).await?;
      }

      let manager = call.ctx.get_client_manager();

      match action.as_str() {
         "add" => {
            if manager.get_rpcs(chain).contains_key(url.as_str()) {
               return Err(ToolError::validation(format!(
                  "{url} is already listed for chain {chain}: remove it first"
               )));
            }

            call.ctx.add_rpc_endpoint(chain, &url).await.map_err(|e| {
               ToolError::new(ErrorCode::Network, e.to_string())
                  .with_hint("Check the URL and that the node is reachable: nothing was stored.")
            })?;
         }
         "remove" | "enable" | "disable" | "mev_protect" => {
            if !manager.get_rpcs(chain).contains_key(url.as_str()) {
               return Err(ToolError::new(
                  ErrorCode::NotFound,
                  format!("no endpoint {url} is listed for chain {chain}"),
               ));
            }

            match action.as_str() {
               "remove" => manager.remove_rpc(chain, url.as_str().into()),
               "enable" => manager.set_rpc_enabled(chain, &url, true),
               "disable" => manager.set_rpc_enabled(chain, &url, false),
               _ => {
                  let protect =
                     arguments.get("value").and_then(Value::as_bool).ok_or_else(|| {
                        ToolError::validation("value is required with mev_protect: true or false")
                     })?;

                  manager.set_rpc_mev_protect(chain, &url, protect);
               }
            }

            call.ctx.try_save_client_manager().map_err(|e| {
               ToolError::internal(format!(
                  "the endpoint changed but did not save: {e}"
               ))
            })?;
         }
         other => {
            return Err(ToolError::validation(format!(
               "action {other} is not one of add, remove, enable, disable, mev_protect"
            )));
         }
      }

      record_allowed(call, "networks_set_rpc", CAPABILITY);

      // The host, never the whole URL: a provider key rides in the path or the query, and this sentence
      // is spoken.
      zeus_core::ui::announce(&format!(
         "Agent changed an RPC endpoint ({action}) on chain {chain}: {}",
         url_host(&url).unwrap_or("an endpoint")
      ));

      let mut data = json!({"chain": chain, "url": url, "action": action});

      // Answering what the endpoint now is takes a read, so it needs the read switch as well as the
      // write — the same split a settings answer uses.
      if call.profile.allows_global("settings.networks.read")
         && let Some(endpoint) = manager.get_rpcs(chain).get(url.as_str())
      {
         data["endpoint"] = json!({
             "url": endpoint.url.as_ref(),
             "default": endpoint.default,
             "enabled": endpoint.enabled,
             "mev_protect": endpoint.mev_protect,
             "working": endpoint.check.working,
         });
      }

      Ok(ToolOutput::new(data))
   })
}

/// The intent one call of this tool is decided against.
fn intent() -> Intent {
   Intent::GlobalWrite {
      capability: CAPABILITY,
   }
}
