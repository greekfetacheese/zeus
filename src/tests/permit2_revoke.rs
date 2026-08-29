#[cfg(test)]
mod tests {
   use crate::core::signature::{generate_permit2_json_value, parse_typed_data};
   use crate::utils::simulate::simulate_transaction;

   use zeus_eth::{
      abi::permit::encode_permit_single_call,
      alloy_primitives::{U256, aliases::U48, hex},
      alloy_provider::Provider,
      alloy_rpc_types::BlockId,
      alloy_signer::Signer,
      currency::ERC20Token,
      revm_utils::*,
      types::ChainId,
      utils::address_book,
   };

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn permit2_revoke_sim_on_fork() {
      let ctx = crate::tests::unlock_ctx();
      let chain_id = 1u64;
      let chain = ChainId::from(chain_id);
      let client = ctx.get_client(chain_id).await.expect("rpc client");

      let permit2 = address_book::permit2_contract(chain_id).unwrap();
      let token = ERC20Token::usdc().address;
      let spender = address_book::universal_router_v2(chain_id).unwrap();

      let owner = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let amount = U256::ZERO;
      let expiration = U256::ZERO;
      let now = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap()
         .as_secs();
      let sig_deadline = U256::from(now + 30 * 60);

      let json = generate_permit2_json_value(
         chain_id,
         token,
         spender,
         amount,
         permit2,
         expiration,
         sig_deadline,
         U48::ZERO,
      );
      let typed = parse_typed_data(json).expect("typed data");
      let signature = owner.key.sign_dynamic_typed_data(&typed).await.expect("sign");

      let calldata = encode_permit_single_call(
         owner.address,
         token,
         amount,
         expiration,
         U48::ZERO,
         spender,
         sig_deadline,
         signature,
      );
      eprintln!(
         "calldata selector: 0x{}",
         hex::encode(&calldata[..4])
      );

      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(owner.clone());
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain, Some(&block), fork_db);

      let sim = simulate_transaction(
         &mut evm,
         owner.address,
         permit2,
         calldata,
         U256::ZERO,
         vec![],
      )
      .expect("permit2 revoke with matching signer should simulate");
      eprintln!("sim success gas={}", sim.tx_gas_used());

      let other = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let json_bad = generate_permit2_json_value(
         chain_id,
         token,
         spender,
         amount,
         permit2,
         expiration,
         sig_deadline,
         U48::from(1u64),
      );
      let typed_bad = parse_typed_data(json_bad).expect("typed data bad");
      let sig_bad = other.key.sign_dynamic_typed_data(&typed_bad).await.expect("sign bad");
      let calldata_bad = encode_permit_single_call(
         owner.address,
         token,
         amount,
         expiration,
         U48::from(1u64),
         spender,
         sig_deadline,
         sig_bad,
      );
      let err = simulate_transaction(
         &mut evm,
         owner.address,
         permit2,
         calldata_bad,
         U256::ZERO,
         vec![],
      )
      .expect_err("wrong signer should revert");
      let err = err.to_string();
      assert!(
         err.contains("InvalidSigner()"),
         "expected InvalidSigner in {err}"
      );
   }
}
