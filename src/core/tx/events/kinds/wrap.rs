use crate::core::ZeusCtx;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::weth9,
   alloy_primitives::{Address, Bytes, Log, U256},
   currency::{Currency, NativeCurrency},
   utils::NumericValue,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrapETHParams {
   pub chain: u64,
   pub recipient: Address,
   pub eth_wrapped: NumericValue,
   pub eth_wrapped_usd: Option<NumericValue>,
}

impl WrapETHParams {
   pub fn from_log(ctx: ZeusCtx, chain: u64, log: &Log) -> Result<Self, anyhow::Error> {
      let mut decoded = None;
      if let Ok(decoded_log) = weth9::decode_deposit_log(log) {
         decoded = Some(decoded_log);
      }

      let decoded = decoded.ok_or(anyhow!("Failed to decode deposit log"))?;

      let currency = Currency::from(NativeCurrency::from(chain));
      let eth_wrapped = NumericValue::format_wei(decoded.wad, currency.decimals());
      let eth_wrapped_usd = ctx.get_currency_value_for_amount(eth_wrapped.f64(), &currency);

      Ok(Self {
         chain,
         recipient: decoded.dst,
         eth_wrapped,
         eth_wrapped_usd: Some(eth_wrapped_usd),
      })
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnwrapWETHParams {
   pub chain: u64,
   pub src: Address,
   pub weth_unwrapped: NumericValue,
   pub weth_unwrapped_usd: Option<NumericValue>,
   pub eth_received: NumericValue,
   pub eth_received_usd: Option<NumericValue>,
}

impl UnwrapWETHParams {
   pub fn new(
      ctx: ZeusCtx,
      chain: u64,
      call_data: Bytes,
      value: U256,
      logs: Vec<Log>,
   ) -> Result<Self, anyhow::Error> {
      let selector = call_data.get(0..4).unwrap_or_default();
      if selector != weth9::withdraw_selector() {
         return Err(anyhow!("Call is not a WETH withdraw"));
      }

      let mut decoded = None;
      for log in &logs {
         if let Ok(decoded_log) = weth9::decode_withdraw_log(log) {
            decoded = Some(decoded_log);
            break;
         }
      }

      if decoded.is_none() {
         return Err(anyhow!("Failed to decode withdraw log"));
      }

      let decoded = decoded.unwrap();

      let currency = Currency::from(NativeCurrency::from_chain_id(chain).unwrap());
      let weth_unwrapped = NumericValue::format_wei(value, currency.decimals());
      let weth_unwrapped_usd = ctx.get_currency_value_for_amount(weth_unwrapped.f64(), &currency);
      let eth_received = NumericValue::format_wei(decoded.wad, currency.decimals());
      let eth_received_usd = ctx.get_currency_value_for_amount(eth_received.f64(), &currency);

      Ok(Self {
         chain,
         src: decoded.src,
         weth_unwrapped,
         weth_unwrapped_usd: Some(weth_unwrapped_usd),
         eth_received,
         eth_received_usd: Some(eth_received_usd),
      })
   }

   pub fn from_log(ctx: ZeusCtx, chain: u64, log: &Log) -> Result<Self, anyhow::Error> {
      let mut decoded = None;
      if let Ok(decoded_log) = weth9::decode_withdraw_log(log) {
         decoded = Some(decoded_log);
      }

      if decoded.is_none() {
         return Err(anyhow!("Failed to decode withdraw log"));
      }

      let decoded = decoded.unwrap();

      let currency = Currency::from(NativeCurrency::from_chain_id(chain).unwrap());
      let weth_unwrapped = NumericValue::format_wei(decoded.wad, currency.decimals());
      let weth_unwrapped_usd = ctx.get_currency_value_for_amount(weth_unwrapped.f64(), &currency);
      let eth_received = NumericValue::format_wei(decoded.wad, currency.decimals());
      let eth_received_usd = ctx.get_currency_value_for_amount(eth_received.f64(), &currency);

      Ok(Self {
         chain,
         src: decoded.src,
         weth_unwrapped,
         weth_unwrapped_usd: Some(weth_unwrapped_usd),
         eth_received,
         eth_received_usd: Some(eth_received_usd),
      })
   }
}
