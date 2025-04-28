use alloy_primitives::{Address, U256};
use alloy_signer_local::PrivateKeySigner;

use revm::state::Bytecode;


#[derive(Clone, Debug)]
pub enum AccountType {
   /// Externally Owned Account
   EOA,

   /// An Ethereum Smart Contract
   Contract(Bytecode),
}

/// Represents a dummy account we want to insert into the fork enviroment
#[derive(Clone, Debug)]
pub struct DummyAccount {
   pub account_type: AccountType,
   pub balance: U256,
   pub address: Address,
   pub key: PrivateKeySigner,
}

impl DummyAccount {
   pub fn new(account_type: AccountType, balance: U256) -> Self {
      let key = PrivateKeySigner::random();
      Self {
         account_type,
         balance,
         address: key.address(),
         key,
      }
   }
}