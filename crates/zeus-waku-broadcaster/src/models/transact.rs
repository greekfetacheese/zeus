//! Transact request and response models for the Railgun Waku broadcaster.
//!
//! Ported from @railgun-community/shared-models and broadcaster-transaction.ts

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The type of transact request sent to the broadcaster.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BroadcasterTransactRequestType {
   Common,
}

/// Raw parameters that get encrypted and sent to the broadcaster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcasterRawParamsTransact {
   pub transact_type: BroadcasterTransactRequestType,
   #[serde(rename = "txidVersion")]
   pub txid_version: String, // e.g. "V2_PoseidonMerkle"
   pub to: String,
   pub data: String, // hex calldata for the Railgun contract
   #[serde(rename = "broadcasterViewingKey")]
   pub broadcaster_viewing_key: String, // hex of the broadcaster's viewing pubkey
   #[serde(rename = "chainID")]
   pub chain_id: u64,
   #[serde(rename = "chainType")]
   pub chain_type: u8,
   #[serde(rename = "minGasPrice")]
   pub min_gas_price: String, // as string (bigint)
   #[serde(rename = "feesID")]
   pub fees_id: String,
   #[serde(rename = "useRelayAdapt")]
   pub use_relay_adapt: bool,
   #[serde(rename = "devLog")]
   pub dev_log: bool,
   #[serde(rename = "minVersion")]
   pub min_version: String,
   #[serde(rename = "maxVersion")]
   pub max_version: String,
   #[serde(rename = "preTransactionPOIsPerTxidLeafPerList")]
   pub pre_transaction_pois_per_txid_leaf_per_list: HashMap<String, serde_json::Value>,
   // responseKey is added by us before encryption (16 random bytes as hex)
   #[serde(rename = "responseKey", skip_serializing_if = "Option::is_none")]
   pub response_key: Option<String>,
}

/// The encrypted method params sent in the Waku message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcasterEncryptedMethodParams {
   pub pubkey: String, // hex of the random public key (from ECDH)
   #[serde(rename = "encryptedData")]
   pub encrypted_data: serde_json::Value, // the AES-GCM encrypted blob (matches TS EncryptedData)
}

/// The outer message sent on the transact topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastMessageData {
   pub method: String, // "transact"
   pub params: BroadcasterEncryptedMethodParams,
}

/// Response received on the transact-response topic (before decryption).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactResponseEnvelope {
   pub result: (String, String), // or the encrypted data structure
}

/// Decrypted response from the broadcaster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakuTransactResponse {
   pub id: String,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub tx_hash: Option<String>,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub error: Option<String>,
}
