use anyhow::anyhow;
use alloy_consensus::TxType;
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_network::{Ethereum, TransactionBuilder},
   alloy_primitives::{Address, TxHash, Bytes, U256},
   alloy_rpc_types::{TransactionReceipt, TransactionRequest},
   types::ChainId,
   utils::NumericValue,
   wallet::{SecureSigner, SecureWallet},
   currency::{Currency, ERC20Token, NativeCurrency},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TxMethod {
   Transfer(NativeCurrency),
   ERC20Transfer((ERC20Token, NumericValue)),
   Bridge((Currency, NumericValue)),
   Swap(SwapDetails),
   /// Unknown transaction method
   Other,
}

impl TxMethod {
   pub fn as_str(&self) -> &str {
      match self {
         TxMethod::Transfer(_) => "Transfer",
         TxMethod::ERC20Transfer(_) => "ERC20 Transfer",
         TxMethod::Bridge(_) => "Bridge",
         TxMethod::Swap(_) => "Swap",
         TxMethod::Other => "Other",
      }
   }

   pub fn is_transfer(&self) -> bool {
      matches!(self, TxMethod::Transfer(_))
   }

   pub fn is_erc20_transfer(&self) -> bool {
      matches!(self, TxMethod::ERC20Transfer(_))
   }

   pub fn is_bridge(&self) -> bool {
      matches!(self, TxMethod::Bridge(_))
   }

   pub fn is_swap(&self) -> bool {
      matches!(self, TxMethod::Swap(_))
   }

   pub fn is_other(&self) -> bool {
      matches!(self, TxMethod::Other)
   }

   pub fn native_currency(&self) -> Option<&NativeCurrency> {
      match self {
         TxMethod::Transfer(native) => Some(native),
         _ => None,
      }
   }

   pub fn erc20_transfer_info(&self) -> Option<&(ERC20Token, NumericValue)> {
      match self {
         TxMethod::ERC20Transfer(info) => Some(info),
         _ => None,
      }
   }

   pub fn bridge_info(&self) -> Option<&(Currency, NumericValue)> {
      match self {
         TxMethod::Bridge(info) => Some(info),
         _ => None,
      }
   }

   pub fn swap_details(&self) -> Option<&SwapDetails> {
      match self {
         TxMethod::Swap(details) => Some(details),
         _ => None,
      }
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SwapDetails {
   pub token_in: Currency,
   pub token_out: Currency,
   pub amount_in: NumericValue,
   pub amount_out: NumericValue,
}

impl Default for SwapDetails {
   fn default() -> Self {
      Self {
         token_in: Currency::from_erc20(ERC20Token::weth()),
         token_out: Currency::from_erc20(ERC20Token::dai()),
         amount_in: NumericValue::default(),
         amount_out: NumericValue::default(),
      }
   }
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxDetails {
   pub success: bool,
   pub from: Address,
   pub to: Address,
   pub value: NumericValue,
   /// At the time of the tx
   pub eth_price: NumericValue,
   pub call_data: Bytes,
   pub hash: TxHash,
   pub block: u64,
   pub index: u64,
   pub method: TxMethod,
   pub nonce: u64,
   pub gas_used: u64,
   pub gas_limit: u64,
   pub base_fee: NumericValue,
   pub priority_fee: NumericValue,
   pub tx_type: TxType,
}

impl Default for TxDetails {
   fn default() -> Self {
      Self {
         success: false,
         from: Address::ZERO,
         to: Address::ZERO,
         value: NumericValue::default(),
         eth_price: NumericValue::default(),
         call_data: Bytes::default(),
         hash: TxHash::ZERO,
         block: 0,
         index: 0,
         method: TxMethod::Transfer(NativeCurrency::default()),
         nonce: 0,
         gas_used: 21_000,
         gas_limit: 21_000,
         base_fee: NumericValue::default(),
         priority_fee: NumericValue::default(),
         tx_type: TxType::Eip1559,
      }
   }
}

impl TxDetails {
   pub fn new(
      success: bool,
      from: Address,
      to: Address,
      value: NumericValue,
      eth_price: NumericValue,
      call_data: Bytes,
      hash: TxHash,
      block: u64,
      index: u64,
      method: TxMethod,
      nonce: u64,
      gas_used: u64,
      gas_limit: u64,
      base_fee: NumericValue,
      priority_fee: NumericValue,
      tx_type: TxType,
   ) -> Self {
      Self {
         success,
         from,
         to,
         value,
         eth_price,
         call_data,
         hash,
         block,
         index,
         method,
         nonce,
         gas_used,
         gas_limit,
         base_fee,
         priority_fee,
         tx_type,
      }
   }

   pub fn success_str(&self) -> &str {
      match self.success{
         true => "Success",
         false => "Failed",
      }
   }

   /// Base fee + Priority fee
   pub fn gas_price(&self) -> NumericValue {
      let fee = self.base_fee.wei().unwrap_or_default() + self.priority_fee.wei().unwrap_or_default();
      NumericValue::format_to_gwei(fee)
   }

   /// Gas used * Gas Price
   /// 
   /// Amount paid in native currency to include the tx
   pub fn fee_in_eth(&self) -> NumericValue {
      let gas_price = self.gas_price().wei().unwrap_or_default();
      let fee_in_eth = U256::from(self.gas_used) * gas_price;
      NumericValue::format_wei(fee_in_eth, 18)
   }

   /// Fee in USD at the time of the transaction
   pub fn fee_in_usd(&self) -> NumericValue {
      let fee_in_eth = self.fee_in_eth();
      NumericValue::value(fee_in_eth.f64(), self.eth_price.f64())
   }

   /// Tx value in USD at the time of the transaction
   pub fn value_in_usd(&self) -> NumericValue {
      NumericValue::value(self.value.f64(), self.eth_price.f64())
   }
}

#[derive(Clone)]
pub struct TxParams {
   pub tx_method: TxMethod,
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
      tx_method: TxMethod,
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
         tx_method,
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
      let cost_in_eth = self.gas_cost();
      let cost = NumericValue::format_wei(cost_in_eth, 18);

      if balance.wei().unwrap() < cost.wei().unwrap() {
         return Err(anyhow!(
            "Insufficient balance to cover gas fees, need at least {} {} but you have {} {}",
            cost.formatted(),
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

pub fn legacy_or_eip1559(params: TxParams) -> TransactionRequest {
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
