use alloy_eips::eip7702::{Authorization as AlloyAuthorization, SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, U256};
use alloy_rpc_types::{Log, ReceiptWithBloom, TransactionReceipt};

use serde::{Deserialize, Serialize};

use crate::abis::entry_point::EntryPoint::PackedUserOperation;

/// ERC-4337 UserOperation in unpacked JSON-RPC wire format.
///
/// EntryPoint 0.7 & 0.8
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserOperation {
   pub sender: Address,

   pub nonce: U256,

   #[serde(skip_serializing_if = "Option::is_none")]
   pub factory: Option<Address>,

   #[serde(skip_serializing_if = "Option::is_none")]
   pub factory_data: Option<Bytes>,

   pub call_data: Bytes,

   #[serde(with = "alloy_serde::quantity")]
   pub call_gas_limit: u128,

   #[serde(with = "alloy_serde::quantity")]
   pub verification_gas_limit: u128,

   #[serde(with = "alloy_serde::quantity")]
   pub pre_verification_gas: u128,

   #[serde(with = "alloy_serde::quantity")]
   pub max_fee_per_gas: u128,

   #[serde(with = "alloy_serde::quantity")]
   pub max_priority_fee_per_gas: u128,

   #[serde(skip_serializing_if = "Option::is_none")]
   pub paymaster: Option<Address>,

   #[serde(
      default,
      skip_serializing_if = "Option::is_none",
      with = "alloy_serde::quantity::opt"
   )]
   pub paymaster_verification_gas_limit: Option<u128>,

   #[serde(
      default,
      skip_serializing_if = "Option::is_none",
      with = "alloy_serde::quantity::opt"
   )]
   pub paymaster_post_op_gas_limit: Option<u128>,

   #[serde(skip_serializing_if = "Option::is_none")]
   pub paymaster_data: Option<Bytes>,

   #[serde(
      rename = "eip7702Auth",
      default,
      skip_serializing_if = "Authorization::is_none"
   )]
   pub authorization: Authorization,

   pub signature: Bytes,
}

/// Eip-7702 authorization
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Authorization {
   #[default]
   None,
   SignedEip7702(SignedAuthorization),
   Eip7702(AlloyAuthorization),
}

impl Authorization {
   fn is_none(&self) -> bool {
      matches!(self, Authorization::None)
   }
}

/// A submitted user operation hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserOperationHash(pub B256);

/// Gas estimates returned by `eth_estimateUserOperationGas`.
///
/// EntryPoint 0.7 & 0.8
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOperationGasEstimate {
   #[serde(with = "alloy_serde::quantity")]
   pub pre_verification_gas: u128,

   #[serde(with = "alloy_serde::quantity")]
   pub verification_gas_limit: u128,

   #[serde(with = "alloy_serde::quantity")]
   pub call_gas_limit: u128,

   #[serde(default, with = "alloy_serde::quantity::opt")]
   pub paymaster_verification_gas_limit: Option<u128>,

   #[serde(default, with = "alloy_serde::quantity::opt")]
   pub paymaster_post_op_gas_limit: Option<u128>,

   #[serde(with = "alloy_serde::quantity")]
   pub max_fee_per_gas: u128,

   #[serde(with = "alloy_serde::quantity")]
   pub max_priority_fee_per_gas: u128,
}

/// Receipt returned by `eth_getUserOperationReceipt`.
///
/// EntryPoint 0.7 & 0.8
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOperationReceipt {
   pub entry_point: Address,

   pub user_op_hash: B256,

   pub sender: Address,

   pub nonce: U256,

   pub actual_gas_used: U256,

   pub actual_gas_cost: U256,

   pub success: bool,

   #[serde(skip_serializing_if = "Option::is_none")]
   pub reason: Option<Bytes>,
   pub logs: Vec<Log>,
   pub receipt: TransactionReceipt<ReceiptWithBloom>,
}

impl UserOperation {
   pub fn into_packed(&self) -> PackedUserOperation {
      self.into()
   }

   /// Returns the total gas limit for this UserOperation, including paymaster gas limits if
   /// applicable.
   pub fn total_gas_limit(&self) -> u128 {
      let mut total = self.pre_verification_gas + self.verification_gas_limit + self.call_gas_limit;
      if let Some(paymaster_verification_gas_limit) = self.paymaster_verification_gas_limit {
         total += paymaster_verification_gas_limit;
      }
      if let Some(paymaster_post_op_gas_limit) = self.paymaster_post_op_gas_limit {
         total += paymaster_post_op_gas_limit;
      }
      total
   }
}
