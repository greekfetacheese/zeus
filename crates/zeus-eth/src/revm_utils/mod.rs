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

/// Build the EVM a simulation runs on.
///
/// `spec` is the chain's hardfork as the chain's own node reports it — `eth_config`, resolved
/// through [`ChainId::spec_from_node`]. `None` means the node did not answer, the chain's schedule
/// is not one Zeus tracks, or the fork is not one Zeus can place, and it falls back to
/// [`ChainId::spec`], the fork Zeus ships. Either answer has to arrive here: the spec is captured
/// when the EVM is built (see below), so it cannot be corrected later.
pub fn new_evm<DB>(chain: ChainId, block: Option<&Block>, db: DB, spec: Option<SpecId>) -> Evm2<DB>
where
   DB: Database,
{
   let spec = spec.unwrap_or_else(|| chain.spec());

   // The spec goes on the `CfgEnv` *before* `build_mainnet()`: that is where the instruction set
   // (and its static gas table) captures the spec, and `set_spec_and_mainnet_gas_params` is what
   // turns on the Amsterdam repricings. Assigning `cfg.spec` directly leaves EIP-8037 state
   // creation gas and the EIP-2780 intrinsic model off, so the simulation prices a transaction
   // that creates state with pre-Amsterdam rules.
   let mut evm = Context::mainnet()
      .with_db(db)
      .modify_cfg_chained(|cfg| cfg.set_spec_and_mainnet_gas_params(spec))
      .build_mainnet();

   if let Some(block) = block {
      evm.block.number = U256::from(block.header.number);
      evm.block.beneficiary = block.header.beneficiary;
      evm.block.timestamp = U256::from(block.header.timestamp);
   }

   evm.cfg.chain_id = chain.id();

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
   use alloy_primitives::{Address, TxKind};
   use alloy_sol_types::SolError;

   /// EIP-7708 arrives with the spec and nothing else: a chain whose fork has it emits an ETH
   /// transfer log for a value move, and a chain whose fork does not stays silent. This is the
   /// gating the ETH transfer event in the tx analysis rests on.
   #[test]
   fn the_simulated_spec_decides_whether_an_eth_transfer_logs() {
      let log_count = |chain: ChainId, spec: Option<SpecId>, value: u64| {
         let mut evm = new_evm(chain, None, InMemoryDB::default(), spec);
         evm.tx.caller = Address::repeat_byte(0x11);
         evm.tx.chain_id = Some(chain.id());
         evm.tx.kind = TxKind::Call(Address::repeat_byte(0x22));
         evm.tx.value = U256::from(value);
         evm.tx.gas_limit = 1_000_000;
         evm.tx.gas_price = 1;

         evm.transact(evm.tx.clone()).unwrap().result.logs().len()
      };

      assert_eq!(log_count(ChainId::EthereumSepolia, None, 1), 1);
      assert_eq!(log_count(ChainId::Ethereum, None, 1), 0);
      // A value move to a different account is what logs; zero reaches nobody.
      assert_eq!(log_count(ChainId::EthereumSepolia, None, 0), 0);
      // What a node reports is what the simulation runs: mainnet told to simulate on Amsterdam
      // logs the transfer its chain would, before mainnet has the fork.
      assert_eq!(
         log_count(ChainId::Ethereum, Some(SpecId::AMSTERDAM), 1),
         1
      );
   }

   #[test]
   fn test_revert_msg_with_data() {
      let prefix = hex::decode("08c379a0").unwrap();
      let mut full_revert_data = prefix;
      full_revert_data.extend_from_slice(b"This is a test message");

      let revert_bytes = Bytes::from(full_revert_data);
      let msg = revert_msg(&revert_bytes);

      // Not ABI-encoded Error(string) — fall back to hex.
      assert_eq!(
         msg,
         format!("0x{}", hex::encode(revert_bytes.as_ref()))
      );
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
      assert_eq!(
         msg,
         format!("0x{}", hex::encode(&invalid_utf8_payload))
      );
   }

   #[test]
   fn test_revert_msg_permit2_invalid_signer() {
      let selector = crate::abi::permit::Permit2::InvalidSigner::SELECTOR;
      let msg = revert_msg(&Bytes::from(selector.to_vec()));
      assert_eq!(msg, "InvalidSigner()");
   }
}
