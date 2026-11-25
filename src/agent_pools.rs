//! The add-pool flow, as the agent port's answer.
//!
//! Looking a pair up is the app's own work: the chain is read through the client manager, the pool
//! standards are the GUI's own types, and the V4 lookup is the state view's. None of that can move into
//! the edge, so the flow stays here and the port carries it.
//!
//! What comes back is the row `read::pool_row` builds, which is the row `pools_list` shows: a pool an
//! agent added is the same pool the user sees.

use crate::core::agent::read;
use crate::core::{PoolError, ZeusCtx};
use crate::gui::ui::dapps::uniswap::pool::{AddPoolRequest, build_pool, store_pool};
use serde_json::Value;
use zeus_mcp::{PoolRequest, PoolVersion};

/// Look one pool up on chain and track it, answering the row the pool list shows.
pub async fn add(ctx: ZeusCtx, request: PoolRequest) -> Result<Value, PoolError> {
   let chain = request.chain;

   let asked = match request.version {
      PoolVersion::V2 => AddPoolRequest::V2 {
         token_a: request.token_a,
         token_b: request.token_b,
      },
      PoolVersion::V3 => AddPoolRequest::V3 {
         token_a: request.token_a,
         token_b: request.token_b,
         fee: request.fee,
      },
      PoolVersion::V4 => AddPoolRequest::V4 {
         token_a: request.token_a,
         token_b: request.token_b,
         fee: request.fee,
         tick_spacing: request.tick_spacing,
         hooks: request.hooks,
      },
   };

   let pool = build_pool(ctx.clone(), chain, asked).await?;

   // The row is taken before the pool is stored: storing consumes it.
   let row = read::pool_row(&pool);

   store_pool(ctx, chain, pool).await?;

   Ok(row)
}
