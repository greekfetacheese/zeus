use crate::types::ChainId;
use alloy_rpc_types::Block;
use alloy_sol_types::decode_revert_reason;

use revm::{
   Context, MainBuilder, MainContext,
   context::{BlockEnv, CfgEnv, Evm, TxEnv},
   handler::{EthFrame, EthPrecompiles, instructions::EthInstructions},
   interpreter::interpreter::EthInterpreter,
   primitives::{Bytes, U256, hardfork::SpecId},
};

pub use revm;
pub use revm::{
   Database, DatabaseCommit, ExecuteCommitEvm, ExecuteEvm,
   context_interface::result::{ExecutionResult, Output},
   database::InMemoryDB,
   interpreter::Host,
};

pub type Evm2<DB> = Evm<
   Context<BlockEnv, TxEnv, CfgEnv, DB>,
   (),
   EthInstructions<EthInterpreter, Context<BlockEnv, TxEnv, CfgEnv, DB>>,
   EthPrecompiles,
   EthFrame,
>;

pub mod dummy_account;
pub mod fork_db;
pub mod simulate;

pub use dummy_account::{AccountType, DummyAccount};
pub use fork_db::{ForkDB, ForkFactory};

pub fn new_evm<DB>(chain: ChainId, block: Option<&Block>, db: DB) -> Evm2<DB>
where
   DB: Database,
{
   let mut evm = Context::mainnet().with_db(db).build_mainnet();

   if let Some(block) = block {
      evm.block.number = U256::from(block.header.number);
      evm.block.beneficiary = block.header.beneficiary;
      evm.block.timestamp = U256::from(block.header.timestamp);
   }

   let spec = if chain.is_ethereum() {
      SpecId::OSAKA
   } else {
      SpecId::PRAGUE
   };

   evm.cfg.chain_id = chain.id();
   evm.cfg.spec = spec;

   // Disable checks
   evm.cfg.disable_balance_check = true;
   evm.cfg.disable_base_fee = true;
   evm.cfg.disable_block_gas_limit = true;
   evm.cfg.disable_nonce_check = true;

   evm
}

pub fn revert_msg(bytes: &Bytes) -> String {
   if let Some(msg) = decode_revert_reason(bytes) {
      return msg;
   }
   if let Some(msg) = crate::abi::permit::decode_permit2_revert(bytes) {
      return msg;
   }
   if bytes.is_empty() {
      return "empty revert data".to_string();
   }
   format!("0x{}", alloy_primitives::hex::encode(bytes))
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::hex;
   use alloy_sol_types::SolError;

   #[test]
   fn test_revert_msg_with_data() {
      let prefix = hex::decode("08c379a0").unwrap();
      let mut full_revert_data = prefix;
      full_revert_data.extend_from_slice(b"This is a test message");

      let revert_bytes = Bytes::from(full_revert_data);
      let msg = revert_msg(&revert_bytes);

      // Not ABI-encoded Error(string) — fall back to hex.
      assert_eq!(msg, format!("0x{}", hex::encode(revert_bytes.as_ref())));
   }

   #[test]
   fn test_revert_msg_too_short() {
      let short_bytes = Bytes::from(vec![1, 2, 3]);
      let msg = revert_msg(&short_bytes);
      assert!(!msg.is_empty());
   }

   #[test]
   fn test_revert_msg_with_invalid_utf8() {
      let invalid_utf8_payload = Bytes::from(vec![0x08, 0xc3, 0x79, 0xa0, 0xf0, 0x9f, 0x92]);
      let msg = revert_msg(&invalid_utf8_payload);
      assert_eq!(msg, format!("0x{}", hex::encode(&invalid_utf8_payload)));
   }

   #[test]
   fn test_revert_msg_permit2_invalid_signer() {
      let selector = crate::abi::permit::Permit2::InvalidSigner::SELECTOR;
      let msg = revert_msg(&Bytes::from(selector.to_vec()));
      assert_eq!(msg, "InvalidSigner()");
   }
}
