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

use op_revm::OpSpecId;

pub use op_revm;
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
      SpecId::PRAGUE
   } else if chain.is_optimism() || chain.is_base() {
      OpSpecId::ISTHMUS.into_eth_spec()
   } else {
      SpecId::CANCUN
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
   decode_revert_reason(bytes).unwrap_or_else(|| "Failed to decode revert reason".to_string())
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::hex;

   #[test]
   fn test_revert_msg_with_data() {
      let msg_str = "This is a test message";
      let prefix = hex::decode("08c379a0").unwrap();

      let mut full_revert_data = prefix;
      full_revert_data.extend_from_slice(msg_str.as_bytes());

      let revert_bytes = Bytes::from(full_revert_data);
      let msg = revert_msg(&revert_bytes);

      assert_eq!(
         msg, msg_str,
         "Should extract the message after the 4-byte selector"
      );
   }

   #[test]
   fn test_revert_msg_too_short() {
      let short_bytes = Bytes::from(vec![1, 2, 3]);
      let msg = revert_msg(&short_bytes);
      assert_eq!(msg, "0x", "Should return '0x' for data less than 4 bytes");
   }

   #[test]
   fn test_revert_msg_with_invalid_utf8() {
      let invalid_utf8_payload = Bytes::from(vec![0x08, 0xc3, 0x79, 0xa0, 0xf0, 0x9f, 0x92]);
      let msg = revert_msg(&invalid_utf8_payload);
      assert_eq!(
         msg, "0x",
         "Should return '0x' if the data part is not valid UTF-8"
      );
   }
}
