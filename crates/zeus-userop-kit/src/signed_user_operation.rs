use alloy_primitives::{Address, Bytes};
use serde::{Deserialize, Serialize};

use crate::{abis::entry_point::encode_handle_ops, user_operation::UserOperation};

/// A signed 4337 UserOperation with an optional signed 7702 Authorization
/// ready to be sent to a bundler.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(js, derive(tsify::Tsify))]
#[cfg_attr(js, tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct SignedUserOperation {
    pub user_op: UserOperation,
    pub entry_point: Address,
}

impl SignedUserOperation {
    /// Encode `EntryPoint.handleOps([this], beneficiary)` calldata.
    ///
    /// Use this for local revm simulation of a bundler inclusion path. Empty
    /// `user_op.call_data` is normal for Railgun paymaster unshields — the real
    /// work lives in `paymasterAndData`.
    pub fn encode_handle_ops(&self, beneficiary: Address) -> Bytes {
        encode_handle_ops(&self.user_op, beneficiary)
    }
}
