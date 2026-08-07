use crate::core::{ZeusCtx, types::Dapp};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use zeus_eth::{
   abi::protocols::across,
   alloy_primitives::{Address, Log},
   currency::{Currency, NativeCurrency},
   utils::NumericValue,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeParams {
   pub dapp: Dapp,
   pub origin_chain: u64,
   pub destination_chain: u64,
   pub input_currency: Currency,
   pub output_currency: Currency,
   pub amount: NumericValue,
   /// USD value at the time of the tx
   pub amount_usd: Option<NumericValue>,
   pub received: NumericValue,
   /// USD value at the time of the tx
   pub received_usd: Option<NumericValue>,
   pub depositor: Address,
   pub recipient: Address,
}

impl Default for BridgeParams {
   fn default() -> Self {
      Self {
         dapp: Dapp::Across,
         origin_chain: 1,
         destination_chain: 10,
         input_currency: Currency::default(),
         output_currency: Currency::default(),
         amount: NumericValue::default(),
         amount_usd: None,
         received: NumericValue::default(),
         received_usd: None,
         depositor: Address::default(),
         recipient: Address::default(),
      }
   }
}

impl BridgeParams {
   pub async fn from_log(
      ctx: ZeusCtx,
      origin_chain: u64,
      log: &Log,
   ) -> Result<Self, anyhow::Error> {
      let mut decode_log = None;
      if let Ok(decoded) = across::decode_funds_deposited_log(log) {
         decode_log = Some(decoded);
      }

      if decode_log.is_none() {
         return Err(anyhow!("Failed to decode funds deposited log"));
      }

      let decoded = decode_log.unwrap();
      let dest_chain = u64::from_str(&decoded.destination_chain_id.to_string())?;

      let input_token = ctx.get_token(origin_chain, decoded.input_token).await?;
      let output_token = ctx.get_token(dest_chain, decoded.output_token).await?;

      // Assuming depositor and recipient are EOAs
      let show_native = input_token.is_native_wrapped() && output_token.is_native_wrapped();

      let input_currency = if show_native {
         Currency::from(NativeCurrency::from(origin_chain))
      } else {
         Currency::from(input_token.clone())
      };

      let output_currency = if show_native {
         Currency::from(NativeCurrency::from(dest_chain))
      } else {
         Currency::from(output_token.clone())
      };

      let amount = NumericValue::format_wei(decoded.input_amount, input_token.decimals);
      let amount_usd =
         ctx.get_currency_value_for_amount(amount.f64(), &Currency::from(input_token.clone()));
      let received = NumericValue::format_wei(decoded.output_amount, output_token.decimals);
      let received_usd = ctx.get_currency_value_for_amount(
         received.f64(),
         &Currency::from(output_token.clone()),
      );

      let params = BridgeParams {
         dapp: Dapp::Across,
         origin_chain,
         destination_chain: dest_chain,
         input_currency,
         output_currency,
         amount,
         amount_usd: Some(amount_usd),
         received,
         received_usd: Some(received_usd),
         depositor: decoded.depositor,
         recipient: decoded.recipient,
      };
      Ok(params)
   }
}
