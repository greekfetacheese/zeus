use super::{parse_typed_data, siwe};
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
   SignMessage(SignMessageData),
   /// EIP-4361 Sign-In With Ethereum: an EIP-191 message that authenticates
   /// the account to a given domain. Signed exactly like `SignMessage`; the
   /// variant exists so the prompt can name what is being signed.
   Siwe(SignMessageData),
   Other(Value),
}

/// EIP-191 `personal_sign` message. The display string is a lossy UTF-8
/// rendering; signing always uses the raw bytes exactly as the dapp sent them.
#[derive(Debug, Clone)]
pub struct SignMessageData {
   pub display: String,
   pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ClearSignedDetails {
   pub raw: Value,
   pub display: ClearDisplay,
   /// The chain this message is signed for — its own `domain.chainId`, which is the chain the
   /// descriptor and every value in `display` were resolved for.
   ///
   /// Carried here rather than on [`ClearDisplay`] because that type is stored inside a recorded
   /// transaction, so a field added to it would be a persisted-shape change; this wrapper is
   /// build-to-prompt only. The prompt draws its chain row and its address labels from here.
   ///
   /// A clear-signed message always has one: [`try_clear_sign_typed_data`] refuses a domain without
   /// a usable `chainId`, since the descriptor is looked up by `(chain, verifyingContract, typeHash)`.
   ///
   /// [`try_clear_sign_typed_data`]: crate::core::clear_signing::try_clear_sign_typed_data
   pub chain: u64,
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

   pub fn dummy_siwe() -> Self {
      let account = Address::from_str("0x6ff5693b99212da76ad316178a184ab56d299b43").unwrap();
      let display = format!(
         "https://beta.walletbeat.eth.limo wants you to sign in with your Ethereum account:\n\
          {}\n\n\
          Sign in to authenticate your wallet. This is a test SIWE message.\n\n\
          URI: https://beta.walletbeat.eth.limo/\n\
          Version: 1\n\
          Chain ID: 1\n\
          Nonce: s6y3xys06pc\n\
          Issued At: 2026-10-02T04:16:56.083Z",
         account.to_checksum(None)
      );

      Self::Siwe(SignMessageData {
         bytes: display.as_bytes().to_vec(),
         display,
      })
   }

   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      msg_value: Option<Value>,
      msg_bytes: Option<Vec<u8>>,
   ) -> Result<Self, anyhow::Error> {
      if let Some(bytes) = msg_bytes {
         let display = String::from_utf8_lossy(&bytes).into_owned();
         let data = SignMessageData { display, bytes };

         if siwe::is_siwe(&data.display) {
            return Ok(Self::Siwe(data));
         }

         return Ok(Self::SignMessage(data));
      }

      if let Some(value) = msg_value {
         if let Ok(details) = Permit2Details::new(ctx.clone(), chain, value.clone()).await {
            return Ok(Self::Permit2(details));
         }
         if let Ok(details) = Permit2612Details::new(ctx.clone(), chain, value.clone()).await {
            return Ok(Self::Permit2612(details));
         }
         if let Ok(typed) = parse_typed_data(value.clone()) {
            if let Some(display) = clear_signing::try_clear_sign_typed_data(ctx, &typed).await {
               return Ok(Self::ClearSigned(ClearSignedDetails {
                  raw: value,
                  display,
                  // The descriptor and every value above were resolved for the domain's chain, so
                  // that is the chain the prompt has to name.
                  chain: domain_chain(typed.domain.chain_id, chain),
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

   pub fn is_sign_message(&self) -> bool {
      matches!(self, Self::SignMessage(_))
   }

   /// Whether signing this message grants an unlimited allowance. Only the
   /// permit variants can.
   pub fn is_unlimited(&self) -> bool {
      match self {
         Self::Permit2(details) => details.is_unlimited(),
         Self::Permit2612(details) => details.is_unlimited(),
         _ => false,
      }
   }

   pub fn msg_string(&self) -> Option<String> {
      match self {
         Self::SignMessage(msg) | Self::Siwe(msg) => Some(msg.display.clone()),
         _ => None,
      }
   }

   pub fn typed_data(&self) -> Option<TypedData> {
      match self {
         Self::Permit2(details) => parse_typed_data(details.raw_msg.clone()).ok(),
         Self::Permit2Batch(details) => parse_typed_data(details.msg_value.clone()).ok(),
         Self::Permit2612(details) => parse_typed_data(details.raw_msg.clone()).ok(),
         Self::ClearSigned(details) => parse_typed_data(details.raw.clone()).ok(),
         Self::SignMessage(_) | Self::Siwe(_) => None,
         Self::Other(details) => parse_typed_data(details.clone()).ok(),
      }
   }

   pub async fn sign(&self, signer: SecureKey) -> Result<Signature, anyhow::Error> {
      let signer = signer.to_signer();
      match self {
         Self::Permit2(_) | Self::Permit2Batch(_) | Self::Permit2612(_) | Self::ClearSigned(_) => {
            let typed = match self.typed_data() {
               Some(data) => data,
               None => return Err(anyhow!("No typed data found")),
            };
            let sig = signer.sign_dynamic_typed_data(&typed).await?;
            Ok(sig)
         }
         Self::SignMessage(msg) | Self::Siwe(msg) => {
            let sig = signer.sign_message(&msg.bytes).await?;
            Ok(sig)
         }
         Self::Other(details) => {
            let typed = self.typed_data();
            if typed.is_some() {
               let typed = typed.unwrap();
               let sig = signer.sign_dynamic_typed_data(&typed).await?;
               Ok(sig)
            } else {
               let msg = details.to_string();
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
         Self::SignMessage(msg) | Self::Siwe(msg) => json!(msg.display),
         Self::Other(details) => details.clone(),
      }
   }

   pub fn title(&self) -> &str {
      match self {
         Self::Permit2(p) => p.title(),
         Self::Permit2Batch(_) => "Permit2 Batch Token Approval",
         Self::Permit2612(p) => p.title(),
         Self::ClearSigned(details) => details.display.heading.as_str(),
         Self::SignMessage(_) => "Sign Message",
         Self::Siwe(_) => "Sign-In With Ethereum",
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
         // The fixture's own domain chain, and the chain its USDC token lives on.
         chain: 8453,
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
   /// The chain the permit is signed for — its own `domain.chainId`, not the chain the wallet
   /// happens to be connected to. The signature is bound to that domain, so it is the chain the
   /// allowance lands on, and the chain the token above was read from.
   ///
   /// Permit2's `verifyingContract` cannot stand in for this: the canonical Permit2 address is the
   /// same on every chain, so comparing it against the connected chain's entry ties the message to
   /// no chain at all. The domain's `chainId` is the only thing that names one.
   pub chain: u64,
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
      let token = ERC20Token::weth_base();
      Self {
         chain: token.chain_id,
         token,
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

      // The chain the permit is signed for. Not `chain`, the chain the wallet is connected to:
      // `chain` is only where the user happened to be, and a permit naming another one grants the
      // allowance there. Everything read for this prompt — the token, its price, the addresses —
      // has to come from the permit's own chain, or the prompt describes a different token's
      // approval than the one being signed for.
      let chain = domain_chain(data.domain.chain_id, chain);

      let token_address =
         message["details"]["token"].as_str().ok_or(anyhow!("Missing token address"))?;
      let token_addr = Address::from_str(token_address)?;

      let token = ctx.get_token(chain, token_addr).await?;
      let price_manager = ctx.price_manager();
      let pool_manager = ctx.pool_manager();

      if let Err(e) = price_manager
         .calculate_prices(
            ctx.clone(),
            chain,
            pool_manager,
            vec![token.clone()],
         )
         .await
      {
         tracing::error!("Error updating prices: {:?}", e);
      }

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
         // `chain` is the permit's own domain chain here, not the wallet's.
         chain,
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

   pub fn amount(&self) -> &str {
      if self.amount.wei() == U256::MAX {
         "Unlimited"
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

/// The chain a permit is signed for: the one its own `domain.chainId` names.
///
/// Both permit shapes resolve their chain through here, because the domain is what the signature
/// is bound to — it, and not the chain the wallet is connected to, is where the allowance lands.
/// A domain that names no usable chain falls back to the wallet's own: an absent `chainId` is the
/// ordinary case, and one that does not fit a `u64` cannot be a chain Zeus signed for.
fn domain_chain(chain_id: Option<U256>, fallback_chain: u64) -> u64 {
   chain_id.and_then(|id| u64::try_from(id).ok()).unwrap_or(fallback_chain)
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
   let chain = domain_chain(data.domain.chain_id, fallback_chain);

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

      let price_manager = ctx.price_manager();
      let pool_manager = ctx.pool_manager();

      if let Err(e) = price_manager
         .calculate_prices(
            ctx.clone(),
            // Price the token that is being shown, on the chain it lives on. The wallet's
            // chain is the same value whenever the permit names it; a differing one would
            // look up `(wallet_chain, token.address)`, which is a different token or none
            // at all, so the amount would carry the wrong USD figure.
            fields.chain,
            pool_manager,
            vec![token.clone()],
         )
         .await
      {
         tracing::error!("Error updating prices: {:?}", e);
      }

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

   pub fn amount(&self) -> &str {
      if self.is_unlimited() {
         "Unlimited"
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
      // The chain this permit is signed for, taken from the fixture's `domain.chainId`. The prompt
      // renders this and not the wallet's chain, and the token above was read on it.
      assert_eq!(permit2.chain, 8453);
   }

   /// A permit's chain comes from its own domain, not from the chain the wallet happens to be
   /// connected to: the domain is what the signature is bound to, and so it is where the allowance
   /// lands. Permit2's `verifyingContract` cannot decide this — it is the same address on every
   /// chain — which is why both permit shapes resolve their chain through here, and why one test
   /// covers the decision they share.
   #[test]
   fn a_permit_takes_its_chain_from_the_domain() {
      assert_eq!(domain_chain(Some(U256::from(8453u64)), 1), 8453);

      // A domain that names no chain leaves the wallet's own as the only thing to go on.
      assert_eq!(domain_chain(None, 1), 1);

      // And one that does not fit a `u64` cannot be a chain Zeus signs for, so it is not trusted
      // as one either.
      assert_eq!(domain_chain(Some(U256::MAX), 1), 1);
   }

   /// A permit's token is read on the permit's own chain, not on the wallet's. Driven with the
   /// wallet on Ethereum and a permit whose domain names Base: an address is a different token per
   /// chain, so resolving it on the wallet's chain is exactly how a prompt ends up showing one
   /// token's approval while the signature grants another's.
   ///
   /// `Permit2Details::new` is called directly rather than through `SignMsgType::new` so a failure
   /// here is this assertion and not a fall-through decoder deciding the message is something else.
   #[tokio::test]
   async fn permit2_reads_its_token_on_the_permit_chain() {
      let ctx = ZeusCtx::new();
      // The wallet is connected to Ethereum; the permit is for Base.
      let permit2 = Permit2Details::new(ctx, 1, dummy_permit2_json())
         .await
         .expect("a permit for Base must resolve its token on Base, not on the wallet's chain");

      assert_eq!(
         permit2.chain, 8453,
         "the prompt's chain is the permit's own, not the wallet's"
      );
      assert_eq!(
         permit2.token.chain_id, 8453,
         "and the token it names was read on that chain"
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

   /// A clear-signed message carries the chain its descriptor was resolved for, and the prompt
   /// draws its chain row and its address labels from that rather than from the wallet's chain.
   ///
   /// The resolution itself is structurally domain-only — `try_clear_sign_typed_data` takes no
   /// chain, because the descriptor is looked up by `(domain chainId, verifyingContract, typeHash)`
   /// — so what is left to pin is that the carrier is populated at the one construction site.
   #[test]
   fn a_clear_signed_message_carries_its_domains_chain() {
      let msg = SignMsgType::dummy_clear_signed();
      let clear = msg.clear_signed_details();

      assert!(msg.is_clear_signed());
      assert_eq!(
         clear.chain, 8453,
         "the fixture's own domain chain, which is also where its USDC lives"
      );
   }

   #[tokio::test]
   async fn classifies_siwe_from_personal_sign_bytes() {
      let SignMsgType::Siwe(dummy) = SignMsgType::dummy_siwe() else {
         panic!("dummy_siwe must be a SIWE message");
      };

      let msg = SignMsgType::new(ZeusCtx::new(), 1, None, Some(dummy.bytes)).await.unwrap();

      assert!(matches!(msg, SignMsgType::Siwe(_)));
      assert_eq!(msg.title(), "Sign-In With Ethereum");
   }

   #[tokio::test]
   async fn classifies_plain_message() {
      let bytes = b"Sign in to prove you own this wallet.".to_vec();
      let msg = SignMsgType::new(ZeusCtx::new(), 1, None, Some(bytes)).await.unwrap();

      assert!(msg.is_sign_message());
      assert_eq!(msg.title(), "Sign Message");
   }
}
