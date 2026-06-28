//! BroadcasterTransaction implementation.
//!
//! Port of the TypeScript BroadcasterTransaction from waku-broadcaster-client.
//!
//! Flow:
//! 1. Caller selects a broadcaster using the fee cache (via WakuSidecarClient).
//! 2. Create BroadcasterTransaction with the Railgun tx details + selected broadcaster.
//! 3. `create` builds the raw params, injects responseKey, encrypts with ECDH+AES.
//! 4. `send` publishes the encrypted transact message on the /transact topic.
//! 5. Client subscribes to /transact-response topic and decrypts matching responses using responseKey.

use anyhow::{Result, anyhow};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::Chain;
use crate::SelectedBroadcaster;
use crate::client::WakuSidecarClient;
use crate::encryption::encrypt_transact_payload;
use crate::models::transact::{
   BroadcastMessageData, BroadcasterEncryptedMethodParams, BroadcasterRawParamsTransact,
   BroadcasterTransactRequestType, WakuTransactResponse,
};

/// High-level transact object. Holds the encrypted message ready to send.
pub struct BroadcasterTransaction {
   pub chain: Chain,
   pub content_topic: String,
   pub response_topic: String,
   pub message_data: BroadcastMessageData,
   /// The responseKey (hex) we generated — used to decrypt the broadcaster's reply.
   pub response_key: String,
   /// Nullifiers (for optional on-chain confirmation matching).
   pub nullifiers: Vec<String>,
}

impl BroadcasterTransaction {
   /// Create and encrypt a new transact request.
   ///
   /// `broadcaster` comes from `client.get_best_fee_quote(...)`.
   /// `to` and `data` are the target contract + calldata produced by the Railgun engine.
   pub async fn create(
      txid_version: &str,
      to: &str,
      data: &str,
      broadcaster: &SelectedBroadcaster,
      chain: Chain,
      nullifiers: Vec<String>,
      overall_batch_min_gas_price: u128,
      use_relay_adapt: bool,
   ) -> Result<Self> {
      let fees_id = broadcaster.fees_id.clone();

      // The broadcaster's viewing key is needed for ECDH.
      // For now we take it from the SelectedBroadcaster if present, or derive placeholder.
      // In a full integration the SelectedBroadcaster or fee message should carry the viewing key,
      // or we decode the railgun_address.
      let viewing_key_bytes: [u8; 32] = match &broadcaster.viewing_public_key {
         Some(v) => {
            let bytes = hex::decode(v.trim_start_matches("0x"))?;
            if bytes.len() != 32 {
               return Err(anyhow!("viewing public key must be 32 bytes"));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
         }
         None => {
            // Fallback: use a hash of the railgun address as demo key (NOT for production)
            warn!("No viewing_public_key on SelectedBroadcaster — using demo derivation");
            let mut arr = [0u8; 32];
            let hash = [0u8; 32]; // TODO: proper viewing key derivation
            arr.copy_from_slice(&hash);
            arr
         }
      };

      let mut raw = BroadcasterRawParamsTransact {
         transact_type: BroadcasterTransactRequestType::Common,
         txid_version: txid_version.to_string(),
         to: to.to_string(),
         data: data.to_string(),
         broadcaster_viewing_key: hex::encode(&viewing_key_bytes),
         chain_id: chain.id,
         chain_type: chain.type_,
         min_gas_price: overall_batch_min_gas_price.to_string(),
         fees_id,
         use_relay_adapt,
         dev_log: false,
         min_version: "8.0.0".to_string(),
         max_version: "9.0.0".to_string(),
         pre_transaction_pois_per_txid_leaf_per_list: Default::default(),
         response_key: None,
      };

      let (random_pubkey, encrypted_data) = encrypt_transact_payload(&mut raw, &viewing_key_bytes)?;

      let message_data = BroadcastMessageData {
         method: "transact".to_string(),
         params: BroadcasterEncryptedMethodParams {
            pubkey: random_pubkey,
            encrypted_data,
         },
      };

      // We also need to store the response_key that was injected (it is in raw now)
      let response_key = raw
         .response_key
         .clone()
         .ok_or_else(|| anyhow!("response_key was not set during encryption"))?;

      let content_topic = format!(
         "/railgun/v2/{}-{}-transact/json",
         chain.type_, chain.id
      );
      let response_topic = format!(
         "/railgun/v2/{}-{}-transact-response/json",
         chain.type_, chain.id
      );

      Ok(Self {
         chain,
         content_topic,
         response_topic,
         message_data,
         response_key,
         nullifiers,
      })
   }

   /// Send the transact request via the sidecar and wait for a response.
   ///
   /// This publishes the message and then polls the response topic (or relies on subscription).
   pub async fn send(&self, client: &mut WakuSidecarClient) -> Result<WakuTransactResponse> {
      info!(
         "Sending encrypted transact to broadcaster on {}",
         self.content_topic
      );

      // Publish the transact message
      let payload = serde_json::to_string(&self.message_data)?;
      client
         .publish(
            self.content_topic.clone(),
            payload.into_bytes(),
            None,
         )
         .await?;

      // Ensure we are subscribed to the response topic
      client.subscribe(vec![self.response_topic.clone()]).await?;

      // Poll for a response that we can decrypt with our response_key.
      // In a real client this would be driven by incoming messages from the sidecar.
      // For now we do a simple poll loop (the sidecar example can be extended to push responses).
      let response_key_bytes = hex::decode(&self.response_key)?;

      for attempt in 0..60 {
         // up to ~30s
         if let Some(resp) = client.try_get_decrypted_transact_response(&response_key_bytes).await?
         {
            info!("Received decrypted transact response: {:?}", resp);
            return Ok(resp);
         }
         sleep(Duration::from_millis(500)).await;
         if attempt % 10 == 0 {
            debug!(
               "Still waiting for broadcaster response (attempt {})",
               attempt
            );
         }
      }

      Err(anyhow!(
         "Timed out waiting for broadcaster transact response"
      ))
   }
}

// Extension on the client for transact support (keeps all logic in one owner).
impl WakuSidecarClient {
   /// Convenience: create + send in one go.
   pub async fn transact(
      &mut self,
      txid_version: &str,
      to: &str,
      data: &str,
      broadcaster: &SelectedBroadcaster,
      nullifiers: Vec<String>,
      min_gas_price: u128,
      use_relay_adapt: bool,
   ) -> Result<WakuTransactResponse> {
      let tx = BroadcasterTransaction::create(
         txid_version,
         to,
         data,
         broadcaster,
         self.chain(),
         nullifiers,
         min_gas_price,
         use_relay_adapt,
      )
      .await?;

      tx.send(self).await
   }

   /// Try to decrypt any buffered transact-response messages using the given key.
   /// This is a placeholder until we wire full response streaming from the sidecar.
   pub async fn try_get_decrypted_transact_response(
      &self,
      response_key: &[u8],
   ) -> Result<Option<WakuTransactResponse>> {
      // In a complete implementation the sidecar would forward messages from the response topic
      // and we would have a buffer of raw encrypted responses here.
      // For the first pass we return None (the send loop above will keep polling after publish).
      // You can extend the SidecarMessage enum + handler to populate an internal response queue.
      let _ = response_key;
      Ok(None)
   }
}
