//! The Uniswap swap, and the quote that prices it.
//!
//! A quote is a read: it prices the route the pools hold now, answers it, and never signs anything. It is
//! also where the USD figures in an answer come from, since no grant limits a swap by value. The swap itself
//! runs through the money pipeline — the grant, the asset envelope, the wallet lock, the record — and it
//! re-prices before it sends, so what it signs is the route at the moment of signing rather than the one
//! this caller happened to read.

use super::tx::{decimals_argument, wei_amount};
use super::{
   Call, ToolFuture, ToolOutput, ToolSpec, action, deadline_argument, dry_run_argument, exec_error,
   optional_address, priority_argument, required_address, required_chain, slippage_argument,
   string_argument, wallet_argument,
};
use crate::error::ToolError;
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use zeus_core::agent::exec::{self, ExecCall, QuoteRequest, SwapQuote, SwapRequest};
use zeus_core::agent::read;
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::currency::{Currency, NativeCurrency};
use zeus_eth::utils::NumericValue;

pub const QUOTE: ToolSpec = ToolSpec {
   name: "quote_swap",
   title: "Price a swap",
   description: "What a Uniswap swap would produce right now: the route it would take, what it would produce, and the least it would accept at the slippage the call names. Prices the same pools the swap page does and never signs or sends anything; call tx_swap to make it happen.",
   schema: QUOTE_SCHEMA,
   annotations: Annotations::READ,
   capability: Some("wallet.read.portfolio"),
   call: quote,
};

pub const SWAP: ToolSpec = ToolSpec {
   name: "tx_swap",
   title: "Swap on Uniswap",
   description: "Swap one token for another through the Uniswap Universal Router, from a granted wallet. The token that leaves has to be in that wallet's allowlist, and the slippage may not exceed the grant's own cap. The route is priced again when the swap runs, so what quote_swap answered is advice rather than a promise. There is no dry run: the transaction is built from that route inside the flow.",
   schema: SWAP_SCHEMA,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some("wallet.write.swap"),
   call: swap,
};

const QUOTE_SCHEMA: &str = r#"{"type": "object", "properties": {
   "wallet": {"type": "string", "description": "The wallet that would pay, as wallet_list shows it."},
   "chain": {"type": "integer", "description": "The chain to trade on."},
   "token_in": {"type": "string", "description": "The token to sell. Omit it for the chain's own coin."},
   "token_out": {"type": "string", "description": "The token to buy."},
   "decimals": {"type": "integer", "description": "The input token's decimals, as wallet_get reports them."},
   "amount": {"type": "string", "description": "How much to sell, in whole units, as a decimal string such as 1.25. Never a JSON number."},
   "slippage_bps": {"type": "integer", "description": "How far the output may fall below the quote, in basis points: 50 is 0.5%. A swap is capped by the grant; a quote is not."}
}, "required": ["wallet", "chain", "token_out", "amount"], "additionalProperties": false}"#;

const SWAP_SCHEMA: &str = r#"{"type": "object", "properties": {
   "wallet": {"type": "string", "description": "The wallet to trade from, as wallet_list shows it."},
   "chain": {"type": "integer", "description": "The chain to trade on."},
   "token_in": {"type": "string", "description": "The token to sell. Omit it for the chain's own coin."},
   "token_out": {"type": "string", "description": "The token to buy."},
   "decimals": {"type": "integer", "description": "The input token's decimals, as wallet_get reports them."},
   "amount": {"type": "string", "description": "How much to sell, in whole units, as a decimal string such as 1.25. Never a JSON number."},
   "slippage_bps": {"type": "integer", "description": "How far the output may fall below the quote, in basis points: 50 is 0.5%. The grant caps this."},
   "deadline": {"type": "integer", "description": "How long the swap stays valid, in minutes: 1 to 60, 5 by default."},
   "dry_run": {"type": "boolean", "description": "Refused for a swap: the transaction is built from the route inside the flow, and quote_swap is the read-only answer."},
   "priority": {"type": "string", "enum": ["normal", "fast", "very_fast"], "description": "The fee tier; fast by default."},
   "action_id": {"type": "string", "description": "A client-chosen id. Repeating a call with the same id and arguments replays the first answer instead of swapping twice."}
}, "required": ["wallet", "chain", "token_out", "amount"], "additionalProperties": false}"#;

fn quote(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let request = quote_request(&arguments)?;
      let chain = request.chain;
      let wallet = request.from;
      let decimals_in = request.decimals_in;
      let token_in = request.currency_in;
      let token_out = request.currency_out;
      let slippage_bps = request.slippage_bps;
      let amount_in = NumericValue::format_wei(request.amount_in, decimals_in);

      let priced = call
         .ops
         .quote(request)
         .await
         .map_err(|error| exec_error(error, "wallet.read.portfolio"))?;

      Ok(ToolOutput::new(quote_row(
         call,
         chain,
         wallet,
         token_in,
         token_out,
         decimals_in,
         &amount_in,
         slippage_bps,
         &priced,
      )))
   })
}

fn swap(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let request = swap_request(&arguments)?;
      let wanted = ExecCall::Swap(request);
      let capability = wanted.capability();

      if dry_run_argument(&arguments)? {
         let value = exec::preview(call.ctx.clone(), &call.profile, &wanted)
            .await
            .map_err(|error| exec_error(error, capability))?;

         return Ok(action::preview(value, &arguments));
      }

      let held = call.clone();

      action::run(call, &arguments, async move {
         Ok(super::tx::perform(&held, "tx_swap", capability, wanted).await?)
      })
      .await
   })
}

/// One quote's answer: the amounts, and the route a caller has to be able to check.
fn quote_row(
   call: &Call,
   chain: u64,
   wallet: Address,
   token_in: Address,
   token_out: Address,
   decimals_in: u8,
   amount_in: &NumericValue,
   slippage_bps: u16,
   quote: &SwapQuote,
) -> Value {
   let out = NumericValue::format_wei(quote.amount_out, quote.decimals_out);
   let floor = NumericValue::format_wei(
      floor_of(quote.amount_out, slippage_bps),
      quote.decimals_out,
   );

   let route: Vec<Value> = quote
      .steps
      .iter()
      .map(|step| {
         json!({
             "pool": format!("{:#x}", step.pool),
             "version": step.version,
             "fee": step.fee,
             "tick_spacing": step.tick_spacing,
             "hooks": format!("{:#x}", step.hooks),
             "amount_in": step.amount_in.to_string(),
             "amount_out": step.amount_out.to_string(),
         })
      })
      .collect();

   json!({
       "chain": chain,
       "wrapped": quote.wrapped,
       "slippage_bps": slippage_bps,
       "amount_in": amount_row(call, chain, wallet, token_in, decimals_in, amount_in),
       "amount_out": amount_row(call, chain, wallet, token_out, quote.decimals_out, &out),
       "min_received": amount_row(call, chain, wallet, token_out, quote.decimals_out, &floor),
       "route": route,
   })
}

/// The least a swap would accept: the quote less the slippage the caller allows.
fn floor_of(amount_out: U256, slippage_bps: u16) -> U256 {
   let keep = U256::from(10_000u64.saturating_sub(u64::from(slippage_bps)));

   amount_out.saturating_mul(keep) / U256::from(10_000u64)
}

/// One amount, with its USD value when the app knows a price for it.
fn amount_row(
   call: &Call,
   chain: u64,
   wallet: Address,
   token: Address,
   decimals: u8,
   amount: &NumericValue,
) -> Value {
   let (symbol, usd) = described(call, chain, wallet, token, amount);

   read::amount(amount, decimals, &symbol, usd.as_ref())
}

/// A token's symbol and what an amount of it is worth, from what the app already holds.
fn described(
   call: &Call,
   chain: u64,
   wallet: Address,
   token: Address,
   amount: &NumericValue,
) -> (String, Option<NumericValue>) {
   if token.is_zero() {
      let native = Currency::from(NativeCurrency::from(chain));
      let usd = call.ctx.get_currency_value_for_amount(amount.f64(), &native);

      return (read::native_symbol(chain), Some(usd));
   }

   let known = call
      .ctx
      .get_portfolio(chain, wallet)
      .tokens()
      .iter()
      .find(|held| held.address == token)
      .cloned();

   match known {
      Some(held) => {
         let usd = call.ctx.get_token_value_for_amount(amount.f64(), &held);

         (held.symbol.to_string(), Some(usd))
      }
      // A token the wallet has never held is named by its address: guessing a symbol would be worse.
      None => (short(token), None),
   }
}

/// An address, short enough to say out loud.
fn short(address: Address) -> String {
   let text = format!("{address:#x}");
   let (head, tail) = text.split_at(8);

   format!("{head}...{}", &tail[tail.len() - 4..])
}

/// The request a quote describes.
fn quote_request(arguments: &Value) -> Result<QuoteRequest, ToolError> {
   let token_in = optional_address(arguments, "token_in")?;
   let decimals = decimals_argument(arguments, token_in.is_some())?;

   Ok(QuoteRequest {
      chain: required_chain(arguments)?,
      from: wallet_argument(arguments)?,
      currency_in: token_in.unwrap_or(Address::ZERO),
      currency_out: required_address(arguments, "token_out")?,
      amount_in: wei_amount(string_argument(arguments, "amount")?, decimals)?,
      decimals_in: decimals,
      slippage_bps: slippage_argument(arguments)?,
      priority: priority_argument(arguments)?,
   })
}

/// The request a swap describes: the quote's fields, plus how long it may stay valid.
fn swap_request(arguments: &Value) -> Result<SwapRequest, ToolError> {
   let quote = quote_request(arguments)?;

   Ok(SwapRequest {
      chain: quote.chain,
      from: quote.from,
      currency_in: quote.currency_in,
      currency_out: quote.currency_out,
      amount_in: quote.amount_in,
      decimals_in: quote.decimals_in,
      slippage_bps: quote.slippage_bps,
      deadline: deadline_argument(arguments)?,
      priority: quote.priority,
   })
}
