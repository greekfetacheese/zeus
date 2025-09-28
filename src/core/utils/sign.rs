use super::parse_typed_data;
use crate::core::ZeusCtx;
use anyhow::anyhow;
use serde_json::{Value, json};
use std::str::FromStr;
use zeus_eth::{
   abi::permit::Permit2,
   alloy_dyn_abi::TypedData,
   alloy_primitives::{Address, U256},
   alloy_signer::{Signature, Signer},
   currency::{Currency, ERC20Token},
   utils::{NumericValue, SecureSigner, address_book},
};

const PERMIT_SINGLE: &str = "PermitSingle";

#[derive(Debug, Clone)]
pub enum SignMsgType {
   Permit2(Permit2Details),
   Permit2Batch(Permit2BatchDetails),
   PersonalSign(Value),
   Other(Value),
}

impl SignMsgType {
   pub fn dummy_permit2() -> Self {
      Self::Permit2(Permit2Details::dummy())
   }

   pub async fn from_msg(ctx: ZeusCtx, chain: u64, msg: Value) -> Self {
      let mut msg_type = Self::Other(msg.clone());

      if let Ok(details) = Permit2Details::new(ctx, chain, msg).await {
         msg_type = Self::Permit2(details);
      }

      msg_type
   }

   pub fn from_message(message: String) -> Self {
      Self::PersonalSign(json!(message))
   }

   pub fn is_known(&self) -> bool {
      matches!(self, Self::Permit2(_) | Self::Permit2Batch(_))
   }

   pub fn is_permit2_single(&self) -> bool {
      matches!(self, Self::Permit2(_))
   }

   pub fn is_permit2_batch(&self) -> bool {
      matches!(self, Self::Permit2Batch(_))
   }

   pub fn is_other(&self) -> bool {
      matches!(self, Self::Other(_))
   }

   pub fn is_personal_sign(&self) -> bool {
      matches!(self, Self::PersonalSign(_))
   }

   pub fn typed_data(&self) -> Option<TypedData> {
      match self {
         Self::Permit2(details) => match parse_typed_data(details.raw_msg.clone()) {
            Ok(data) => Some(data),
            Err(_) => {
               None
            },
         },
         Self::Permit2Batch(details) => match parse_typed_data(details.msg_value.clone()) {
            Ok(data) => Some(data),
            Err(_) => None,
         },
         Self::PersonalSign(msg) => match parse_typed_data(msg.clone()) {
            Ok(data) => Some(data),
            Err(_) => None,
         },
         Self::Other(details) => match parse_typed_data(details.clone()) {
            Ok(data) => Some(data),
            Err(_) => None,
         },
      }
   }

   pub async fn sign(&self, signer: &SecureSigner) -> Result<Signature, anyhow::Error> {
      match self {
         Self::Permit2(_) => {
            let typed = match self.typed_data() {
               Some(data) => data,
               None => return Err(anyhow!("No typed data found")),
            };
            let sig = signer.to_signer().sign_dynamic_typed_data(&typed).await?;
            Ok(sig)
         }
         Self::Permit2Batch(_) => {
            let typed = match self.typed_data() {
               Some(data) => data,
               None => return Err(anyhow!("No typed data found")),
            };
            let sig = signer.to_signer().sign_dynamic_typed_data(&typed).await?;
            Ok(sig)
         }
         Self::PersonalSign(msg) => {
            let msg = msg.to_string();
            let signer = signer.to_signer();
            let sig = signer.sign_message(msg.as_bytes()).await?;
            Ok(sig)
         }
         Self::Other(details) => {
            let typed = self.typed_data();
            if typed.is_some() {
               let typed = typed.unwrap();
               let sig = signer.to_signer().sign_dynamic_typed_data(&typed).await?;
               Ok(sig)
            } else {
               let msg = details.to_string();
               let signer = signer.to_signer();
               let sig = signer.sign_message(msg.as_bytes()).await?;
               Ok(sig)
            }
         }
      }
   }

   pub fn msg_value(&self) -> &Value {
      match self {
         Self::Permit2(details) => &details.msg_value,
         Self::Permit2Batch(details) => &details.msg_value,
         Self::PersonalSign(msg) => &msg,
         Self::Other(details) => &details,
      }
   }

   pub fn title(&self) -> &str {
      match self {
         Self::Permit2(_) => "Permit2 Token Approval",
         Self::Permit2Batch(_) => "Permit2 Batch Token Approval",
         Self::PersonalSign(_) => "Personal Sign",
         Self::Other(_) => "Unknown Message",
      }
   }

   /// Get the permit2 details if this is a permit2 message
   ///
   /// Panics if this is not a permit2 message
   pub fn permit2_details(&self) -> &Permit2Details {
      match self {
         Self::Permit2(details) => details,
         _ => panic!("Not a permit2 message"),
      }
   }

   /// Get the permit2 batch details if this is a permit2 batch message
   ///
   /// Panics if this is not a permit2 batch message
   pub fn permit2_batch_details(&self) -> &Permit2BatchDetails {
      match self {
         Self::Permit2Batch(details) => details,
         _ => panic!("Not a permit2 message"),
      }
   }
}

#[derive(Debug, Clone)]
pub struct Permit2BatchDetails {
   pub permit_batch: Permit2::PermitBatch,
   pub tokens: Vec<ERC20Token>,
   pub amounts: Vec<NumericValue>,
   pub amounts_usd: Vec<Option<NumericValue>>,
   pub expiration: u64,
   pub permit2_contract: Address,
   pub spender: Address,
   pub msg_value: Value,
}

#[derive(Debug, Clone)]
pub struct Permit2Details {
   pub token: ERC20Token,
   pub amount: NumericValue,
   pub amount_usd: Option<NumericValue>,
   pub expiration: u64,
   pub permit2_contract: Address,
   pub spender: Address,
   pub msg_value: Value,
   pub raw_msg: Value,
}

impl Permit2Details {
   pub fn dummy() -> Self {
      let permit2 = Address::from_str("0x000000000022d473030f116ddee9f6b43ac78ba3").unwrap();
      let spender = Address::from_str("0x6ff5693b99212da76ad316178a184ab56d299b43").unwrap();
      Self {
         token: ERC20Token::weth_base(),
         amount: NumericValue::parse_to_wei("100000000", 18),
         amount_usd: Some(NumericValue::value(1.0, 1600.0)),
         expiration: 1747886275,
         permit2_contract: permit2,
         spender,
         msg_value: dummy_permit2_json(),
         raw_msg: dummy_permit2_json(),
      }
   }

   pub async fn new(ctx: ZeusCtx, chain: u64, msg: Value) -> Result<Self, anyhow::Error> {
      let data = parse_typed_data(msg.clone())?;
      if data.primary_type != PERMIT_SINGLE {
         return Err(anyhow!("Invalid permit2 data"));
      }

      let message = &data.message;
      let domain = &data.domain;

      let token_address =
         message["details"]["token"].as_str().ok_or(anyhow!("Missing token address"))?;
      let token_addr = Address::from_str(token_address)?;

      let z_client = ctx.get_zeus_client();
      let cached = ctx.read(|ctx| ctx.currency_db.get_erc20_token(chain, token_addr));

      let token = if let Some(token) = cached {
         token
      } else {
         let token = z_client
            .request(chain, |client| async move {
               ERC20Token::new(client, token_addr, chain).await
            })
            .await?;
         ctx.write(|ctx| ctx.currency_db.insert_currency(chain, Currency::from(token.clone())));
         token
      };

      let amount = message["details"]["amount"].as_str().ok_or(anyhow!("Missing amount"))?;
      let amount = U256::from_str(amount)?;
      let amount = NumericValue::format_wei(amount, token.decimals);
      let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &token);

      let expiration =
         message["details"]["expiration"].as_str().ok_or(anyhow!("Missing expiration"))?;
      let expiration = u64::from_str(expiration)?;

      let spender_str = message["spender"].as_str().ok_or(anyhow!("Missing spender"))?;
      let spender = Address::from_str(spender_str)?;

      let permit2_contract =
         domain.verifying_contract.ok_or(anyhow!("Missing verifying contract"))?;

      let actual_permit2_contract = address_book::permit2_contract(chain)?;

      if actual_permit2_contract != permit2_contract {
         return Err(anyhow!(
            "The extracted permit2 contract address from the msg does not match with the actual Permit2 contract address"
         ));
      }

      Ok(Self {
         token,
         amount,
         amount_usd: Some(amount_usd),
         expiration,
         permit2_contract,
         spender,
         msg_value: message.clone(),
         raw_msg: msg,
      })
   }

   pub fn amount(&self) -> String {
      if self.amount.wei() == U256::MAX {
         "Unlimited".to_string()
      } else {
         self.amount.format_abbreviated()
      }
   }
}

fn dummy_permit2_json() -> serde_json::Value {
   serde_json::json!({
       "types": {
           "PermitSingle": [
               {
                   "name": "details",
                   "type": "PermitDetails"
               },
               {
                   "name": "spender",
                   "type": "address"
               },
               {
                   "name": "sigDeadline",
                   "type": "uint256"
               }
           ],
           "PermitDetails": [
               {
                   "name": "token",
                   "type": "address"
               },
               {
                   "name": "amount",
                   "type": "uint160"
               },
               {
                   "name": "expiration",
                   "type": "uint48"
               },
               {
                   "name": "nonce",
                   "type": "uint48"
               }
           ],
           "EIP712Domain": [
               {
                   "name": "name",
                   "type": "string"
               },
               {
                   "name": "chainId",
                   "type": "uint256"
               },
               {
                   "name": "verifyingContract",
                   "type": "address"
               }
           ]
       },
       "domain": {
           "name": "Permit2",
           "chainId": "8453",
           "verifyingContract": "0x000000000022d473030f116ddee9f6b43ac78ba3"
       },
       "primaryType": "PermitSingle",
       "message": {
           "details": {
               "token": "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
               "amount": "1461501637330902918203684832716283019655932542975",
               "expiration": "1747742070",
               "nonce": "0"
           },
           "spender": "0x6ff5693b99212da76ad316178a184ab56d299b43",
           "sigDeadline": "1745151870"
       }
   })
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::ZeusCtx;

   #[tokio::test]
   async fn test_permit2_details() {
      let ctx = ZeusCtx::new();
      let json = dummy_permit2_json();
      let msg_type = SignMsgType::from_msg(ctx, 8453, json).await;
      let permit2 = msg_type.permit2_details();

      assert_eq!(
         permit2.token.address,
         Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap()
      );
      assert_eq!(permit2.expiration, 1747742070);
      assert_eq!(
         permit2.permit2_contract,
         Address::from_str("0x000000000022d473030f116ddee9f6b43ac78ba3").unwrap()
      );
      assert_eq!(
         permit2.spender,
         Address::from_str("0x6ff5693b99212da76ad316178a184ab56d299b43").unwrap()
      );
   }
}
