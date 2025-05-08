use alloy_rpc_types::Block;
use types::ChainId;

use revm::{
   Context, MainBuilder, MainContext,
   context::{BlockEnv, CfgEnv, Evm, TxEnv},
   handler::{EthPrecompiles, instructions::EthInstructions},
   interpreter::interpreter::EthInterpreter,
   primitives::{Bytes, hardfork::SpecId},
};

use op_revm::OpSpecId;

pub use revm;
pub use op_revm;
pub use revm::{ExecuteCommitEvm, Database, DatabaseCommit, context_interface::result::ExecutionResult,  ExecuteEvm, database::InMemoryDB, interpreter::Host};

pub type Evm2<DB> = Evm<
   Context<BlockEnv, TxEnv, CfgEnv, DB>,
   (),
   EthInstructions<EthInterpreter, Context<BlockEnv, TxEnv, CfgEnv, DB>>,
   EthPrecompiles,
>;


pub mod dummy_account;
pub mod fork_db;
pub mod simulate;

pub use dummy_account::{AccountType, DummyAccount};
pub use fork_db::{ForkFactory, ForkDB};

pub fn new_evm<DB>(chain: ChainId, block: Option<&Block>, db: DB) -> Evm2<DB>
where
   DB: Database,
{
   let mut evm = Context::mainnet().with_db(db).build_mainnet();

   if let Some(block) = block {
      evm.block.number = block.header.number;
      evm.block.beneficiary = block.header.beneficiary;
      evm.block.timestamp = block.header.timestamp;
   }

   let spec = if chain.is_ethereum() {
      SpecId::PRAGUE
   } else if chain.is_optimism() || chain.is_base() {
      OpSpecId::ISTHMUS.into_eth_spec()
   } else {
      SpecId::CANCUN
   };

  // evm.cfg.chain_id = chain;
   evm.cfg.spec = spec;

   // Disable checks
   evm.cfg.disable_balance_check = true;
   evm.cfg.disable_base_fee = true;
   evm.cfg.disable_block_gas_limit = true;
   evm.cfg.disable_nonce_check = true;

   evm
}

pub fn revert_msg(bytes: &Bytes) -> String {
   if bytes.len() < 4 {
      return "0x".to_string();
   }
   let error_data = &bytes[4..];

   match String::from_utf8(error_data.to_vec()) {
      Ok(s) => s.trim_matches(char::from(0)).to_string(),
      Err(_) => "0x".to_string(),
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   #[test]
   fn test_revert_msg() {
      let msg_str = "This is a test message";
      let msg_bytes = Bytes::from(msg_str.as_bytes());
      let msg = revert_msg(&msg_bytes);
      assert_eq!(msg, msg_str);

      let empty_msg = "";
      let empty_msg_bytes = Bytes::from(empty_msg.as_bytes());
      let msg = revert_msg(&empty_msg_bytes);
      assert_eq!(msg, "0x");
   }
}
