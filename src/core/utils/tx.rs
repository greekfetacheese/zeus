use anyhow::anyhow;
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_network::{Ethereum, TransactionBuilder},
   alloy_primitives::{Address, Bytes, U256, utils::format_ether},
   alloy_rpc_types::{TransactionReceipt, TransactionRequest},
   types::ChainId,
   utils::NumericValue,
   wallet::{SecureSigner, SecureWallet},
};

#[derive(Clone)]
pub struct TxParams {
   pub signer: SecureSigner,
   pub recipient: Address,
   pub value: U256,
   pub chain: ChainId,
   pub miner_tip: U256,
   pub base_fee: u64,
   pub call_data: Bytes,
   pub gas_used: u64,
}

impl TxParams {
   pub fn new(
      signer: SecureSigner,
      recipient: Address,
      value: U256,
      chain: ChainId,
      miner_tip: U256,
      base_fee: u64,
      call_data: Bytes,
      gas_used: u64,
   ) -> Self {
      Self {
         signer,
         recipient,
         value,
         chain,
         miner_tip,
         base_fee,
         call_data,
         gas_used,
      }
   }

   pub fn max_fee_per_gas(&self) -> U256 {
      let fee = self.miner_tip + U256::from(self.base_fee);
      // add a 10% tolerance
      fee * U256::from(110) / U256::from(100)
   }

   pub fn gas_cost(&self) -> U256 {
      if self.chain.is_ethereum() || self.chain.is_optimism() || self.chain.is_base() {
         U256::from(U256::from(self.gas_used) * self.max_fee_per_gas())
      } else {
         U256::from(self.gas_used * self.base_fee)
      }
   }

   pub fn sufficient_balance(&self, balance: NumericValue) -> Result<(), anyhow::Error> {
      let coin = self.chain.coin_symbol();
      let cost = self.gas_cost();
      let cost_str = format!("{:.6}", format_ether(cost));

      if balance.wei().unwrap() < cost {
         return Err(anyhow!(
            "Insufficient balance to cover gas fees, need at least {} {} but you have {} {}",
            cost_str,
            coin,
            balance.formatted(),
            coin
         ));
      }

      Ok(())
   }
}

pub async fn send_tx<P>(client: P, params: TxParams) -> Result<TransactionReceipt, anyhow::Error>
where
   P: Provider<(), Ethereum> + Clone + 'static,
{
   let signer_address = params.signer.borrow().address();
   let nonce = client.get_transaction_count(signer_address).await?;

   let mut tx = legacy_or_eip1559(params.clone());
   tx.set_nonce(nonce);
   tx.set_gas_limit(params.gas_used * 15 / 10); // +50%

   let wallet = SecureWallet::from(params.signer.clone());
   let tx_envelope = tx.clone().build(wallet.borrow()).await?;

   tracing::info!("Sending Transaction...");
   let time = std::time::Instant::now();
   let receipt = client
      .send_tx_envelope(tx_envelope)
      .await?
      .get_receipt()
      .await?;
   tracing::info!(
      "Time take to send tx: {:?}secs",
      time.elapsed().as_secs_f32()
   );

   Ok(receipt)
}

fn legacy_or_eip1559(params: TxParams) -> TransactionRequest {
   // Eip1559
   if params.chain.is_ethereum() || params.chain.is_optimism() || params.chain.is_base() {
      return TransactionRequest::default()
         .with_from(params.signer.borrow().address())
         .with_to(params.recipient)
         .with_chain_id(params.chain.id())
         .with_value(params.value)
         .with_input(params.call_data.clone())
         .with_max_priority_fee_per_gas(params.miner_tip.to::<u128>())
         .max_fee_per_gas(params.max_fee_per_gas().to::<u128>());
   } else {
      // Legacy
      return TransactionRequest::default()
         .with_from(params.signer.borrow().address())
         .with_to(params.recipient)
         .with_value(params.value)
         .with_input(params.call_data)
         .with_gas_price(params.base_fee.into());
   }
}
