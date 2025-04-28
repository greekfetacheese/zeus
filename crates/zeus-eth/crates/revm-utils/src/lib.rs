use alloy_rpc_types::Block;

use revm::{
   Context, Database, MainBuilder, MainContext,
   context::{BlockEnv, CfgEnv, Evm, TxEnv},
   handler::{EthPrecompiles, instructions::EthInstructions},
   interpreter::interpreter::EthInterpreter,
   primitives::{Bytes, hardfork::SpecId},
};

pub use revm;
pub use revm::{ExecuteEvm, interpreter::Host, ExecuteCommitEvm, database::InMemoryDB};

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
   use alloy_primitives::{Address, TxKind, U256, address};
   use alloy_provider::ProviderBuilder;
   use alloy_sol_types::{SolCall, sol};
   use revm::{ExecuteEvm, state::Bytecode};

   sol! {
       contract Revert {
         function revert_test() pure public {
            revert("Revert Message");
         }
     }
   }

   #[test]
   fn test_revert_msg_simple() {
      let msg_str = "This is a test message";
      let msg_bytes = Bytes::from(msg_str.as_bytes());
      let msg = revert_msg(&msg_bytes);
      assert_eq!(msg, msg_str);

      let empty_msg = "";
      let empty_msg_bytes = Bytes::from(empty_msg.as_bytes());
      let msg = revert_msg(&empty_msg_bytes);
      assert_eq!(msg, "0x");
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_revert_msg() {
      let url = "https://eth.merkle.io".parse().unwrap();
      let client = ProviderBuilder::new().on_http(url);

      let bytecode_str = "0x6080604052348015600e575f5ffd5b50600436106026575f3560e01c80639ca24dff14602a575b5f5ffd5b60306032565b005b6040517f08c379a000000000000000000000000000000000000000000000000000000000815260040160629060c1565b60405180910390fd5b5f82825260208201905092915050565b7f526576657274204d6573736167650000000000000000000000000000000000005f82015250565b5f60ad600e83606b565b915060b682607b565b602082019050919050565b5f6020820190508181035f83015260d68160a3565b905091905056fea26469706673582212205e666dc5e21b806133bd01b882c49cfafbaaa1e1c0dec00e607d9a9a77cc6efd64736f6c634300081c0033";
      let bytecode = Bytecode::new_raw(bytecode_str.parse().unwrap());
      let dummy_contract = DummyAccount::new(AccountType::Contract(bytecode), U256::ZERO);

      let factory = ForkFactory::new_sandbox_factory(client, 1, None, None);

      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(1, None, fork_db);

      let c = Revert::revert_testCall {};
      let data = c.abi_encode();

      evm.tx.caller = Address::ZERO;
      evm.tx.data = data.into();
      evm.tx.value = U256::ZERO;
      evm.tx.kind = TxKind::Call(dummy_contract.address);

      let res = evm.transact(evm.tx.clone()).unwrap();
      println!("Result {:#?}", res.result);

      let output = res.result.output().unwrap();

      if !res.result.is_success() {
         let err = revert_msg(&output);
         println!("Call Reverted: {}", err);
      }
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   #[should_panic]
   async fn test_revert_msg_simulate() {
      let url = "https://eth.merkle.io".parse().unwrap();
      let client = ProviderBuilder::new().on_http(url);

     // let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
      let usdc = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
      let dummy = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let recipient = address!("0x000000000000000000000000000000000000dEaD");
      let amount = U256::from(1000000000000000000_u128);

      let factory = ForkFactory::new_sandbox_factory(client.clone(), 1, None, None);
     // dummy.insert(&mut factory, weth, amount).unwrap();
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(1, None, fork_db);

      super::simulate::transfer_token(&mut evm, usdc, dummy.address, recipient, amount, false).unwrap();
   }
}
