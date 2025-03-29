use alloy_rpc_types::Block;

use revm::{
   Context, Database, MainBuilder, MainContext,
   context::{BlockEnv, CfgEnv, Evm, TxEnv},
   handler::{EthPrecompiles, instructions::EthInstructions},
   interpreter::interpreter::EthInterpreter,
   primitives::{Bytes, hardfork::SpecId},
};

pub use revm;

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
pub use fork_db::ForkFactory;

pub fn new_evm<DB>(chain: u64, block: Option<Block>, db: DB) -> Evm2<DB>
where
   DB: Database,
{
   let mut evm = Context::mainnet().with_db(db).build_mainnet();

   let spec = match chain {
      1 => SpecId::PRAGUE,
      _ => SpecId::CANCUN,
   };

   evm.cfg.spec = spec;

   if let Some(block) = block {
      evm.block.number = block.header.number;
      evm.block.beneficiary = block.header.beneficiary;
      evm.block.timestamp = block.header.timestamp;
   }

   // Disable checks
   evm.cfg.disable_balance_check = true;
   evm.cfg.disable_base_fee = true;
   evm.cfg.disable_block_gas_limit = true;
   evm.cfg.disable_nonce_check = true;

   evm
}

pub fn revert_msg(bytes: &Bytes) -> &str {
   if bytes.len() < 4 {
      return "EVM Returned 0x (Empty Bytes)";
   }
   let error_data = &bytes[4..];

   match String::from_utf8(error_data.to_vec()) {
      Ok(s) => Box::leak(s.trim_matches(char::from(0)).to_string().into_boxed_str()),
      Err(_) => "EVM Returned 0x (Empty Bytes)",
   }
}
