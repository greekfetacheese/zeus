use crate::core::ZeusCtx;

use anyhow::anyhow;
use serde_json::Value;
use zeus_eth::abi::permit::{Permit2::allowanceReturn, allowance};
use zeus_eth::alloy_dyn_abi::{Eip712Domain, Eip712Types, Resolver, TypedData};
use zeus_eth::alloy_primitives::{Address, U256, aliases::U48};
use zeus_eth::utils::address_book;
use zeus_eth::{
   alloy_signer::{Signature, Signer},
   currency::ERC20Token,
};
use zeus_wallet::SecureKey;

use std::time::{SystemTime, UNIX_EPOCH};

pub mod msg;
pub mod sign;

/// Info for a token approval through the Permit2 contract
#[derive(Clone)]
pub struct Permit2Info {
   /// The allowance details from the Permit2 contract for a token
   pub allowance: allowanceReturn,

   /// Whether the Permit2 contract needs to be approved to spend the token
   ///
   /// Usually we do this approval one-time with unlimited allowance
   pub needs_approval: bool,

   /// Whether we need to sign again an approval
   pub needs_new_signature: bool,

   /// When this approval should expire
   pub expiration: U256,

   /// When this signature should expire
   pub sig_deadline: U256,

   /// The message to be signed
   pub msg: Option<Value>,
}

impl Permit2Info {
   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      token: &ERC20Token,
      amount: U256,
      owner: Address,
      spender: Address,
   ) -> Result<Self, anyhow::Error> {
      let permit2 = address_book::permit2_contract(chain)?;
      let client = ctx.get_zeus_client();

      let data_fut = client.request(chain, |client| async move {
         let data = allowance(client, permit2, owner, token.address, spender).await?;
         Ok(data)
      });

      let allowance_fut = client.request(chain, |client| async move {
         token.allowance(client, owner, permit2).await
      });

      let (data, allowance) = tokio::try_join!(data_fut, allowance_fut)?;

      let contract_need_approval = allowance < amount;

      let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

      let expired = u64::try_from(data.expiration)? < current_time;
      let needs_new_signature = U256::from(data.amount) < amount || expired;

      #[cfg(feature = "dev")]
      {
         tracing::info!("AllowanceReturn {:?}", data);
         tracing::info!("Permit2 Expired: {}", expired);
         tracing::info!(
            "Permit2 Contract Needs Approval: {}",
            contract_need_approval
         );
         tracing::info!(
            "Permit2 Needs New Signature: {}",
            needs_new_signature
         );
      }

      let expiration = U256::from(current_time + 30 * 24 * 60 * 60); // 30 days
      let sig_deadline = U256::from(current_time + 30 * 60); // 30 minutes

      let value = if needs_new_signature {
         let v = generate_permit2_json_value(
            chain,
            token.address,
            spender,
            amount,
            permit2,
            expiration,
            sig_deadline,
            data.nonce,
         );
         Some(v)
      } else {
         None
      };

      Ok(Self {
         allowance: data,
         needs_approval: contract_need_approval,
         needs_new_signature,
         expiration,
         sig_deadline,
         msg: value,
      })
   }

   pub async fn sign(&self, signer: &SecureKey) -> Result<Signature, anyhow::Error> {
      let typed = if let Some(msg) = &self.msg {
         parse_typed_data(msg.clone())?
      } else {
         return Err(anyhow!("No message to sign"));
      };

      let signature = signer.to_signer().sign_dynamic_typed_data(&typed).await?;
      Ok(signature)
   }
}

pub fn generate_permit2_json_value(
   chain_id: u64,
   token: Address,
   spender: Address,
   amount: U256,
   permit2: Address,
   expiration: U256,
   sig_deadline: U256,
   nonce: U48,
) -> Value {
   let value = serde_json::json!({
       "types": {
           "PermitSingle": [
               {"name": "details", "type": "PermitDetails"},
               {"name": "spender", "type": "address"},
               {"name": "sigDeadline", "type": "uint256"}
           ],
           "PermitDetails": [
               {"name": "token", "type": "address"},
               {"name": "amount", "type": "uint160"},
               {"name": "expiration", "type": "uint48"},
               {"name": "nonce", "type": "uint48"}
           ],
           "EIP712Domain": [
               {"name": "name", "type": "string"},
               {"name": "chainId", "type": "uint256"},
               {"name": "verifyingContract", "type": "address"}
           ]
       },
       "domain": {
           "name": "Permit2",
           "chainId": chain_id.to_string(),
           "verifyingContract": permit2.to_string()
       },
       "primaryType": "PermitSingle",
       "message": {
           "details": {
               "token": token.to_string(),
               "amount": amount.to_string(),
               "expiration": expiration.to_string(),
               "nonce": nonce.to_string()
           },
           "spender": spender.to_string(),
           "sigDeadline": sig_deadline.to_string()
       }
   });

   value
}

pub fn parse_typed_data(json: Value) -> Result<TypedData, anyhow::Error> {
   let domain: Eip712Domain = serde_json::from_value(json["domain"].clone())?;
   let types: Eip712Types = serde_json::from_value(json["types"].clone())?;
   let resolver = Resolver::from(&types);
   let primary_type =
      json["primaryType"].as_str().ok_or(anyhow!("Missing primaryType"))?.to_string();

   let message = json["message"].clone();

   Ok(TypedData {
      domain,
      resolver,
      primary_type,
      message,
   })
}
