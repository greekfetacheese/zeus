pub mod simulate;
// pub mod simulate_position;
pub mod swap_quoter;
pub mod universal_router_v2;
pub mod zeus_delegate;

use crate::core::ZeusCtx;

use zeus_eth::{
   abi::permit::{Permit2::allowanceReturn, allowance},
   alloy_primitives::{Address, Signature, U256},
   alloy_signer::Signer,
   currency::ERC20Token,
   utils::{SecureSigner, address_book, generate_permit2_json_value, parse_typed_data},
};

use anyhow::anyhow;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// UNIX time in X days from now
pub fn get_unix_time_from_days(days: u64) -> Result<u64, anyhow::Error> {
   let now = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();

   Ok(now + 86400 * days)
}

/// UNIX time in X minutes from now
pub fn get_unix_time_from_minutes(minutes: u64) -> Result<u64, anyhow::Error> {
   let now = SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();

   Ok(now + 60 * minutes)
}

/// Info for a token approval through the Permit2 contract
#[derive(Clone)]
pub struct Permit2Details {
   /// The allowance details from the Permit2 contract for a token
   pub allowance: allowanceReturn,

   /// Whether the Permit2 contract needs to be approved to spend the token
   ///
   /// Usually we do this approval one-time with unlimited allowance
   pub permit2_needs_approval: bool,

   /// Whether we need to sign again an approval
   pub needs_new_signature: bool,

   /// When this approval should expire
   pub expiration: U256,

   /// When this signature should expire
   pub sig_deadline: U256,

   /// The message to be signed
   pub msg: Option<Value>,
}

impl Permit2Details {
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
      let permit2_contract_need_approval = allowance < amount;

      let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

      let expired = u64::try_from(data.expiration)? < current_time;
      let needs_new_signature = U256::from(data.amount) < amount || expired;

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
         permit2_needs_approval: permit2_contract_need_approval,
         needs_new_signature,
         expiration,
         sig_deadline,
         msg: value,
      })
   }

   pub async fn sign(&self, signer: &SecureSigner) -> Result<Signature, anyhow::Error> {
      let typed = if let Some(msg) = &self.msg {
         parse_typed_data(msg.clone())?
      } else {
         return Err(anyhow!("No message to sign"));
      };

      let signature = signer.to_signer().sign_dynamic_typed_data(&typed).await?;
      Ok(signature)
   }
}
