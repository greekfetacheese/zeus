//! The Uniswap swap and its quote, as the agent pipeline calls them.
//!
//! Both price the same route — the pools the app tracks for the chain, read from the chain first and
//! filtered by the page's own knobs, then the local quoter — and a swap re-prices before it sends, so the
//! route it signs is the one the pools hold at that moment. A quote is therefore advice and never a
//! promise, which is also why nothing about a route crosses the port: a caller names what it wants to
//! trade, not which pools to trade it in.

use crate::core::agent::exec::{ExecError, QuoteRequest, QuoteStep, SwapQuote, SwapRequest};
use crate::core::{AgentEnv, TransactionRich, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::gui::ui::dapps::uniswap::swap::{
   SwapUi, get_relevant_pools, swap_via_ur, unwrap_weth, wrap_eth,
};
use crate::utils::swap_quoter::{Quote, get_quote, get_quote_with_split_routing};
use std::sync::atomic::AtomicBool;
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::amm::uniswap::{AnyUniswapPool, UniswapPool};
use zeus_eth::currency::{Currency, ERC20Token, NativeCurrency};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

/// The route knobs, as the swap page holds them.
///
/// They are in-memory only — a new launch is a new set — so a call that names none of them gets the route
/// the page would build right now, which is the only definition of "what a human could have got".
struct Knobs {
   v2: bool,
   v3: bool,
   v4: bool,
   split: bool,
   max_hops: usize,
   max_split_routes: usize,
   mev_protect: bool,
}

impl Knobs {
   fn from_page() -> Self {
      SHARED_GUI.read(|gui| {
         let settings = &gui.uniswap.settings;

         Self {
            v2: settings.swap_on_v2,
            v3: settings.swap_on_v3,
            v4: settings.swap_on_v4,
            split: settings.split_routing_enabled,
            max_hops: settings.max_hops,
            max_split_routes: settings.max_split_routes,
            mev_protect: settings.mev_protect,
         }
      })
   }
}

/// What one quote request would get.
pub async fn quote(ctx: ZeusCtx, request: QuoteRequest) -> Result<SwapQuote, ExecError> {
   let chain = super::agent_tx::chain(request.chain)?;
   let currency_in = currency(&ctx, chain, request.from, request.currency_in).await?;
   let currency_out = currency(&ctx, chain, request.from, request.currency_out).await?;
   let amount_in = amount_for(
      &currency_in,
      request.amount_in,
      request.decimals_in,
   )?;

   // Nothing to price: the coin's side of it is the same one, so the answer is what was asked.
   if wraps(&currency_in, &currency_out).is_some() {
      return Ok(SwapQuote {
         amount_out: request.amount_in,
         decimals_out: currency_out.decimals(),
         wrapped: true,
         steps: Vec::new(),
      });
   }

   let quote = priced(
      &ctx,
      &currency_in,
      &currency_out,
      amount_in,
      &Knobs::from_page(),
   )
   .await?;

   if quote.amount_out.is_zero() {
      return Err(ExecError::Failed(
         "no route was found for this pair at this size".to_owned(),
      ));
   }

   let steps = quote
      .swap_steps
      .iter()
      .map(|step| QuoteStep {
         pool: step.pool.address(),
         version: version_of(&step.pool),
         fee: step.pool.fee().fee(),
         tick_spacing: step.pool.tick_spacing_i32(),
         hooks: step.pool.hooks(),
         amount_in: step.amount_in.wei(),
         amount_out: step.amount_out.wei(),
      })
      .collect();

   Ok(SwapQuote {
      amount_out: quote.amount_out.wei(),
      decimals_out: currency_out.decimals(),
      wrapped: false,
      steps,
   })
}

/// Make the swap.
pub async fn swap(
   ctx: ZeusCtx,
   env: AgentEnv,
   request: SwapRequest,
) -> Result<TransactionRich, ExecError> {
   let chain = super::agent_tx::chain(request.chain)?;
   let currency_in = currency(&ctx, chain, request.from, request.currency_in).await?;
   let currency_out = currency(&ctx, chain, request.from, request.currency_out).await?;
   let amount_in = amount_for(
      &currency_in,
      request.amount_in,
      request.decimals_in,
   )?;
   let knobs = Knobs::from_page();

   // Wrapping is not a route: the page does it with the router's own two calls, and so does this.
   if let Some(pair) = wraps(&currency_in, &currency_out) {
      return match pair {
         Wrapped::Eth => wrap_eth(ctx, env, request.from, chain, amount_in)
            .await
            .map_err(super::agent_tx::classify),
         Wrapped::Weth => unwrap_weth(ctx, env, request.from, chain, amount_in)
            .await
            .map_err(super::agent_tx::classify),
      };
   }

   let quote = priced(
      &ctx,
      &currency_in,
      &currency_out,
      amount_in.clone(),
      &knobs,
   )
   .await?;

   if quote.amount_out.is_zero() {
      return Err(ExecError::Failed(
         "no route was found for this pair at this size".to_owned(),
      ));
   }

   // The basis points the grant allowed are a percentage here, the unit the quote and the router use.
   let slippage = f64::from(request.slippage_bps) / 100.0;

   swap_via_ur(
      ctx,
      env,
      chain,
      slippage,
      knobs.mev_protect,
      request.deadline,
      request.from,
      amount_in,
      currency_in,
      currency_out,
      // The route this call priced, which is what the agent was answered with a moment ago.
      quote.amount_out.clone(),
      quote.swap_steps,
   )
   .await
   .map_err(super::agent_tx::classify)
}

/// Which way a pair wraps, when it is one pair of the same coin.
enum Wrapped {
   Eth,
   Weth,
}

fn wraps(currency_in: &Currency, currency_out: &Currency) -> Option<Wrapped> {
   if currency_in.is_native() && currency_out.is_native_wrapped() {
      return Some(Wrapped::Eth);
   }

   if currency_in.is_native_wrapped() && currency_out.is_native() {
      return Some(Wrapped::Weth);
   }

   None
}

/// The currency an address names: the chain's own coin for the zero address.
async fn currency(
   ctx: &ZeusCtx,
   chain: ChainId,
   owner: Address,
   address: Address,
) -> Result<Currency, ExecError> {
   if address.is_zero() {
      return Ok(Currency::from(NativeCurrency::from(chain.id())));
   }

   Ok(Currency::from(
      super::agent_tx::erc20(ctx, chain, owner, address).await?,
   ))
}

/// The amount, checked against the token it is denominated in.
///
/// The scale cannot be guessed — one digit wrong is three orders of magnitude — so a request that states
/// the wrong one is refused rather than rescaled.
fn amount_for(currency: &Currency, amount: U256, decimals: u8) -> Result<NumericValue, ExecError> {
   if currency.decimals() != decimals {
      return Err(ExecError::Invalid(format!(
         "{} has {} decimals, not {decimals}: the amount would be scaled wrong",
         currency.symbol(),
         currency.decimals()
      )));
   }

   if amount.is_zero() {
      return Err(ExecError::Invalid(
         "amount is zero: there is nothing to swap".to_owned(),
      ));
   }

   Ok(NumericValue::format_wei(amount, decimals))
}

/// Price one route from the pools the app holds for the chain.
async fn priced(
   ctx: &ZeusCtx,
   currency_in: &Currency,
   currency_out: &Currency,
   amount_in: NumericValue,
   knobs: &Knobs,
) -> Result<Quote, ExecError> {
   let chain = currency_in.chain_id();

   let pools = get_relevant_pools(
      ctx.clone(),
      knobs.v2,
      knobs.v3,
      knobs.v4,
      currency_in,
      currency_out,
   );

   // The page refreshes the pools it is about to quote, and so does this: a quote is a claim about the
   // reserves as they are now, not as they were when the app last looked.
   let pools = ctx
      .pool_manager()
      .update_state_for_pools(ctx.clone(), chain, pools)
      .await
      .map_err(|error| {
         ExecError::Failed(format!(
            "the pool state could not be read: {error}"
         ))
      })?;

   let mut liquid: Vec<AnyUniswapPool> = pools
      .into_iter()
      .filter(|pool| ctx.pool_has_sufficient_liquidity(pool).unwrap_or(false))
      .collect();

   // A hook pool prices its own cut, and one that only the chain can price is quoted for this amount.
   SwapUi::prepare_hook_pools(
      ctx.clone(),
      &mut liquid,
      currency_in,
      amount_in.wei(),
      &AtomicBool::new(false),
   )
   .await;

   let base_fee = ctx.get_base_fee(chain).unwrap_or_default().next;
   let priority_fee = ctx.get_priority_fee(chain).unwrap_or_default();
   let eth_price = ctx.get_token_price(&ERC20Token::wrapped_native_token(chain));
   let currency_out_price = ctx.get_currency_price(currency_out);

   let currency_in = currency_in.clone();
   let currency_out = currency_out.clone();
   let max_hops = knobs.max_hops;
   let split = knobs.split;
   let max_split_routes = knobs.max_split_routes;

   crate::runtime::rt()
      .spawn_blocking(move || {
         if split {
            get_quote_with_split_routing(
               amount_in,
               currency_in,
               currency_out,
               liquid,
               eth_price,
               currency_out_price,
               base_fee,
               priority_fee.wei(),
               max_hops,
               max_split_routes,
            )
         } else {
            get_quote(
               amount_in,
               currency_in,
               currency_out,
               liquid,
               eth_price,
               currency_out_price,
               base_fee,
               priority_fee.wei(),
               max_hops,
            )
         }
      })
      .await
      .map_err(|error| ExecError::Failed(format!("the quoter did not run: {error}")))
}

/// Which pool version a hop used.
fn version_of(pool: &AnyUniswapPool) -> u8 {
   if pool.dex_kind().is_v2() {
      2
   } else if pool.dex_kind().is_v3() {
      3
   } else {
      4
   }
}
