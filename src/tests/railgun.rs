#[cfg(test)]
mod tests {
   use std::time::Duration;

   use crate::core::ZeusCtx;
   use crate::embedded::railgun::embedded_circuits;
   use crate::utils::create_railgun_provider;
   use alloy_eips::BlockId;
   use zeus_eth::revm_utils::simulate::erc20_balance;
   use zeus_eth::utils::client::RpcClient;
   use zeus_railgun::caip::AssetId;
   use zeus_railgun::*;

   use ncrypt_me::{Credentials, secure_types::SecureString};
   use zeus_eth::alloy_primitives::{TxKind, U256};
   use zeus_eth::alloy_provider::Provider;
   use zeus_eth::{currency::ERC20Token, revm_utils::*, types::ChainId, utils::NumericValue};
   use zeus_railgun::rand;
   use zeus_wallet::Wallet;

   fn create_wallet() -> Wallet {
      let seed_phrase = "boil belt beef hunt cruel lady code dance double city young rule very sight roast make eight travel tattoo mixed you color update double";
      Wallet::new_from_mnemonic("test".into(), seed_phrase.into()).unwrap()
   }

   fn load_vault(ctx: ZeusCtx) {
      let credentials = Credentials::new(
         SecureString::from("dev"),
         SecureString::from("dev"),
         SecureString::from("dev"),
      );

      let mut vault = ctx.clone_vault();
      vault.set_credentials(credentials);

      let data = vault.decrypt(None).unwrap();
      vault.load(data).unwrap();

      ctx.set_vault(vault);
   }

   #[test]
   fn test_max_merge_inputs() {
      let prover = Groth16Prover::new(None).with_embedded_circuits(embedded_circuits());
      let max_inputs = prover.max_merge_inputs();
      assert_eq!(max_inputs, 2);
   }

   #[tokio::test]
   async fn test_resync() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt()
         .with_env_filter("info,error,debug")
         .with_test_writer()
         .init();

      let ctx = ZeusCtx::new();
      let chain = ChainId::EthereumSepolia;
      load_vault(ctx.clone());

      {
         let _ = ctx.get_railgun_provider(chain.id(), false).await?;
      }

      ctx.resync_railgun(chain.id()).await?;

      tracing::info!("Railgun Resynced, waiting for 5 seconds  ...");
      tokio::time::sleep(Duration::from_secs(5)).await;

      Ok(())
   }

   #[tokio::test]
   async fn test_sync() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt()
         .with_env_filter("info,error,debug")
         .with_test_writer()
         .init();

      let ctx = ZeusCtx::new();
      load_vault(ctx.clone());

      let chain = ChainId::EthereumSepolia;
      let _chain_config = ChainConfig::from_chain_id(chain.id()).unwrap();
      let client = ctx.get_client(chain.id()).await?;

      let wallet = create_wallet();
      let seed = wallet.seed()?;
      let signer = RailgunSigner::from_seed(&seed, 0, chain.id())?;

      let generated = ctx.write_vault(|vault| vault.ensure_railgun_db_key())?;
      assert!(!generated);
      
      let db_key = ctx.write_vault(|vault| vault.railgun_db_key())?;

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(client.clone(), chain.id(), db_key).await?;

      railgun_provider.register(signer).await?;
      railgun_provider.set_provider(client.clone());

      {
         let indexer = railgun_provider.utxo_indexer.write().await;
         indexer.rpc_syncer.set_provider(client.clone().erased()).await;
         indexer.utxo_verifier.set_provider(client.clone().erased()).await;
      }

      let latest_block = client.get_block_number().await?;
      let to_block = latest_block - 1_000_000;
      let use_subsquid = false;

      railgun_provider.sync_to(to_block, use_subsquid).await?;

      for _ in 0..100 {
         let to_block = railgun_provider.global_synced_block().await + 10_000;

         railgun_provider.sync_to(to_block, use_subsquid).await?;
      }

      Ok(())
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_shield_unshield() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt()
         .with_env_filter("info,error")
         .with_test_writer()
         .init();

      let ctx = ZeusCtx::new();
      load_vault(ctx.clone());
      let chain = ChainId::EthereumSepolia;
      let chain_config = ChainConfig::from_chain_id(chain.id()).unwrap();
      let railgun_addr = chain_config.railgun_smart_wallet;

      let wallet = create_wallet();
      let seed = wallet.seed()?;
      let signer = RailgunSigner::from_seed(&seed, 0, chain.id())?;
      let railgun_address = signer.address().clone();

      let generated = ctx.write_vault(|vault| vault.ensure_railgun_db_key())?;
      assert!(!generated);

      let db_key = ctx.read_vault(|vault| vault.railgun_db_key())?;
      let client = ctx.get_client(chain.id()).await?;

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(client.clone(), chain.id(), db_key).await?;

      railgun_provider.register(signer.clone()).await?;

      let amount = NumericValue::parse_to_wei("1", 18);
      let weth = ERC20Token::wrapped_native_token(chain.id());
      let weth_id = AssetId::Erc20(weth.address);

      let client = ctx.get_client(chain.id()).await?;

      let dummy_account = DummyAccount {
         account_type: AccountType::EOA,
         balance: U256::ZERO,
         address: wallet.address(),
         key: wallet.key.to_signer(),
      };

      eprintln!("Syncing Railgun provider");
      railgun_provider.sync().await?;

      let synced_block = railgun_provider.utxo_indexer.read().await.global_synced_block();
      eprintln!("Account synced block: {}", synced_block);

      let fork_block = BlockId::number(synced_block);
      let full_block = client.get_block(fork_block).await.unwrap();
      let timestamp = full_block.unwrap().header.timestamp;
      eprintln!("Fork block {}", synced_block);

      let mut factory =
         ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, Some(fork_block));
      factory.insert_dummy_account(dummy_account);
      factory.give_token(wallet.address(), weth.address, amount.wei()).unwrap();

      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain.into(), None, fork_db);

      // Approve the Railgun contract to spend the tokens
      evm.tx.chain_id = Some(evm.cfg.chain_id);
      evm.tx.caller = wallet.address();
      evm.tx.data = weth.encode_approve(railgun_addr, U256::MAX).into();
      evm.tx.value = U256::ZERO;
      evm.tx.kind = TxKind::Call(weth.address);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(&output);
         eprintln!("Call Reverted: {}", err);
         eprintln!("Output: {:?}", output);
         eprintln!("Gas Used: {}", res.tx_gas_used());
         panic!("Call Failed");
      } else {
         eprintln!("Token Approve Successful");
      }

      let amount_u128 = amount.wei().try_into()?;
      let mut rng = rand::rng();

      let shield_tx = railgun_provider
         .shield()
         .shield(railgun_address.clone(), weth_id, amount_u128)
         .build(&mut rng)
         .unwrap();
      let calldata = shield_tx[0].data.clone();

      // Execute the shield
      evm.tx.caller = wallet.address();
      evm.tx.data = calldata.into();
      evm.tx.value = U256::ZERO;
      evm.tx.kind = TxKind::Call(railgun_addr);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(&output);
         eprintln!("Call Reverted: {}", err);
         eprintln!("Output: {:?}", output);
         eprintln!("Gas Used: {}", res.tx_gas_used());
         panic!("Shield Call Failed");
      } else {
         eprintln!("Shield Call Successful");
         eprintln!("Gas Used: {}", res.tx_gas_used());
      }

      let logs = res.logs().to_vec();
      let to_block = synced_block + 1;
      let timestamp = timestamp + 12;

      railgun_provider
         .utxo_indexer
         .write()
         .await
         .sync_from_logs(logs, to_block, timestamp)?;

      let balances = railgun_provider.balance(railgun_address.clone()).await;
      assert_eq!(balances.len(), 1);

      let priv_balance = railgun_provider.balance_erc20(railgun_address.clone(), weth_id).await;
      let priv_balance_fmt = NumericValue::format_wei(U256::from(priv_balance), weth.decimals);

      // Expected balance after 0.25% fee
      let expected_balance = amount.calc_slippage(0.25, weth.decimals);
      println!("Private Balance: {}", priv_balance_fmt.f64());
      assert_eq!(priv_balance_fmt.wei(), expected_balance.wei());

      // Prepare the unshield transaction
      let tx_builder =
         railgun_provider
            .transact()
            .unshield(signer, wallet.address(), weth_id, priv_balance)?;

      let unshield_tx = railgun_provider.build(tx_builder, &mut rng).await?;

      // Execute the unshield transaction
      evm.tx.data = unshield_tx.tx_data.data.clone().into();

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(&output);
         eprintln!("Call Reverted: {}", err);
         eprintln!("Output: {:?}", output);
         eprintln!("Gas Used: {}", res.tx_gas_used());
         panic!("Unshield Call Failed");
      } else {
         eprintln!("Unshield Call Successful");
         eprintln!("Gas Used: {}", res.tx_gas_used());
      }

      // Expected balance after 0.25% fee
      let expected_balance = priv_balance_fmt.calc_slippage(0.25, weth.decimals);

      let weth_balance = erc20_balance(&mut evm, weth.address, wallet.address())?;
      let weth_balance_fmt = NumericValue::format_wei(U256::from(weth_balance), weth.decimals);
      println!("Weth Balance: {}", weth_balance_fmt.f64());
      assert_eq!(weth_balance_fmt.wei(), expected_balance.wei());

      // Sync the indexer
      let logs = res.logs().to_vec();
      railgun_provider
         .utxo_indexer
         .write()
         .await
         .sync_from_logs(logs, synced_block, timestamp)?;

      let priv_balance = railgun_provider.balance_erc20(railgun_address.clone(), weth_id).await;
      let priv_balance_fmt = NumericValue::format_wei(U256::from(priv_balance), weth.decimals);
      println!("Private Balance: {}", priv_balance_fmt.f64());
      assert_eq!(priv_balance_fmt.wei(), 0);

      Ok(())
   }
}
