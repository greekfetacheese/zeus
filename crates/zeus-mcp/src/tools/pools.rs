//! The tracked Uniswap pools: the list, and the one verb that adds one.
//!
//! A pool row is what a quote would run against: the pair, the fee and the pool's identity. Seeing one
//! changes nothing, so the list is a plain read; adding one is a write, because a tracked pool is where
//! every quote and every swap on that pair will run.
//!
//! The lookup itself is the binary's: proving a pair exists means reading the chain through the app's own
//! pool code, which the edge cannot name. The row it answers is `read::pool_row`, so a pool an agent
//! adds is the row the list shows.

use super::{Call, ToolFuture, ToolOutput, ToolSpec, chain_argument, gate_write, record_allowed};
use crate::error::{ErrorCode, ToolError};
use crate::ops::{PoolRequest, PoolVersion};
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use std::str::FromStr;
use zeus_core::agent::{Intent, read};
use zeus_core::context::PoolError;
use zeus_core::ui;
use zeus_eth::alloy_primitives::Address;

/// The switch adding a pool needs: the screen's own write capability, from the catalog.
const CAPABILITY: &str = "settings.pools.write";

pub const LIST: ToolSpec = ToolSpec {
   name: "pools_list",
   title: "Pools",
   description: "The Uniswap pools Zeus tracks, with their pair, fee, DEX and identity, on one chain \
      or on every chain that has any.",
   schema: r#"{"type": "object", "properties": {"chain": {"type": "integer",
      "description": "Only this chain; every chain otherwise."}}, "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: Some("settings.pools.read"),
   call: list,
};

pub const ADD: ToolSpec = ToolSpec {
   name: "pools_add",
   title: "Add a Uniswap pool",
   description: "Start tracking one Uniswap pool. The chain is asked first: a pair with no pool at \
      that version and fee is refused rather than stored, and the answer carries the row the list will \
      show. A tracked pool is where this pair's quotes and swaps run, which is why adding one has a \
      write switch of its own.",
   schema: r#"{"type": "object", "properties": {"chain": {"type": "integer", "description": "The chain the pool runs on; a chain id from networks_list."}, "version": {"type": "string",
      "enum": ["v2", "v3", "v4"], "description": "Which pool standard."}, "token_a":
      {"type": "string", "description": "One token's address; the zero address is native ETH."},
      "token_b": {"type": "string", "description": "The other token's address."}, "fee":
      {"type": "integer", "description": "A v3 or v4 pool's fee tier in hundredths of a bip: 3000 is 0.3 percent. A v2 pool has none."}, "tick_spacing": {"type": "integer", "description": "A v4 pool's tick spacing; leave it out for the spacing its fee tier has."}, "hooks":
      {"type": "string", "description": "A v4 pool's hooks contract; leave it out for a pool with none."}}, "required": ["chain", "version", "token_a", "token_b"],
      "additionalProperties": false}"#,
   annotations: Annotations {
      read_only: false,
      destructive: false,
      // Looking the pair up reads the chain, which is what this hint means.
      open_world: true,
   },
   capability: Some(CAPABILITY),
   call: add,
};

fn list(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let chain = chain_argument(&arguments)?;
      let pools = call.ctx.read(|ctx| read::pools(ctx, chain));
      Ok(ToolOutput::new(pools))
   })
}

fn add(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let chain = chain_argument(&arguments)?.ok_or_else(|| {
         ToolError::validation("chain is required: pools_list names the chains that have any")
      })?;

      let version = match super::string_argument(&arguments, "version")? {
         "v2" => PoolVersion::V2,
         "v3" => PoolVersion::V3,
         "v4" => PoolVersion::V4,
         other => {
            return Err(ToolError::validation(format!(
               "version {other} is not one of v2, v3, v4"
            )));
         }
      };

      let token_a = address_argument(&arguments, "token_a")?;
      let token_b = address_argument(&arguments, "token_b")?;

      if token_a == token_b {
         return Err(ToolError::validation(
            "token_a and token_b must be different",
         ));
      }

      // A field the version does not have is refused rather than ignored: a caller that sent a fee with
      // a v2 pair believes it is adding something else.
      let mut fee = 0;
      let mut tick_spacing = 0;
      let mut hooks = Address::ZERO;

      if version == PoolVersion::V2 {
         for field in ["fee", "tick_spacing", "hooks"] {
            if arguments.get(field).is_some_and(|value| !value.is_null()) {
               return Err(ToolError::validation(format!(
                  "{field} does not describe a v2 pool: that pool is the pair alone"
               )));
            }
         }
      } else {
         fee = pool_fee(&arguments)?;

         if version == PoolVersion::V4 {
            tick_spacing = pool_tick_spacing(&arguments)?;

            if let Some(text) = arguments.get("hooks").and_then(Value::as_str) {
               hooks = Address::from_str(text.trim())
                  .map_err(|_| ToolError::validation(format!("hooks is not an address: {text}")))?;
            }
         } else {
            for field in ["tick_spacing", "hooks"] {
               if arguments.get(field).is_some_and(|value| !value.is_null()) {
                  return Err(ToolError::validation(format!(
                     "{field} only describes a v4 pool"
                  )));
               }
            }
         }
      }

      gate_write(
         call,
         "pools_add",
         &intent(),
         &format!("Let the agent start tracking a pool on chain {chain}?"),
      )
      .await?;

      let request = PoolRequest {
         chain,
         version,
         token_a,
         token_b,
         fee,
         tick_spacing,
         hooks,
      };

      let row = call.ops.add_pool(request).await.map_err(pool_error)?;

      record_allowed(call, "pools_add", CAPABILITY);

      ui::announce(&format!("Agent added a pool on chain {chain}"));

      Ok(ToolOutput::new(json!({"pool": row})))
   })
}

/// The fee a v3 or v4 pool is looked up at.
fn pool_fee(arguments: &Value) -> Result<u32, ToolError> {
   let fee = arguments.get("fee").and_then(Value::as_u64).ok_or_else(|| {
      ToolError::validation(
         "fee is required: the pool's fee tier in hundredths of a bip (3000 is 0.3%)",
      )
   })?;

   // The upper bound a V4 fee has: zero is legal and so is the dynamic-fee flag.
   if fee > 0x00FF_FFFF {
      return Err(ToolError::validation(format!(
         "fee must be between 0 and 16777215, not {fee}"
      )));
   }

   Ok(fee as u32)
}

/// A v4 pool's tick spacing, where zero asks for the one its fee tier has.
fn pool_tick_spacing(arguments: &Value) -> Result<i32, ToolError> {
   match arguments.get("tick_spacing") {
      None | Some(Value::Null) => Ok(0),
      Some(value) => {
         let spacing = value
            .as_i64()
            .ok_or_else(|| ToolError::validation("tick_spacing must be a whole number"))?;

         i32::try_from(spacing)
            .map_err(|_| ToolError::validation(format!("tick_spacing is out of range: {spacing}")))
      }
   }
}

/// The refusal a failed pool lookup becomes.
///
/// "The chain has no such pool" is the caller's to fix. "A node could not be reached" says nothing about
/// the pool, so it must not be reported as one that does not exist.
fn pool_error(error: PoolError) -> ToolError {
   match error {
      PoolError::Refused(message) => ToolError::new(ErrorCode::Unsupported, message)
         .with_hint("Nothing was stored. pools_list shows what is tracked."),
      PoolError::Network(message) => ToolError::new(ErrorCode::Network, message)
         .with_hint("Nothing was stored. Retry once a node answers."),
   }
}

/// The intent one call of this tool is decided against.
fn intent() -> Intent {
   Intent::GlobalWrite {
      capability: CAPABILITY,
   }
}

/// One address argument, by name.
fn address_argument(arguments: &Value, key: &str) -> Result<Address, ToolError> {
   let text = super::string_argument(arguments, key)?;

   Address::from_str(text)
      .map_err(|_| ToolError::validation(format!("{key} is not an address: {text}")))
}
