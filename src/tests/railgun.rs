#[cfg(test)]
mod tests {
   use std::sync::Arc;
   use std::time::Duration;

   use crate::core::{ZeusCtx, railgun_db_file, railgun_dir};
   use alloy_eips::BlockId;
   use zeus_eth::revm_utils::simulate::erc20_balance;
   use zeus_eth::utils::client::RpcClient;
   use zeus_railgun::caip::AssetId;
   use zeus_railgun::*;

   use std::time::Instant;
   use zeus_eth::alloy_primitives::{TxKind, U256};
   use zeus_eth::alloy_provider::Provider;
   use zeus_eth::{currency::ERC20Token, revm_utils::*, types::ChainId, utils::NumericValue};
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

      let snapshot_loader = SnapshotLoader::new(railgun_dir.clone());
      let chain_config = ChainConfig::from_chain_id(chain).unwrap();
      let utxo_verifier = RootVerifier::new(client.clone(), chain_config.railgun_smart_wallet);
      let rpc_syncer = RpcSyncer::new(
         client.clone(),
         chain,
         chain_config.railgun_smart_wallet,
      )
      .with_snapshot_loader(snapshot_loader.clone());

      let subsquid_syncer: Option<Arc<dyn UtxoSyncer>> = Some(Arc::new(
         SubsquidSyncer::new(&chain_config.subsquid_endpoint, chain)
            .with_snapshot_loader(snapshot_loader),
      ));

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
   async fn db_compact_mem_usage() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt()
         .with_env_filter("info,error,debug")
         .with_test_writer()
         .init();

      let ctx = ZeusCtx::new();
      let chain = ChainId::EthereumSepolia;

      let railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx.clone(), chain.id()).await?;

      let mut times = 0;

      loop {
         railgun_provider.compact().await?;
         times += 1;
         tracing::info!("Compacted DB {} times", times);
         tokio::time::sleep(Duration::from_secs(1)).await;
      }
   }

   #[tokio::test]
   async fn db_save_mem_usage() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt()
         .with_env_filter("info,error,debug")
         .with_test_writer()
         .init();

      let ctx = ZeusCtx::new();
      let chain = ChainId::EthereumSepolia;

      let railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx.clone(), chain.id()).await?;

      let mut times = 0;

      loop {
         railgun_provider.save(true).await?;
         times += 1;
         tracing::info!("Saved DB {} times", times);
         tokio::time::sleep(Duration::from_secs(1)).await;
      }
   }

   #[tokio::test]
   async fn snapshot_loader_mem_usage() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt()
         .with_env_filter("info,error,debug")
         .with_test_writer()
         .init();

      let chain = ChainId::Ethereum;
      let snapshot_loader = SnapshotLoader::new(railgun_dir().unwrap());

      let mut times = 0;

      loop {
         {
            let _events = snapshot_loader.load(chain.id()).await?;
         }

         unsafe {
            if libc::malloc_trim(0) == 1 {
               tracing::info!("Released free memory");
            } else {
               tracing::warn!("Failed to release free memory");
            }
         }

         times += 1;
         tracing::info!("read snapshot {} times", times);
         tokio::time::sleep(Duration::from_secs(10)).await;
      }
   }

   #[tokio::test]
   async fn test_sync() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt()
         .with_env_filter("info,error,debug")
         .with_test_writer()
         .init();

      let ctx = ZeusCtx::new();
      let chain = ChainId::Ethereum;
      let _chain_config = ChainConfig::from_chain_id(chain.id()).unwrap();
      let client = ctx.get_client(chain.id()).await?;

      let wallet = create_wallet();
      let seed = wallet.seed()?;
      let signer = RailgunSigner::from_seed(&seed, 0, chain.id())?;

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx.clone(), chain.id()).await?;

      railgun_provider.register(signer).await?;
      railgun_provider.set_provider(client.clone());

      {
         let indexer = railgun_provider.utxo_indexer.write().await;
         indexer.rpc_syncer.set_provider(client.clone().erased()).await;
         indexer.utxo_verifier.set_provider(client.clone().erased()).await;
      }

      let latest_block = client.get_block_number().await?;
      let to_block = latest_block;
      let use_subsquid = false;

      railgun_provider.sync_to(to_block, use_subsquid).await?;

      Ok(())
   }

   #[tokio::test]
   async fn test_snapshot_loader() {
      tracing_subscriber::fmt()
         .with_env_filter("info,error,debug")
         .with_test_writer()
         .init();

      let chain = 1;
      let dir = railgun_dir().unwrap();
      let loader = SnapshotLoader::new(dir);

      let time = Instant::now();
      let snapshot = loader.load(chain).await.unwrap();
      println!(
         "Snapshot loaded in {}ms",
         time.elapsed().as_millis()
      );

      println!(
         "Events {} | latest block {}",
         snapshot.events.len(),
         snapshot.block_number
      );

      loop {
         std::thread::sleep(std::time::Duration::from_secs(1));
         println!("Press ctrl-c to exit");
      }
   }

   #[tokio::test]
   async fn test_load_state() {
      tracing_subscriber::fmt().with_env_filter("info,error,debug").init();

      let ctx = ZeusCtx::new();
      let chain = 1;

      let wallet = create_wallet();
      let seed = wallet.seed().unwrap();
      let signer = RailgunSigner::from_seed(&seed, 0, chain).unwrap();

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx, chain).await.unwrap();

      railgun_provider.register(signer).await.unwrap();

      let synced_block = railgun_provider.utxo_indexer.read().await.account_synced_block();
      println!("Synced block: {}", synced_block);

      match railgun_provider.compact().await {
         Ok(true) => println!("Database compaction performed"),
         Ok(false) => println!("Database does not need compaction"),
         Err(e) => eprintln!("Compaction failed: {}", e),
      }

      loop {
         std::thread::sleep(std::time::Duration::from_secs(1));
         println!("Press ctrl-c to exit");
      }
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

      let synced_block = railgun_provider.utxo_indexer.read().await.account_synced_block();
      println!("Synced block: {}", synced_block);

      let block_id = BlockId::number(synced_block);
      railgun_provider.verify_root(Some(block_id)).await?;

      Ok(())
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn test_shield_unshield() -> Result<(), anyhow::Error> {
      tracing_subscriber::fmt()
         .with_env_filter("info,error")
         .with_test_writer()
         .init();

      let ctx = ZeusCtx::new();
      let chain = ChainId::EthereumSepolia;
      let chain_config = ChainConfig::from_chain_id(chain.id()).unwrap();
      let railgun_addr = chain_config.railgun_smart_wallet;

      let wallet = create_wallet();
      let seed = wallet.seed()?;
      let signer = RailgunSigner::from_seed(&seed, 0, chain.id())?;
      let railgun_address = signer.address().clone();

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx.clone(), chain.id()).await?;

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

      let synced_block = railgun_provider.utxo_indexer.read().await.account_synced_block();
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
