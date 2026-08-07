use alloy_eips::eip7702::SignedAuthorization;
use serde::{Deserialize, Serialize};
use zeus_eth::alloy_primitives::Address;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EOADelegateParams {
   pub chain: u64,
   pub eoa: Address,
   pub address: Address,
   pub nonce: u64,
}

impl EOADelegateParams {
   pub fn new(chain: u64, eoa: Address, auth: SignedAuthorization) -> Self {
      Self {
         chain,
         eoa,
         address: auth.address().clone(),
         nonce: auth.nonce(),
      }
   }
}
