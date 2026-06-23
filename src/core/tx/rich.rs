use crate::utils::TimeStamp;
use zeus_eth::{
   alloy_primitives::{Address, Bytes, TxHash, U256},
   utils::NumericValue,
};

use alloy_consensus::TxType;

use super::analysis::TransactionAnalysis;
use super::events::DecodedEvent;
use serde::{Deserialize, Serialize};

/// A transaction that has been sent to the network with additional data like
///
/// a high-level overview of the transaction, decoded events etc...
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionRich {
   pub tx_type: TxType,
   pub success: bool,
   pub chain: u64,
   pub block: u64,
   pub timestamp: TimeStamp,
   pub value_sent: NumericValue,
   pub value_sent_usd: NumericValue,
   pub eth_received: NumericValue,
   pub eth_received_usd: NumericValue,
   pub tx_cost: NumericValue,
   pub tx_cost_usd: NumericValue,
   pub hash: TxHash,
   pub contract_interact: bool,

   pub analysis: TransactionAnalysis,
   pub main_event: DecodedEvent,
}

impl TransactionRich {
   /// Who sent the transaction
   pub fn sender(&self) -> Address {
      self.analysis.sender
   }

   pub fn interact_to(&self) -> Address {
      self.analysis.interact_to
   }

   pub fn value(&self) -> U256 {
      self.analysis.value
   }

   pub fn call_data(&self) -> Bytes {
      self.analysis.call_data.clone()
   }
}
