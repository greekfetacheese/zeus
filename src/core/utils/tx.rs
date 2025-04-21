use crate::core::utils::action::OnChainAction;
use alloy_consensus::TxType;
use anyhow::anyhow;
use std::time::Duration;
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_network::{Ethereum, TransactionBuilder},
   alloy_primitives::{Address, Bytes, TxHash, U256},
   alloy_rpc_types::{TransactionReceipt, TransactionRequest},
   currency::{Currency, ERC20Token, NativeCurrency},
   types::ChainId,
   utils::NumericValue,
   wallet::{SecureSigner, SecureWallet},
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
         token_in: Currency::from(ERC20Token::weth()),
         token_out: Currency::from(ERC20Token::dai()),
         amount_in: NumericValue::default(),
         amount_out: NumericValue::default(),
      }
   }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxSummary {
   pub success: bool,
   pub chain: u64,
   pub block: u64,
   pub timestamp: u64,
   pub from: Address,
   pub to: Address,
   pub eth_spent: NumericValue,
   pub eth_spent_usd: NumericValue,
   pub tx_cost: NumericValue,
   pub tx_cost_usd: NumericValue,
   pub gas_used: u64,
   pub hash: TxHash,
   pub action: OnChainAction,
   pub contract_interact: bool,
}

impl Default for TxSummary {
   fn default() -> Self {
      Self {
         success: false,
         chain: 1,
         block: 0,
         timestamp: 0,
         from: Address::ZERO,
         to: Address::ZERO,
         eth_spent: NumericValue::default(),
         eth_spent_usd: NumericValue::default(),
         tx_cost: NumericValue::default(),
         tx_cost_usd: NumericValue::default(),
         gas_used: 60_000,
         hash: TxHash::ZERO,
         action: OnChainAction::dummy_transfer(),
         contract_interact: false,
      }
   }
}

impl TxSummary {

   pub fn dummy_token_approve() -> Self {
      let tx_cost = NumericValue::parse_to_wei("0.0001", 18);
      let tx_cost_usd = NumericValue::value(tx_cost.f64(), 1600.0);
      let timestamp = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap()
         .as_secs();
      Self {
         success: true,
         chain: 1,
         block: 0,
         timestamp,
         from: Address::ZERO,
         to: Address::ZERO,
         eth_spent: NumericValue::default(),
         eth_spent_usd: NumericValue::default(),
         tx_cost,
         tx_cost_usd,
         gas_used: 60_000,
         hash: TxHash::ZERO,
         action: OnChainAction::dummy_token_approve(),
         contract_interact: false,
      }
   }

   pub fn dummy_swap2(from: Address) -> Self {
      let tx_cost = NumericValue::parse_to_wei("0.0001", 18);
      let tx_cost_usd = NumericValue::value(tx_cost.f64(), 1600.0);
      let timestamp = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap()
         .as_secs();
      Self {
         success: true,
         chain: 1,
         block: 0,
         timestamp,
         from,
         to: Address::ZERO,
         eth_spent: NumericValue::default(),
         eth_spent_usd: NumericValue::default(),
         tx_cost,
         tx_cost_usd,
         gas_used: 120_000,
         hash: TxHash::ZERO,
         action: OnChainAction::dummy_swap(),
         contract_interact: true,
      }
   }
   pub fn dummy_swap() -> Self {
      let tx_cost = NumericValue::parse_to_wei("0.0001", 18);
      let tx_cost_usd = NumericValue::value(tx_cost.f64(), 1600.0);
      Self {
         success: true,
         chain: 1,
         block: 0,
         timestamp: 0,
         from: Address::ZERO,
         to: Address::ZERO,
         eth_spent: NumericValue::default(),
         eth_spent_usd: NumericValue::default(),
         tx_cost,
         tx_cost_usd,
         gas_used: 120_000,
         hash: TxHash::ZERO,
         action: OnChainAction::dummy_swap(),
         contract_interact: true,
      }
   }

   pub fn success_str(&self) -> &str {
      match self.success {
         true => "Success",
         false => "Failed",
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
      match self.success {
         true => "Success",
         false => "Failed",
      }
   }

   /// Base fee + Priority fee
   pub fn gas_price(&self) -> NumericValue {
      let fee =
         self.base_fee.wei().unwrap_or_default() + self.priority_fee.wei().unwrap_or_default();
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
   pub signer: SecureSigner,
   pub transcact_to: Address,
   pub nonce: u64,
   pub value: U256,
   pub chain: ChainId,
   pub miner_tip: U256,
   pub base_fee: u64,
   pub call_data: Bytes,
   pub gas_used: u64,
   pub gas_limit: u64,
}

impl TxParams {
   pub fn new(
      signer: SecureSigner,
      transcact_to: Address,
      nonce: u64,
      value: U256,
      chain: ChainId,
      miner_tip: U256,
      base_fee: u64,
      call_data: Bytes,
      gas_used: u64,
      gas_limit: u64,
   ) -> Self {
      Self {
         signer,
         transcact_to,
         nonce,
         value,
         chain,
         miner_tip,
         base_fee,
         call_data,
         gas_used,
         gas_limit,
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

      if balance.wei2() < cost.wei2() {
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
   P: Provider<Ethereum> + Clone + 'static,
{
   let tx = legacy_or_eip1559(params.clone());
   let wallet = SecureWallet::from(params.signer.clone());
   let tx_envelope = tx.clone().build(wallet.borrow()).await?;

   tracing::info!("Sending Transaction...");
   let time = std::time::Instant::now();
   let receipt = client
      .send_tx_envelope(tx_envelope)
      .await?
      .with_timeout(Some(Duration::from_secs(60)))
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
         .with_to(params.transcact_to)
         .with_chain_id(params.chain.id())
         .with_value(params.value)
         .with_nonce(params.nonce)
         .with_input(params.call_data.clone())
         .with_gas_limit(params.gas_limit)
         .with_max_priority_fee_per_gas(params.miner_tip.to::<u128>())
         .max_fee_per_gas(params.max_fee_per_gas().to::<u128>());
   } else {
      // Legacy
      return TransactionRequest::default()
         .with_from(params.signer.borrow().address())
         .with_to(params.transcact_to)
         .with_value(params.value)
         .with_nonce(params.nonce)
         .with_input(params.call_data)
         .with_gas_limit(params.gas_limit)
         .with_gas_price(params.base_fee.into());
   }
}