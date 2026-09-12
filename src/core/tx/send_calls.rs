//! EIP-5792 `wallet_sendCalls` — single calls as a normal tx, batches via EIP-7702.

use super::TransactionRich;
use super::send::{send_transaction, signed_7702_authorization};
use crate::core::ZeusCtx;
use anyhow::anyhow;
use userop_kit::smart_account::simple_smart_account::SIMPLE_7702_ACCOUNT;
use zeus_eth::{
   alloy_primitives::{Address, Bytes, U256},
   alloy_provider::Provider,
   alloy_rpc_types::TransactionReceipt,
   alloy_sol_types::SolCall,
   types::ChainId,
};

/// One call from an EIP-5792 `wallet_sendCalls` request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalletCall {
   pub to: Address,
   pub data: Bytes,
   pub value: U256,
}

mod abi {
   use zeus_eth::alloy_sol_types::sol;

   sol! {
       contract BaseAccount {
           struct Call {
               address target;
               uint256 value;
               bytes data;
           }

           function executeBatch(Call[] calldata calls) external;
       }
   }
}

pub fn encode_execute_batch(calls: &[WalletCall]) -> Bytes {
   let calls = calls
      .iter()
      .map(|call| abi::BaseAccount::Call {
         target: call.to,
         value: call.value,
         data: call.data.clone(),
      })
      .collect();
   abi::BaseAccount::executeBatchCall::new((calls,)).abi_encode().into()
}

/// Send one or more dapp calls. Multiple calls are packed into a single EIP-7702
/// `executeBatch` on [`SIMPLE_7702_ACCOUNT`].
pub async fn send_wallet_calls(
   ctx: ZeusCtx,
   dapp: String,
   chain: ChainId,
   from: Address,
   calls: Vec<WalletCall>,
) -> Result<(TransactionReceipt, TransactionRich), anyhow::Error> {
   if calls.is_empty() {
      return Err(anyhow!("no calls"));
   }

   if calls.len() == 1 {
      let call = &calls[0];
      return send_transaction(
         ctx,
         dapp,
         None,
         chain,
         true,
         from,
         call.to,
         call.data.clone(),
         call.value,
         Vec::new(),
      )
      .await;
   }

   if !chain.supports_eip7702() {
      return Err(anyhow!(
         "atomic batching requires EIP-7702, which this network does not support"
      ));
   }

   let (call_data, authorization_list) =
      prepare_7702_batch(ctx.clone(), chain, from, &calls).await?;

   let result = send_transaction(
      ctx.clone(),
      dapp,
      None,
      chain,
      true,
      from,
      from,
      call_data,
      U256::ZERO,
      authorization_list.clone(),
   )
   .await?;

   if !authorization_list.is_empty() {
      ctx.write(|ctx| {
         ctx.delegated_wallets.add(chain.id(), from, SIMPLE_7702_ACCOUNT);
      });
   }

   Ok(result)
}

async fn prepare_7702_batch(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   calls: &[WalletCall],
) -> Result<
   (
      Bytes,
      Vec<alloy_eips::eip7702::SignedAuthorization>,
   ),
   anyhow::Error,
> {
   let call_data = encode_execute_batch(calls);

   let delegated = match ctx.check_delegated_wallet_status(chain.id(), from).await {
      Ok(()) => ctx.read(|ctx| ctx.delegated_wallets.get(chain.id(), from)),
      Err(e) => {
         tracing::warn!("Could not check EIP-7702 delegation: {:?}", e);
         None
      }
   };

   if delegated == Some(SIMPLE_7702_ACCOUNT) {
      return Ok((call_data, Vec::new()));
   }

   let wallet = ctx.get_wallet(from).ok_or(anyhow!("Wallet not found"))?;
   let client = ctx.get_zeus_client();
   let nonce = client
      .request(chain.id(), |client| async move {
         client.get_transaction_count(from).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   // Sender nonce is incremented before the authorization list is applied.
   let auth = signed_7702_authorization(
      &wallet.key,
      chain.id(),
      SIMPLE_7702_ACCOUNT,
      nonce + 1,
   )?;

   Ok((call_data, vec![auth]))
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::str::FromStr;

   #[test]
   fn encode_execute_batch_uses_execute_batch_selector() {
      let to = Address::from_str("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").unwrap();
      let encoded = encode_execute_batch(&[WalletCall {
         to,
         data: Bytes::from_str("0x095ea7b3").unwrap(),
         value: U256::ZERO,
      }]);
      assert_eq!(
         &encoded[..4],
         abi::BaseAccount::executeBatchCall::SELECTOR
      );
      assert!(encoded.len() > 4);
   }
}
