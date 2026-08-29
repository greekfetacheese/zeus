use crate::core::ZeusCtx;
use crate::utils::TimeStamp;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::permit,
   alloy_primitives::{Address, Log, U256},
   currency::Currency,
   utils::NumericValue,
};

/// Represents both the Permit & Approval event of the Permit2 contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitParams {
   pub event_name: String,
   pub chain: u64,
   pub owner: Address,
   pub token: Currency,
   pub spender: Address,
   pub amount: NumericValue,
   pub amount_usd: Option<NumericValue>,
   pub expiration: TimeStamp,
}

impl PermitParams {
   pub async fn from_log(ctx: ZeusCtx, chain: u64, log: &Log) -> Result<Self, anyhow::Error> {
      let mut decoded_permit = None;
      let mut decoded_approval = None;
      let mut name = String::new();

      if let Ok(decoded_log) = permit::decode_permit_log(log) {
         decoded_permit = Some(decoded_log);
         name = "Permit".to_string();
      }

      if let Ok(decoded_log) = permit::decode_approval_log(log) {
         decoded_approval = Some(decoded_log);
         name = "Permit Approval".to_string();
      }

      if decoded_permit.is_none() && decoded_approval.is_none() {
         return Err(anyhow!("Failed to decode log"));
      }

      let token_addr = match &decoded_permit {
         Some(decoded) => decoded.token,
         None => decoded_approval.as_ref().unwrap().token,
      };

      let spender = match &decoded_permit {
         Some(decoded) => decoded.spender,
         None => decoded_approval.as_ref().unwrap().spender,
      };

      let owner = match &decoded_permit {
         Some(decoded) => decoded.owner,
         None => decoded_approval.as_ref().unwrap().owner,
      };

      let amount = match &decoded_permit {
         Some(decoded) => U256::from(decoded.amount),
         None => U256::from(decoded_approval.as_ref().unwrap().amount),
      };

      let expiration = match &decoded_permit {
         Some(decoded) => decoded.expiration,
         None => decoded_approval.as_ref().unwrap().expiration,
      };

      let erc_token = ctx.get_token(chain, token_addr).await?;

      let token = Currency::from(erc_token.clone());
      let amount = NumericValue::format_wei(amount, token.decimals());

      if amount.is_zero() {
         name = "Revoke Permit".to_string();
      }

      let amount_usd =
         ctx.get_currency_value_for_amount(amount.f64(), &Currency::from(token.clone()));

      let actual_amount_usd = if amount_usd.is_zero() {
         let price_manager = ctx.price_manager();
         let pool_manager = ctx.pool_manager();

         match price_manager
            .calculate_prices(ctx.clone(), chain, pool_manager, vec![erc_token])
            .await
         {
            Ok(_) => {}
            Err(e) => {
               tracing::error!(
                  "Error calculating price for token {}: {:?}",
                  token.symbol(),
                  e
               );
            }
         }

         ctx.get_currency_value_for_amount(amount.f64(), &token)
      } else {
         amount_usd
      };

      let expiration: u64 = expiration.to_string().parse()?;
      let exp_timestamp = TimeStamp::Seconds(expiration);

      Ok(Self {
         event_name: name,
         chain,
         owner,
         token,
         spender,
         amount,
         amount_usd: Some(actual_amount_usd),
         expiration: exp_timestamp,
      })
   }
}
