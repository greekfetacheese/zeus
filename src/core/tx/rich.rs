use crate::core::clear_signing::ClearDisplay;
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
   /// ERC-7730 view when [`Self::main_event`] is unknown. Older vaults omit this.
   #[serde(default)]
   pub clear_display: Option<ClearDisplay>,
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

   /// History / details heading. Prefers the ERC-7730 intent when the main event is unknown.
   pub fn summary_name(&self) -> String {
      if self.main_event.is_other() {
         if let Some(display) = &self.clear_display {
            return display.heading.clone();
         }
      }
      self.main_event.name()
   }

   pub fn dummy_clear_signed() -> Self {
      Self {
         tx_type: TxType::Eip1559,
         success: true,
         chain: 8453,
         block: 1,
         timestamp: TimeStamp::Seconds(1_745_151_870),
         value_sent: NumericValue::default(),
         value_sent_usd: NumericValue::default(),
         eth_received: NumericValue::default(),
         eth_received_usd: NumericValue::default(),
         tx_cost: NumericValue::default(),
         tx_cost_usd: NumericValue::default(),
         hash: TxHash::default(),
         contract_interact: true,
         analysis: TransactionAnalysis::dummy_clear_signed(),
         main_event: DecodedEvent::Other,
         clear_display: Some(ClearDisplay::dummy_calldata()),
      }
   }
}
