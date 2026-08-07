use crate::core::ZeusCtx;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::erc20,
   alloy_primitives::{Address, Log},
   currency::{Currency, ERC20Token},
   utils::NumericValue,
};

#[derive(Debug, Clone, Serialize)]
pub struct TokenApproveParams {
   pub token: ERC20Token,
   pub amount: NumericValue,
   pub amount_usd: Option<NumericValue>,
   pub owner: Address,
   pub spender: Address,
}

/// Untagged helper used to accept both the current flat format and the legacy
/// `Vec`-based format when deserializing `TokenApproveParams`.
#[derive(Deserialize)]
#[serde(untagged)]
enum TokenApproveParamsSerde {
   New {
      token: ERC20Token,
      amount: NumericValue,
      amount_usd: Option<NumericValue>,
      owner: Address,
      spender: Address,
   },
   Old {
      token: Vec<ERC20Token>,
      amount: Vec<NumericValue>,
      amount_usd: Vec<Option<NumericValue>>,
      owner: Address,
      spender: Address,
   },
}

impl<'de> Deserialize<'de> for TokenApproveParams {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      use serde::de::Error;

      match TokenApproveParamsSerde::deserialize(deserializer)? {
         TokenApproveParamsSerde::New {
            token,
            amount,
            amount_usd,
            owner,
            spender,
         } => Ok(Self {
            token,
            amount,
            amount_usd,
            owner,
            spender,
         }),
         // Migrate from the legacy format by taking the first approved token.
         TokenApproveParamsSerde::Old {
            token,
            amount,
            amount_usd,
            owner,
            spender,
         } => {
            let token = token
               .into_iter()
               .next()
               .ok_or_else(|| D::Error::custom("token list is empty"))?;
            let amount = amount
               .into_iter()
               .next()
               .ok_or_else(|| D::Error::custom("amount list is empty"))?;
            let amount_usd = amount_usd
               .into_iter()
               .next()
               .ok_or_else(|| D::Error::custom("amount_usd list is empty"))?;

            Ok(Self {
               token,
               amount,
               amount_usd,
               owner,
               spender,
            })
         }
      }
   }
}

impl TokenApproveParams {
   pub async fn from_log(ctx: ZeusCtx, chain: u64, log: &Log) -> Result<Self, anyhow::Error> {
      let mut decoded = None;
      let mut token_addr = None;
      if let Ok(decoded_log) = erc20::decode_approve_log(log) {
         decoded = Some(decoded_log);
         token_addr = Some(log.address);
      }

      if decoded.is_none() {
         return Err(anyhow!("Failed to decode approve log"));
      }

      let decoded = decoded.unwrap();
      let token_addr = token_addr.unwrap();

      let token = ctx.get_token(chain, token_addr).await?;

      let amount = NumericValue::format_wei(decoded.value, token.decimals);
      let amount_usd =
         ctx.get_currency_value_for_amount(amount.f64(), &Currency::from(token.clone()));
      let owner = decoded.owner;
      let spender = decoded.spender;

      let params = TokenApproveParams {
         token: token,
         amount: amount,
         amount_usd: Some(amount_usd),
         owner,
         spender,
      };

      Ok(params)
   }
}
