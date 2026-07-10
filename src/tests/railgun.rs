#[cfg(test)]
mod tests {
   use std::sync::Arc;

   use crate::core::{ZeusCtx, railgun_db_file, railgun_dir};
   use alloy_eips::BlockId;
   use zeus_eth::revm_utils::simulate::erc20_balance;
   use zeus_eth::utils::client::RpcClient;
   use zeus_railgun::*;

   use zeus_eth::alloy_primitives::{TxKind, U256};
   use zeus_eth::alloy_provider::{Provider, network::Ethereum};
   use zeus_eth::{currency::ERC20Token, revm_utils::*, utils::NumericValue};
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
      let utxo_syncer = Syncer::new(client.clone(), chain_config.railgun_smart_wallet);

      let database = RedbDatabase::new(db_file)?;
      let utxo_indexer = UtxoIndexer::new(
         Arc::new(database),
         Arc::new(utxo_syncer),
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
      let chain_config = ChainConfig::mainnet();

      let client = ctx.get_client(chain).await?;
      let wallet = create_wallet();
      let seed = wallet.seed()?;
      let signer = RailgunSigner::from_seed(&seed, 0, chain)?;

      let mut railgun_provider: RailgunProvider<RpcClient> =
         create_railgun_provider(ctx.clone(), chain).await?;

      railgun_provider.register(signer).await?;

      // Do a lite sync
      // Pass None for from_block so we resume from the last persisted synced_block + account states in the DB.
      let latest_block = client.get_block_number().await?;
      let to_block = chain_config.deployment_block + 1_000_000;
      println!("To Block {}", to_block);

      railgun_provider.sync_to(None, to_block).await?;

      let synced_block = railgun_provider.utxo_indexer.synced_block();
      println!("Synced block: {}", synced_block);
      assert_eq!(synced_block, to_block);

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
}
