#[cfg(test)]
mod tests {
   use std::sync::Arc;

   use crate::core::{ZeusCtx, railgun_db_file, railgun_dir};
   use alloy_eips::BlockId;
   use zeus_eth::revm_utils::simulate::erc20_balance;
   use zeus_eth::utils::client::RpcClient;
   use zeus_railgun::caip::AssetId;
   use zeus_railgun::*;

   use zeus_eth::alloy_primitives::{TxKind, U256};
   use zeus_eth::alloy_provider::Provider;
   use zeus_eth::{currency::ERC20Token, revm_utils::*, utils::NumericValue};
   use zeus_railgun::rand;
   use zeus_wallet::Wallet;

   fn create_wallet() -> Wallet {
      let seed_phrase = "boil belt beef hunt cruel lady code dance double city young rule very sight roast make eight travel tattoo mixed you color update double";
      Wallet::new_from_mnemonic("test".into(), seed_phrase.into()).unwrap()
   }

   async fn create_railgun_provider(
      ctx: ZeusCtx,
      chain: u64,
   ) -> Result<RailgunProvider<RpcClient>, anyhow::Error> {
      let db_file = railgun_db_file(chain)?;
      let railgun_dir = railgun_dir()?;

      let client = ctx.get_client(chain).await?;

      let chain_config = ChainConfig::mainnet();
      let utxo_verifier = RootVerifier::new(client.clone(), chain_config.railgun_smart_wallet);
      let rpc_syncer = Syncer::new(client.clone(), chain_config.railgun_smart_wallet);
      let subsquid_syncer: Option<Arc<dyn UtxoSyncer>> = Some(Arc::new(SubsquidSyncer::new(
         &chain_config.subsquid_endpoint,
      )));

      let database = RedbDatabase::new(db_file)?;
      let utxo_indexer = UtxoIndexer::new(
         Arc::new(database),
         Arc::new(rpc_syncer),
         subsquid_syncer,
         Arc::new(utxo_verifier),
      )
      .await?;
      let prover = Groth16Prover::new(Some(railgun_dir));

      let railgun_provider = RailgunProvider::new(
         chain_config,
         client.clone(),
         utxo_indexer,
         prover,
         None,
      )
      .await?;

      Ok(railgun_provider)
   }

   #[tokio::test]
   async fn test_sync() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt().with_env_filter("info,error,debug").init();

      let ctx = ZeusCtx::new();
      let chain = 1;
      let _chain_config = ChainConfig::mainnet();

      let wallet = create_wallet();
      let seed = wallet.seed()?;
      let signer = RailgunSigner::from_seed(&seed, 0, chain)?;

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx.clone(), chain).await?;

      railgun_provider.register(signer).await?;

      let client = ctx.get_client(chain).await?;
      let latest_block = client.get_block_number().await?;

      println!("To Block {}", latest_block);

      railgun_provider.sync_to(latest_block, true).await?; // using SubsquidSyncer

      let synced_block = railgun_provider.utxo_indexer.synced_block();
      println!("Synced block: {}", synced_block);

      Ok(())
   }

   #[tokio::test]
   async fn test_load_state() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt().with_env_filter("info,error,debug").init();

      let ctx = ZeusCtx::new();
      let chain = 1;

      let wallet = create_wallet();
      let seed = wallet.seed()?;
      let signer = RailgunSigner::from_seed(&seed, 0, chain)?;

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx.clone(), chain).await?;

      railgun_provider.register(signer).await?;

      let synced_block = railgun_provider.utxo_indexer.synced_block();
      println!("Synced block: {}", synced_block);

      Ok(())
   }

   #[tokio::test]
   async fn test_verify_root() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt().with_env_filter("info,error,debug").init();

      let ctx = ZeusCtx::new();
      let chain = 1;

      let wallet = create_wallet();
      let seed = wallet.seed()?;
      let signer = RailgunSigner::from_seed(&seed, 0, chain)?;

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx.clone(), chain).await?;

      railgun_provider.register(signer).await?;

      let synced_block = railgun_provider.utxo_indexer.synced_block();
      println!("Synced block: {}", synced_block);

      let block_id = BlockId::number(synced_block);
      railgun_provider.verify_root(Some(block_id)).await?;

      Ok(())
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_shield_unshield() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt().with_env_filter("info,error").init();

      let ctx = ZeusCtx::new();
      let chain = 1;
      let chain_config = ChainConfig::mainnet();
      let railgun_addr = chain_config.railgun_smart_wallet;

      let wallet = create_wallet();
      let seed = wallet.seed()?;
      let signer = RailgunSigner::from_seed(&seed, 0, chain)?;
      let railgun_address = signer.address().clone();

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx.clone(), chain).await?;

      railgun_provider.register(signer).await?;

      let amount = NumericValue::parse_to_wei("1", 18);
      let weth = ERC20Token::weth();
      let weth_id = AssetId::Erc20(weth.address);

      let client = ctx.get_client(chain).await?;

      let dummy_account = DummyAccount {
         account_type: AccountType::EOA,
         balance: U256::ZERO,
         address: wallet.address(),
         key: wallet.key.to_signer(),
      };

      let synced_block = railgun_provider.utxo_indexer.synced_block();
      let fork_block = BlockId::number(synced_block);
      let full_block = client.get_block(fork_block).await.unwrap();
      let timestamp = full_block.unwrap().header.timestamp;
      println!("Fork block {}", synced_block);

      let mut factory =
         ForkFactory::new_sandbox_factory(client.clone(), chain, None, Some(fork_block));
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
         panic!("Call Failed");
      } else {
         eprintln!("Railgun Call Successful");
         eprintln!("Gas Used: {}", res.tx_gas_used());
      }

      let logs = res.logs().to_vec();
      railgun_provider.utxo_indexer.sync_from_logs(logs, synced_block, timestamp)?;

      let balance = railgun_provider.balance_erc20(railgun_address, weth_id).await;
      let balance_fmt = NumericValue::format_wei(U256::from(balance), weth.decimals);

      // Expected balance after 0.25% fee
      let expected_balance = amount.calc_slippage(0.25, weth.decimals);
      println!("Private Balance: {}", balance_fmt.f64());
      assert_eq!(balance_fmt.wei(), expected_balance.wei());

      Ok(())
   }
}
