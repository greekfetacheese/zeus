use alloy_primitives::{Address, Bytes, U256, address, aliases::U192, bytes};
use alloy_provider::{Provider, network::Ethereum};
use alloy_rpc_types::Authorization;

use alloy_sol_types::{Eip712Domain, SolCall};
use serde::{Deserialize, Serialize};

use crate::{
   abis::entry_point::EntryPoint,
   entry_point::{ENTRY_POINT_08, entry_point_08_domain},
   smart_account::SmartAccount,
};

/// Creates a simple smart account.
///
/// Defaults to the v0.8 EntryPoint and the eth-infinitism Simple7702Account implementation at
/// `0xe6Cae83BdE06E4c305530e199D7217f42808555B`.
#[derive(Clone)]
pub struct SimpleSmartAccount<P> {
   owner: Address,
   chain_id: u64,
   provider: P,

   implementation: Address,
   entry_point: Address,
   domain: Eip712Domain,
   dummy_signature: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(js, derive(tsify::Tsify))]
#[cfg_attr(js, tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Call {
   #[cfg_attr(js, tsify(type = "`0x${string}`"))]
   pub target: Address,
   #[cfg_attr(js, tsify(type = "`0x${string}`"))]
   pub value: U256,
   #[cfg_attr(js, tsify(type = "`0x${string}`"))]
   pub data: Bytes,
}

impl<P> SimpleSmartAccount<P>
where
   P: Provider<Ethereum>,
{
   pub fn new(owner: Address, chain_id: u64, provider: P) -> Self {
      let implementation = address!("0xe6Cae83BdE06E4c305530e199D7217f42808555B");
      let entry_point = ENTRY_POINT_08;
      let domain = entry_point_08_domain(chain_id);
      let dummy_signature = bytes!(
         "0xfffffffffffffffffffffffffffffff0000000000000000000000000000000007aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1c"
      );

      Self {
         owner,
         chain_id,
         provider,
         implementation,
         entry_point,
         domain,
         dummy_signature,
      }
   }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl<P> SmartAccount for SimpleSmartAccount<P>
where
   P: Provider<Ethereum>,
{
   type CallData = Vec<Call>;

   fn entry_point(&self) -> Address {
      self.entry_point
   }

   fn domain(&self) -> Eip712Domain {
      self.domain.clone()
   }

   fn address(&self) -> Address {
      self.owner
   }

   async fn nonce(&self) -> Result<U256, anyhow::Error> {
      let call = EntryPoint::getNonceCall::new((self.owner, U192::from(0)));
      let calldata = call.abi_encode();

      use alloy_rpc_types::TransactionRequest;
      let tx = TransactionRequest::default().to(self.entry_point).input(calldata.into());

      let result = self.provider.call(tx).await?;
      // Decode the single U256 return value
      let nonce =
         <EntryPoint::getNonceCall as alloy_sol_types::SolCall>::abi_decode_returns(&result)?;
      Ok(nonce)
   }

   async fn authorization(&self) -> Result<Authorization, anyhow::Error> {
      let nonce = self.owner_nonce().await?;

      Ok(Authorization {
         chain_id: U256::from(self.chain_id),
         address: self.implementation,
         nonce,
      })
   }

   fn dummy_signature(&self) -> Bytes {
      self.dummy_signature.clone()
   }

   fn encode_call_data(&self, call_data: Self::CallData) -> Bytes {
      if call_data.is_empty() {
         // If no calls, return empty data to save gas.
         return Bytes::new();
      }

      let calls = call_data
         .into_iter()
         .map(|call| abi::BaseAccount::Call {
            target: call.target,
            value: call.value,
            data: call.data,
         })
         .collect();

      abi::BaseAccount::executeBatchCall::new((calls,)).abi_encode().into()
   }
}

impl<P: Provider<Ethereum>> SimpleSmartAccount<P> {
   /// Gets the nonce for the owner address
   async fn owner_nonce(&self) -> Result<u64, anyhow::Error> {
      let nonce: u64 = self.provider.get_transaction_count(self.owner).await? as u64;
      Ok(nonce)
   }
}

mod abi {
   use alloy_sol_types::sol;

   sol!(
       contract BaseAccount {
           struct Call {
               address target;
               uint256 value;
               bytes data;
           }

           function executeBatch(Call[] calldata calls) external;
       }

   );
}
