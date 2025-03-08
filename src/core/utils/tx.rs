use anyhow::anyhow;
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_network::{Ethereum, TransactionBuilder},
   alloy_primitives::{
      Address, Bytes, U256,
      utils::format_ether,
   },
   alloy_rpc_types::{TransactionReceipt, TransactionRequest},
   types::ChainId,
   wallet::{SafeSigner, SafeWallet},
};

#[derive(Clone)]
pub struct TxParams {
   pub signer: SafeSigner,
   pub recipient: Address,
   pub value: U256,
   pub chain: u64,
   pub miner_tip: U256,
   pub base_fee: u64,
   pub call_data: Bytes,
}

impl TxParams {
   pub fn new(
      signer: SafeSigner,
      recipient: Address,
      value: U256,
      chain: u64,
      miner_tip: U256,
      base_fee: u64,
      call_data: Bytes,
   ) -> Self {
      Self {
         signer,
         recipient,
         value,
         chain,
         miner_tip,
         base_fee,
         call_data,
      }
   }
}

pub async fn send_tx<P>(client: P, params: TxParams) -> Result<TransactionReceipt, anyhow::Error>
where
   P: Provider<(), Ethereum> + Clone + 'static,
{
   let chain = ChainId::new(params.chain)?;
   let signer_address = params.signer.inner().address();

   let nonce = client.get_transaction_count(signer_address).await?;
   let max_fee_per_gas = params.miner_tip + U256::from(params.base_fee);

   let mut tx = TransactionRequest::default()
      .with_from(signer_address)
      .with_to(params.recipient)
      .with_nonce(nonce)
      .with_chain_id(params.chain)
      .with_value(params.value)
      .with_input(params.call_data)
      .with_max_priority_fee_per_gas(params.miner_tip.to::<u128>())
      .max_fee_per_gas(max_fee_per_gas.to::<u128>());

   // calculate the estimated cost of the transaction
   let gas_used = client.estimate_gas(&tx).await?;
   let gas_cost = U256::from(gas_used * params.base_fee);
   let balance = client.get_balance(signer_address).await?;
   has_funds(chain, gas_cost, balance)?;
   tx.set_gas_limit(gas_used * 15 / 10); // +50%

   let signer = SafeWallet::from(params.signer.clone());
   let tx_envelope = tx.clone().build(&signer.inner()).await?;

   tracing::info!("Sending Transaction...");
   let time = std::time::Instant::now();
   let receipt = client
      .send_tx_envelope(tx_envelope)
      .await?
      .get_receipt()
      .await?;
   tracing::info!("Time take to send tx: {:?}secs", time.elapsed().as_secs_f32());

   Ok(receipt)
}

fn has_funds(chain: ChainId, gas_cost: U256, balance: U256) -> Result<(), anyhow::Error> {
   let symbol = chain.coin_symbol();
   let gas_cost = format_ether(gas_cost);
   let balance = format_ether(balance);

   if balance < gas_cost {
      return Err(anyhow!(
         "Insufficient balance to cover gas fees, need at least {} {} but you have {} {}",
         gas_cost,
         symbol,
         balance,
         symbol
      ));
   }

   Ok(())
}
