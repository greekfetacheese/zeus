use super::parse_typed_data;
use crate::core::clear_signing::{
   self, ClearDisplay, ClearSource, DisplayField, FormattedValue, Intent,
};
use crate::{core::ZeusCtx, utils::TimeStamp};
use anyhow::anyhow;
use serde_json::{Value, json};
use std::str::FromStr;
use zeus_eth::{
   abi::permit::Permit2,
   alloy_dyn_abi::TypedData,
   alloy_primitives::{Address, U256},
   alloy_signer::{Signature, Signer},
   currency::ERC20Token,
   utils::{NumericValue, address_book},
};
use zeus_wallet::SecureKey;

const PERMIT_SINGLE: &str = "PermitSingle";
const PERMIT_2612: &str = "Permit";

#[derive(Debug, Clone)]
pub enum SignMsgType {
   Permit2(Permit2Details),
   Permit2Batch(Permit2BatchDetails),
   Permit2612(Permit2612Details),
   ClearSigned(ClearSignedDetails),
   PersonalSign(String),
   Other(Value),
}

#[derive(Debug, Clone)]
pub struct ClearSignedDetails {
   pub raw: Value,
   pub display: ClearDisplay,
}

impl SignMsgType {
   pub fn dummy_permit2() -> Self {
      Self::Permit2(Permit2Details::dummy())
   }

   pub fn dummy_permit2612() -> Self {
      Self::Permit2612(Permit2612Details::dummy())
   }

   pub fn dummy_clear_signed() -> Self {
      Self::ClearSigned(ClearSignedDetails::dummy())
   }

   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      msg_value: Option<Value>,
      msg_string: Option<String>,
   ) -> Result<Self, anyhow::Error> {
      if let Some(string) = msg_string {
         return Ok(Self::PersonalSign(string));
      }

      if let Some(value) = msg_value {
         if let Ok(details) = Permit2Details::new(ctx.clone(), chain, value.clone()).await {
            return Ok(Self::Permit2(details));
         }
         if let Ok(details) = Permit2612Details::new(ctx.clone(), chain, value.clone()).await {
            return Ok(Self::Permit2612(details));
         }
         if let Ok(typed) = parse_typed_data(value.clone()) {
            if let Some(display) =
               clear_signing::try_clear_sign_typed_data(ctx, chain, &typed).await
            {
               return Ok(Self::ClearSigned(ClearSignedDetails {
                  raw: value,
                  display,
               }));
            }
         }
         return Ok(Self::Other(value));
      }

      return Err(anyhow!("No message found"));
   }

   pub fn is_known(&self) -> bool {
      matches!(
         self,
         Self::Permit2(_) | Self::Permit2Batch(_) | Self::Permit2612(_) | Self::ClearSigned(_)
      )
   }

   pub fn is_clear_signed(&self) -> bool {
      matches!(self, Self::ClearSigned(_))
   }

   pub fn is_permit2_single(&self) -> bool {
      matches!(self, Self::Permit2(_))
   }

   pub fn is_permit2_batch(&self) -> bool {
      matches!(self, Self::Permit2Batch(_))
   }

   pub fn is_permit2612(&self) -> bool {
      matches!(self, Self::Permit2612(_))
   }

   pub fn is_other(&self) -> bool {
      matches!(self, Self::Other(_))
   }

   pub fn is_personal_sign(&self) -> bool {
      matches!(self, Self::PersonalSign(_))
   }

   pub fn msg_string(&self) -> Option<String> {
      match self {
         Self::PersonalSign(msg) => Some(msg.clone()),
         _ => None,
      }
   }

   pub fn typed_data(&self) -> Option<TypedData> {
      match self {
         Self::Permit2(details) => parse_typed_data(details.raw_msg.clone()).ok(),
         Self::Permit2Batch(details) => parse_typed_data(details.msg_value.clone()).ok(),
         Self::Permit2612(details) => parse_typed_data(details.raw_msg.clone()).ok(),
         Self::ClearSigned(details) => parse_typed_data(details.raw.clone()).ok(),
         Self::PersonalSign(_) => None,
         Self::Other(details) => parse_typed_data(details.clone()).ok(),
      }
   }

   pub async fn sign(&self, signer: &SecureKey) -> Result<Signature, anyhow::Error> {
      match self {
         Self::Permit2(_) | Self::Permit2Batch(_) | Self::Permit2612(_) | Self::ClearSigned(_) => {
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

   pub fn msg_value(&self) -> Value {
      match self {
         Self::Permit2(details) => details.msg_value.clone(),
         Self::Permit2Batch(details) => details.msg_value.clone(),
         Self::Permit2612(details) => details.msg_value.clone(),
         Self::ClearSigned(details) => details.raw.clone(),
         Self::PersonalSign(msg) => json!(msg),
         Self::Other(details) => details.clone(),
      }
   }

   pub fn title(&self) -> &str {
      match self {
         Self::Permit2(p) => p.title(),
         Self::Permit2Batch(_) => "Permit2 Batch Token Approval",
         Self::Permit2612(p) => p.title(),
         Self::ClearSigned(details) => details.display.heading.as_str(),
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

   pub fn permit2612_details(&self) -> &Permit2612Details {
      match self {
         Self::Permit2612(details) => details,
         _ => panic!("Not an EIP-2612 permit"),
      }
   }

   pub fn clear_signed_details(&self) -> &ClearSignedDetails {
      match self {
         Self::ClearSigned(details) => details,
         _ => panic!("Not a clear-signed message"),
      }
   }
}

impl ClearSignedDetails {
   pub fn dummy() -> Self {
      let usdc = Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap();
      let spender = Address::from_str("0x2222222222222222222222222222222222222222").unwrap();
      let token = ERC20Token {
         chain_id: 8453,
         address: usdc,
         symbol: "USDC".into(),
         name: "USD Coin".into(),
         decimals: 6,
         total_supply: U256::ZERO,
      };
      Self {
         raw: dummy_permit_json(),
         display: ClearDisplay {
            heading: "Permit".to_string(),
            intent: Intent::Text("Permit".to_string()),
            interpolated_intent: Some("Approve 1 USDC for Router".to_string()),
            owner: Some("Circle".to_string()),
            contract_name: Some("USDC".to_string()),
            info_url: None,
            fields: vec![
               DisplayField {
                  label: "Spender".to_string(),
                  value: FormattedValue::Address(spender),
               },
               DisplayField {
                  label: "Amount".to_string(),
                  value: FormattedValue::TokenAmount {
                     amount: NumericValue::format_wei(U256::from(1_000_000u64), 6),
                     token,
                     unlimited: false,
                  },
               },
            ],
            source: ClearSource::Registry {
               path: "dummy".to_string(),
            },
            warnings: Vec::new(),
         },
      }
   }
}

#[derive(Debug, Clone)]
pub struct Permit2BatchDetails {
   pub permit_batch: Permit2::PermitBatch,
   pub tokens: Vec<ERC20Token>,
   pub amounts: Vec<NumericValue>,
   pub amounts_usd: Vec<Option<NumericValue>>,
   pub expiration: TimeStamp,
   pub permit2_contract: Address,
   pub spender: Address,
   pub msg_value: Value,
}

#[derive(Debug, Clone)]
pub struct Permit2Details {
   pub token: ERC20Token,
   pub amount: NumericValue,
   pub amount_usd: Option<NumericValue>,
   pub expiration: TimeStamp,
   pub permit2_contract: Address,
   pub spender: Address,
   pub msg_value: Value,
   pub raw_msg: Value,
}

impl Permit2Details {
   pub fn title(&self) -> &str {
      if self.amount.is_zero() {
         "Revoke Permit2"
      } else if self.is_unlimited() {
         "Unlimited Permit2 Token Approval"
      } else {
         "Permit2 Token Approval"
      }
   }

   pub fn is_unlimited(&self) -> bool {
      self.amount.wei() == U256::MAX
   }

   pub fn dummy() -> Self {
      let permit2 = Address::from_str("0x000000000022d473030f116ddee9f6b43ac78ba3").unwrap();
      let spender = Address::from_str("0x6ff5693b99212da76ad316178a184ab56d299b43").unwrap();
      Self {
         token: ERC20Token::weth_base(),
         amount: NumericValue::parse_to_wei("100000000", 18),
         amount_usd: Some(NumericValue::value(1.0, 1600.0)),
         expiration: TimeStamp::now_as_secs().unwrap().saturating_add_secs(600),
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

      let token = ctx.get_token(chain, token_addr).await?;

      let amount = message["details"]["amount"].as_str().ok_or(anyhow!("Missing amount"))?;
      let amount = U256::from_str(amount)?;
      let amount = NumericValue::format_wei(amount, token.decimals);
      let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &token);

      let expiration =
         message["details"]["expiration"].as_str().ok_or(anyhow!("Missing expiration"))?;
      let expiration = u64::from_str(expiration)?;
      let exp_timestamp = TimeStamp::Seconds(expiration);

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
         expiration: exp_timestamp,
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
         self.amount.abbreviated()
      }
   }
}

/// EIP-2612 `Permit` — gasless ERC-20 allowance signed against the token itself.
#[derive(Debug, Clone)]
pub struct Permit2612Details {
   pub token: ERC20Token,
   pub owner: Address,
   pub spender: Address,
   pub amount: NumericValue,
   pub amount_usd: Option<NumericValue>,
   pub nonce: U256,
   pub deadline: U256,
   pub msg_value: Value,
   pub raw_msg: Value,
}

struct Permit2612Fields {
   owner: Address,
   spender: Address,
   value: U256,
   nonce: U256,
   deadline: U256,
   token: Address,
   chain: u64,
}

fn json_u256(value: &Value) -> Result<U256, anyhow::Error> {
   match value {
      Value::Null => Ok(U256::ZERO),
      Value::String(s) => {
         if s.is_empty() {
            return Ok(U256::ZERO);
         }
         if let Some(hex_val) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            if hex_val.is_empty() {
               return Ok(U256::ZERO);
            }
            U256::from_str_radix(hex_val, 16).map_err(|e| anyhow!("{e}"))
         } else {
            U256::from_str(s).map_err(|e| anyhow!("{e}"))
         }
      }
      Value::Number(n) => {
         if let Some(v) = n.as_u64() {
            Ok(U256::from(v))
         } else {
            U256::from_str(&n.to_string()).map_err(|e| anyhow!("{e}"))
         }
      }
      _ => Err(anyhow!("value is not a uint256")),
   }
}

fn json_address(value: &Value) -> Result<Address, anyhow::Error> {
   let s = value.as_str().ok_or_else(|| anyhow!("value is not an address"))?;
   Address::from_str(s).map_err(|e| anyhow!("{e}"))
}

fn parse_permit2612_fields(
   data: &TypedData,
   fallback_chain: u64,
) -> Result<Permit2612Fields, anyhow::Error> {
   if data.primary_type != PERMIT_2612 {
      return Err(anyhow!("not an EIP-2612 Permit"));
   }

   let message = &data.message;
   let owner = json_address(&message["owner"])?;
   let spender = json_address(&message["spender"])?;
   let value = json_u256(&message["value"])?;
   let nonce = json_u256(&message["nonce"])?;
   let deadline = json_u256(&message["deadline"])?;
   let token = data
      .domain
      .verifying_contract
      .ok_or_else(|| anyhow!("missing verifying contract"))?;
   let chain = data
      .domain
      .chain_id
      .and_then(|id| u64::try_from(id).ok())
      .unwrap_or(fallback_chain);

   Ok(Permit2612Fields {
      owner,
      spender,
      value,
      nonce,
      deadline,
      token,
      chain,
   })
}

impl Permit2612Details {
   pub fn title(&self) -> &str {
      if self.amount.is_zero() {
         "Revoke Token Permit"
      } else if self.is_unlimited() {
         "Unlimited Token Permit"
      } else {
         "Token Permit"
      }
   }

   pub fn is_unlimited(&self) -> bool {
      self.amount.wei() == U256::MAX
   }

   /// EIP-2612: `deadline` is unix seconds; `type(uint256).max` means never expires.
   pub fn deadline_label(&self) -> String {
      if self.deadline == U256::MAX {
         return "Never".to_string();
      }
      let Ok(secs) = u64::try_from(self.deadline) else {
         return "Never".to_string();
      };
      TimeStamp::Seconds(secs).to_relative()
   }

   pub fn dummy() -> Self {
      let token_addr = Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap();
      let owner = Address::from_str("0x1111111111111111111111111111111111111111").unwrap();
      let spender = Address::from_str("0x2222222222222222222222222222222222222222").unwrap();
      Self {
         token: ERC20Token {
            chain_id: 8453,
            address: token_addr,
            symbol: "USDC".into(),
            name: "USD Coin".into(),
            decimals: 6,
            total_supply: U256::ZERO,
         },
         owner,
         spender,
         amount: NumericValue::format_wei(U256::from(1_000_000u64), 6),
         amount_usd: Some(NumericValue::value(1.0, 1.0)),
         nonce: U256::ZERO,
         deadline: U256::from(TimeStamp::now_as_secs().unwrap().timestamp() + 600),
         msg_value: dummy_permit_json()["message"].clone(),
         raw_msg: dummy_permit_json(),
      }
   }

   pub async fn new(ctx: ZeusCtx, chain: u64, msg: Value) -> Result<Self, anyhow::Error> {
      let data = parse_typed_data(msg.clone())?;
      let fields = parse_permit2612_fields(&data, chain)?;
      let token = ctx.get_token(fields.chain, fields.token).await?;
      let amount = NumericValue::format_wei(fields.value, token.decimals);
      let amount_usd = if fields.value == U256::MAX {
         None
      } else {
         Some(ctx.get_token_value_for_amount(amount.f64(), &token))
      };

      Ok(Self {
         token,
         owner: fields.owner,
         spender: fields.spender,
         amount,
         amount_usd,
         nonce: fields.nonce,
         deadline: fields.deadline,
         msg_value: data.message,
         raw_msg: msg,
      })
   }

   pub fn amount(&self) -> String {
      if self.is_unlimited() {
         "Unlimited".to_string()
      } else {
         self.amount.abbreviated()
      }
   }
}

fn dummy_permit_json() -> serde_json::Value {
   serde_json::json!({
       "types": {
           "Permit": [
               {"name": "owner", "type": "address"},
               {"name": "spender", "type": "address"},
               {"name": "value", "type": "uint256"},
               {"name": "nonce", "type": "uint256"},
               {"name": "deadline", "type": "uint256"}
           ],
           "EIP712Domain": [
               {"name": "name", "type": "string"},
               {"name": "version", "type": "string"},
               {"name": "chainId", "type": "uint256"},
               {"name": "verifyingContract", "type": "address"}
           ]
       },
       "domain": {
           "name": "USD Coin",
           "version": "2",
           "chainId": "8453",
           "verifyingContract": "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"
       },
       "primaryType": "Permit",
       "message": {
           "owner": "0x1111111111111111111111111111111111111111",
           "spender": "0x2222222222222222222222222222222222222222",
           "value": "1000000",
           "nonce": "0",
           "deadline": "1745151870"
       }
   })
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
      let msg_type = SignMsgType::new(ctx, 8453, Some(json), None).await.unwrap();
      let permit2 = msg_type.permit2_details();

      assert_eq!(
         permit2.token.address,
         Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap()
      );
      assert_eq!(
         permit2.permit2_contract,
         Address::from_str("0x000000000022d473030f116ddee9f6b43ac78ba3").unwrap()
      );
      assert_eq!(
         permit2.spender,
         Address::from_str("0x6ff5693b99212da76ad316178a184ab56d299b43").unwrap()
      );
   }

   #[test]
   fn json_u256_accepts_decimal_hex_and_number() {
      assert_eq!(
         json_u256(&json!("1000000")).unwrap(),
         U256::from(1_000_000u64)
      );
      assert_eq!(json_u256(&json!("0x0")).unwrap(), U256::ZERO);
      assert_eq!(json_u256(&json!(0)).unwrap(), U256::ZERO);
      assert_eq!(
         json_u256(&json!(
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
         ))
         .unwrap(),
         U256::MAX
      );
   }

   #[test]
   fn parse_eip2612_permit_fields() {
      let typed = parse_typed_data(dummy_permit_json()).unwrap();
      let fields = parse_permit2612_fields(&typed, 1).unwrap();
      assert_eq!(
         fields.token,
         Address::from_str("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap()
      );
      assert_eq!(fields.chain, 8453);
      assert_eq!(fields.value, U256::from(1_000_000u64));
      assert_eq!(
         fields.spender,
         Address::from_str("0x2222222222222222222222222222222222222222").unwrap()
      );
   }

   #[test]
   fn parse_eip2612_infinite_permit() {
      let json = json!({
          "types": {
              "Permit": [
                  {"name": "owner", "type": "address"},
                  {"name": "spender", "type": "address"},
                  {"name": "value", "type": "uint256"},
                  {"name": "nonce", "type": "uint256"},
                  {"name": "deadline", "type": "uint256"}
              ],
              "EIP712Domain": [
                  {"name": "name", "type": "string"},
                  {"name": "version", "type": "string"},
                  {"name": "chainId", "type": "uint256"},
                  {"name": "verifyingContract", "type": "address"}
              ]
          },
          "domain": {
              "name": "USDC",
              "version": "2",
              "chainId": 1,
              "verifyingContract": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
          },
          "primaryType": "Permit",
          "message": {
              "owner": "0x0000000000000000000000000000000000000000",
              "spender": "0x1111111111111111111111111111111111111111",
              "value": "115792089237316195423570985008687907853269984665640564039457584007913129639935",
              "nonce": 0,
              "deadline": 1893456000
          }
      });
      let typed = parse_typed_data(json).unwrap();
      let fields = parse_permit2612_fields(&typed, 1).unwrap();
      assert_eq!(fields.value, U256::MAX);
      assert_eq!(fields.nonce, U256::ZERO);
      assert_eq!(fields.deadline, U256::from(1893456000u64));
      assert_eq!(fields.chain, 1);
      let details = Permit2612Details {
         token: ERC20Token {
            chain_id: 1,
            address: fields.token,
            symbol: "USDC".into(),
            name: "USD Coin".into(),
            decimals: 6,
            total_supply: U256::ZERO,
         },
         owner: fields.owner,
         spender: fields.spender,
         amount: NumericValue::format_wei(fields.value, 6),
         amount_usd: None,
         nonce: fields.nonce,
         deadline: fields.deadline,
         msg_value: json!({}),
         raw_msg: json!({}),
      };
      assert!(details.is_unlimited());
      assert_eq!(details.amount(), "Unlimited");
   }

   #[test]
   fn eip2612_max_deadline_is_never() {
      let details = Permit2612Details {
         token: ERC20Token::weth_base(),
         owner: Address::ZERO,
         spender: Address::ZERO,
         amount: NumericValue::format_wei(U256::from(1u64), 18),
         amount_usd: None,
         nonce: U256::ZERO,
         deadline: U256::MAX,
         msg_value: json!({}),
         raw_msg: json!({}),
      };
      assert_eq!(details.deadline_label(), "Never");
   }

   #[test]
   fn dummy_permit2612_is_known() {
      let msg = SignMsgType::dummy_permit2612();
      assert!(msg.is_permit2612());
      assert!(msg.is_known());
      assert_eq!(msg.title(), "Token Permit");
   }
}
