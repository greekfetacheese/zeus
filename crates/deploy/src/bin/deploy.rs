use zeus_eth::{
   alloy_network::TransactionBuilder,
   alloy_primitives::{Bytes, TxKind, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, TransactionRequest},
   alloy_signer_local::PrivateKeySigner,
   currency::ERC20Token,
   revm::ExecuteCommitEvm,
   revm_utils::{ForkFactory, new_evm, revert_msg},
   types::ChainId,
   utils::{NumericValue, SecureSigner, client::*, price_feed::get_base_token_price},
};

use anyhow::anyhow;
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
   print!("Enter the ChainId (eg 1 for Ethereum Mainnet): ");
   std::io::stdout().flush().unwrap();
   let mut chain_id = String::new();
   std::io::stdin().read_line(&mut chain_id).unwrap();

   let chain_id = chain_id.trim();
   let chain = ChainId::new(chain_id.parse::<u64>().unwrap()).unwrap();

   print!("Enter the rpc url: ");
   std::io::stdout().flush().unwrap();
   let mut rpc_url = String::new();
   std::io::stdin().read_line(&mut rpc_url).unwrap();

   let url = rpc_url.trim();

   print!("Paste the private key to use for signing the transaction: ");
   std::io::stdout().flush().unwrap();
   let mut key = String::new();
   std::io::stdin().read_line(&mut key).unwrap();

   let signer = PrivateKeySigner::from_str(key.trim()).unwrap();

   print!("Signer address: {}\n", signer.address());
   std::io::stdout().flush().unwrap();

   print!("Paste the contract bytecode: ");
   std::io::stdout().flush().unwrap();
   let mut bytecode = String::new();
   std::io::stdin().read_line(&mut bytecode).unwrap();

   let code = Bytes::from_str(bytecode.trim()).unwrap();
   print!("Loaded bytecode: {}\n", code);
   std::io::stdout().flush().unwrap();

   print!("Fetching client and block...\n");
   std::io::stdout().flush().unwrap();

   let retry = retry_layer(10, 400, 300);
   let throttle = throttle_layer(10);
   let client = get_client(url, retry, throttle, 60).await?;
   let client_chain = client.get_chain_id().await?;

   if client_chain != chain.id() {
      return Err(anyhow!(
         "Client chain id {} does not match expected chain id {}",
         client_chain,
         chain.id()
      ));
   }

   let weth = ERC20Token::wrapped_native_token(chain.id());
   let weth_price = get_base_token_price(client.clone(), chain.id(), weth.address, None).await?;

   let block = client.get_block(BlockId::latest()).await.unwrap();
   let fork_factory = ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, None);
   let fork_db = fork_factory.new_sandbox_fork();

   let mut evm = new_evm(chain, block.as_ref(), fork_db);

   println!("Simulating deployment...\n");
   std::io::stdout().flush().unwrap();

   evm.tx.caller = signer.address();
   evm.tx.data = code.clone();
   evm.tx.value = U256::ZERO;
   evm.tx.kind = TxKind::Create;

   let res = evm.transact_commit(evm.tx.clone()).unwrap();
   let gas_used = res.gas_used();

   if !res.is_success() {
      let err = revert_msg(&res.output().unwrap());
      println!("Call Reverted: {}", err);
      return Err(anyhow!("Call Reverted: {}", err));
   }

   print!("Gas Used: {}\n", gas_used);
   std::io::stdout().flush().unwrap();

   let fee = client.get_max_priority_fee_per_gas().await?;
   let priority_fee = NumericValue::format_to_gwei(U256::from(fee));

   let base_fee = client.get_gas_price().await?;
   let total_gas_price = U256::from(base_fee) + priority_fee.wei();
   let cost_in_wei = total_gas_price * U256::from(gas_used);
   let cost_eth = NumericValue::format_wei(cost_in_wei, 18);
   let cost_in_usd = NumericValue::from_f64(cost_eth.f64() * weth_price);

   println!(
      "Cost in USD: ${}",
      cost_in_usd.abbreviated()
   );
   std::io::stdout().flush().unwrap();

   print!("Procced to deploy? [y/n]: ");
   std::io::stdout().flush().unwrap();
   let mut deploy = String::new();
   std::io::stdin().read_line(&mut deploy).unwrap();

   if deploy.trim() != "y" {
      return Err(anyhow!("Deploy cancelled"));
   }

   print!("Sending transaction...\n");
   std::io::stdout().flush().unwrap();

   let nonce = client.get_transaction_count(signer.address()).await.unwrap();
   let value = U256::ZERO;
   let gas_limit = gas_used * 110 / 100;
   let max_fee = total_gas_price * U256::from(110) / U256::from(100);

   let tx = if chain.is_ethereum() || chain.is_optimism() || chain.is_base() {
      TransactionRequest::default()
         .with_from(signer.address())
         .with_chain_id(chain.id())
         .with_deploy_code(code)
         .with_value(value)
         .with_nonce(nonce)
         .with_gas_limit(gas_limit)
         .with_max_priority_fee_per_gas(priority_fee.wei().to::<u128>())
         .max_fee_per_gas(max_fee.to::<u128>())
   } else {
      TransactionRequest::default()
         .with_from(signer.address())
         .with_deploy_code(code)
         .with_chain_id(chain.id())
         .with_value(value)
         .with_nonce(nonce)
         .with_gas_limit(gas_limit)
         .with_gas_price(base_fee)
   };

   let signer = SecureSigner::from(signer);
   let wallet = signer.to_wallet();
   let tx_envelope = tx.clone().build(&wallet).await.unwrap();

   let receipt = client
      .send_tx_envelope(tx_envelope)
      .await
      .unwrap()
      .with_timeout(Some(Duration::from_secs(60)))
      .get_receipt()
      .await
      .unwrap();

   let contract_address = receipt.contract_address.expect("Failed to get contract address");
   print!("Contract Deployed at: {}", contract_address);
   std::io::stdout().flush().unwrap();

   Ok(())
}
