//! Sidecar client for communicating with the Node.js Waku process.
//!
//! Architecture:
//! - Node.js sidecar (js-sidecar/src/index.js) handles ONLY Waku networking using @waku/sdk.
//! - Rust owns all Railgun logic (fees, encryption, broadcaster selection, transact requests).
//! - Communication is line-delimited JSON over stdin/stdout.
//!
//! This gives us reliable, maintained Waku while keeping the majority of code in Rust.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::WakuTransactResponse;
use crate::models::fee_message::BroadcasterFeeMessageData;
use crate::{
   BroadcasterFeeCache, BroadcasterVersionRange, Chain, SelectedBroadcaster, find_best_broadcaster,
   find_broadcasters_for_token,
};

pub const HISTORICAL_LOOKBACK_MS: u64 = 1000 * 60 * 1; // 1 min
pub const FEE_EXPIRATION_TIMEOUT_MS: u64 = 120_000; // 2 min

/// Commands sent from Rust to the Node sidecar (snake_case on the wire for cmd).
#[derive(Debug, Serialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum SidecarCommand {
   Start {
      id: u64,
      params: StartParams,
   },
   Subscribe {
      id: u64,
      params: SubscribeParams,
   },
   Publish {
      id: u64,
      params: PublishParams,
   },
   GetStatus {
      id: u64,
   },
   QueryHistorical {
      id: u64,
      params: QueryHistoricalParams,
   },
   Stop {
      id: u64,
   },
}

#[derive(Debug, Serialize)]
pub struct StartParams {
   pub chain: Chain,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub options: Option<SidecarOptions>,
}

#[derive(Debug, Serialize, Default)]
pub struct SidecarOptions {
   #[serde(skip_serializing_if = "Vec::is_empty")]
   pub additional_direct_peers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SubscribeParams {
   #[serde(rename = "contentTopics")]
   pub content_topics: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PublishParams {
   #[serde(rename = "contentTopic")]
   pub content_topic: String,
   /// Base64-encoded payload
   pub payload: String,
   #[serde(skip_serializing_if = "Option::is_none")]
   #[serde(rename = "pubsubTopic")]
   pub pubsub_topic: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QueryHistoricalParams {
   #[serde(rename = "contentTopics")]
   pub content_topics: Vec<String>,
   #[serde(rename = "timeStartMs", skip_serializing_if = "Option::is_none")]
   pub time_start_ms: Option<u64>,
   #[serde(rename = "timeEndMs", skip_serializing_if = "Option::is_none")]
   pub time_end_ms: Option<u64>,
   #[serde(rename = "pageSize", skip_serializing_if = "Option::is_none")]
   pub page_size: Option<u32>,
}

/// Messages received from the sidecar (responses + async events).
/// JS side sends camelCase, so we use rename.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SidecarMessage {
   Ready {
      version: String,
   },
   Started {
      id: Option<u64>,
      success: bool,
      #[serde(rename = "peerId", default)]
      peer_id: Option<String>,
      #[serde(default)]
      error: Option<String>,
   },
   Subscribed {
      id: Option<u64>,
      success: bool,
      #[serde(rename = "contentTopics", default)]
      content_topics: Vec<String>,
      #[serde(default)]
      error: Option<String>,
   },
   Published {
      id: Option<u64>,
      success: bool,
      #[serde(default)]
      error: Option<String>,
   },
   Status {
      started: bool,
      #[serde(rename = "meshPeers", default)]
      mesh_peers: u32,
      #[serde(rename = "pubsubPeers", default)]
      pubsub_peers: u32,
      #[serde(rename = "storePeers", default)]
      store_peers: u32,
   },
   Message {
      #[serde(rename = "contentTopic")]
      content_topic: String,
      /// Base64-encoded
      payload: String,
      timestamp: u64,
      #[serde(rename = "pubsubTopic", default)]
      pubsub_topic: Option<String>,
      #[serde(default)]
      source: Option<String>, // "live" or "historical"
   },
   PeerUpdate {
      mesh: u32,
      pubsub: u32,
   },
   Stopped {
      id: Option<u64>,
   },
   HistoricalQueried {
      id: Option<u64>,
      success: bool,
      #[serde(default)]
      count: Option<u32>,
      #[serde(default)]
      error: Option<String>,
   },
   Error {
      #[serde(default)]
      id: Option<u64>,
      #[serde(default)]
      message: Option<String>,
   },
}

/// High-level client that spawns and drives the Node.js Waku sidecar.
pub struct WakuSidecarClient {
   child: Option<Child>,
   stdin: Option<ChildStdin>,
   next_id: u64,
   event_tx: mpsc::UnboundedSender<SidecarMessage>,
   event_rx: Option<mpsc::UnboundedReceiver<SidecarMessage>>,
   fee_cache: BroadcasterFeeCache,
   chain: Chain,
   version_range: Option<BroadcasterVersionRange>,
   poi_active_list_keys: Vec<String>,
   transact_response_buffer: Vec<(String, Vec<u8>)>, // (topic, raw payload bytes)
}

impl WakuSidecarClient {
   pub fn new(chain: Chain) -> Self {
      let (tx, rx) = mpsc::unbounded_channel();
      Self {
         child: None,
         stdin: None,
         next_id: 1,
         event_tx: tx,
         event_rx: Some(rx),
         chain,
         fee_cache: BroadcasterFeeCache::new(),
         version_range: None,
         poi_active_list_keys: vec![],
         transact_response_buffer: vec![],
      }
   }

   /// Spawn the sidecar Node process and start reading stdout.
   pub async fn start_sidecar(
      &mut self,
      sidecar_entry: &str,
   ) -> Result<mpsc::UnboundedReceiver<SidecarMessage>> {
      if self.child.is_some() {
         return Err(anyhow!("Sidecar already running"));
      }

      info!("Spawning Waku sidecar: node {}", sidecar_entry);

      let mut cmd = Command::new("node");
      cmd.arg(sidecar_entry)
         .stdin(Stdio::piped())
         .stdout(Stdio::piped())
         .stderr(Stdio::inherit());

      let mut child = cmd.spawn()?;
      let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
      let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

      self.child = Some(child);
      self.stdin = Some(stdin);

      let tx = self.event_tx.clone();
      tokio::spawn(async move {
         let reader = BufReader::new(stdout);
         let mut lines = reader.lines();

         while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim();
            if line.is_empty() || !line.starts_with('{') {
               if !line.is_empty() {
                  debug!("Sidecar non-json line (ignored): {}", line);
               }
               continue;
            }
            match serde_json::from_str::<SidecarMessage>(line) {
               Ok(msg) => {
                  if tx.send(msg).is_err() {
                     break;
                  }
               }
               Err(e) => {
                  debug!("Sidecar parse error: {} | line: {}", e, line);
               }
            }
         }
         debug!("Sidecar stdout reader finished");
      });

      Ok(self.event_rx.take().expect("rx taken once"))
   }

   fn next_id(&mut self) -> u64 {
      let id = self.next_id;
      self.next_id += 1;
      id
   }

   async fn send(&mut self, cmd: &SidecarCommand) -> Result<()> {
      let stdin = self.stdin.as_mut().ok_or_else(|| anyhow!("sidecar not started"))?;
      let json = serde_json::to_string(cmd)?;
      stdin.write_all(json.as_bytes()).await?;
      stdin.write_all(b"\n").await?;
      stdin.flush().await?;
      Ok(())
   }

   pub async fn start_waku(
      &mut self,
      chain: Chain,
      options: Option<SidecarOptions>,
   ) -> Result<u64> {
      let id = self.next_id();
      let cmd = SidecarCommand::Start {
         id,
         params: StartParams { chain, options },
      };
      self.send(&cmd).await?;
      Ok(id)
   }

   pub async fn subscribe(&mut self, content_topics: Vec<String>) -> Result<u64> {
      let id = self.next_id();
      let cmd = SidecarCommand::Subscribe {
         id,
         params: SubscribeParams { content_topics },
      };
      self.send(&cmd).await?;
      Ok(id)
   }

   pub async fn publish(
      &mut self,
      content_topic: String,
      payload: Vec<u8>,
      pubsub_topic: Option<String>,
   ) -> Result<u64> {
      let id = self.next_id();
      use base64::Engine;
      let payload_b64 = base64::engine::general_purpose::STANDARD.encode(&payload);
      let cmd = SidecarCommand::Publish {
         id,
         params: PublishParams {
            content_topic,
            payload: payload_b64,
            pubsub_topic,
         },
      };
      self.send(&cmd).await?;
      Ok(id)
   }

   pub async fn get_status(&mut self) -> Result<u64> {
      let id = self.next_id();
      self.send(&SidecarCommand::GetStatus { id }).await?;
      Ok(id)
   }

   pub async fn query_historical(
      &mut self,
      content_topics: Vec<String>,
      time_start_ms: Option<u64>,
      time_end_ms: Option<u64>,
   ) -> Result<u64> {
      let id = self.next_id();
      let cmd = SidecarCommand::QueryHistorical {
         id,
         params: QueryHistoricalParams {
            content_topics,
            time_start_ms,
            time_end_ms,
            page_size: Some(100),
         },
      };
      self.send(&cmd).await?;
      Ok(id)
   }

   pub async fn stop_sidecar(&mut self) -> Result<()> {
      if let Some(_stdin) = &mut self.stdin {
         let id = self.next_id();
         let _ = self.send(&SidecarCommand::Stop { id }).await;
      }
      if let Some(mut child) = self.child.take() {
         let _ = child.kill().await;
         let _ = child.wait().await;
      }
      self.stdin = None;
      Ok(())
   }

   /// Set the accepted version range for broadcasters (e.g. "8.0.0" to "8.99.99").
   /// Fees outside this range will be rejected on add.
   pub fn set_version_range(&mut self, min: &str, max: &str) {
      self.version_range = Some(BroadcasterVersionRange {
         min_version: min.to_string(),
         max_version: max.to_string(),
      });
   }

   /// Set active POI list keys. Broadcasters that require a POI list key
   /// not in this list will have their fees rejected.
   pub fn set_poi_active_list_keys(&mut self, keys: Vec<String>) {
      self.poi_active_list_keys = keys;
   }

   /// Feed a parsed fee message into the cache.
   /// Applies version range, POI, and expiration filters.
   pub fn add_fee_message(&mut self, data: &BroadcasterFeeMessageData) {
      // Version range filtering
      if let Some(range) = &self.version_range {
         if !version_in_range(&data.version, range) {
            tracing::debug!(
               "Fee version {} outside range for broadcaster {}",
               data.version,
               data.railgun_address
            );
            return;
         }
      }

      // POI filtering (basic)
      if !self.poi_list_ok(&data.required_poi_list_keys) {
         tracing::debug!(
            "Broadcaster {} requires unavailable POI list",
            data.railgun_address
         );
         return;
      }

      self.fee_cache.add_token_fees(&self.chain, data);
   }

   pub fn chain(&self) -> Chain {
      self.chain
   }

   /// Returns the best (lowest fee) usable broadcaster for the token.
   pub fn get_best_fee_quote(&self, token_address: &str) -> Option<SelectedBroadcaster> {
      find_best_broadcaster(&self.fee_cache, &self.chain, token_address, false)
   }

   /// Returns all usable broadcasters for a token, sorted by fee (lowest first).
   pub fn get_all_fee_quotes(&self, token_address: &str) -> Vec<SelectedBroadcaster> {
      find_broadcasters_for_token(&self.fee_cache, &self.chain, token_address, false)
   }

   /// Access the underlying cache if you need advanced queries.
   pub fn fee_cache(&self) -> &BroadcasterFeeCache {
      &self.fee_cache
   }

   /// Clear the fee cache for this chain.
   pub fn clear_fee_cache(&mut self) {
      self.fee_cache.clear_for_chain(&self.chain);
   }

   /// Clear any fees from the cache that are expired.
   pub fn clear_expired_fees(&mut self) -> usize {
      self.fee_cache.clear_expired_fees(&self.chain)
   }

   /// Get last time we received any fee data (ms since epoch).
   pub fn last_received_at(&self) -> Option<u64> {
      self.fee_cache.last_received_at()
   }

   /// Feed an incoming SidecarMessage into the client (for fees + transact response buffering).
   /// Call this from your event loop when you receive SidecarMessage::Message.
   pub fn feed_message(&mut self, msg: &SidecarMessage) {
      if let SidecarMessage::Message {
         content_topic,
         payload,
         ..
      } = msg
      {
         if content_topic.contains("transact-response") {
            use base64::Engine;
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(payload) {
               self.transact_response_buffer.push((content_topic.clone(), decoded));
               // keep bounded
               if self.transact_response_buffer.len() > 50 {
                  self.transact_response_buffer.remove(0);
               }
            }
         }
         // Fee messages are handled via add_fee_message in the caller (example)
      }
   }

   /// Try to find and decrypt a pending transact-response using the given response_key (16 or 32 bytes).
   pub async fn try_get_decrypted_transact_response(
      &self,
      response_key: &[u8],
   ) -> Result<Option<WakuTransactResponse>> {
      for (_topic, raw) in &self.transact_response_buffer {
         // Try to parse as the response envelope the sidecar / broadcaster sends
         if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(raw) {
            // Many responses come as { data: "...", signature? } or directly the decrypted form
            // Try direct decrypt first
            if let Ok(decrypted) = crate::encryption::aes_gcm_decrypt(&json_val, response_key) {
               if let Ok(resp) = serde_json::from_value::<WakuTransactResponse>(decrypted) {
                  return Ok(Some(resp));
               }
            }

            // Try if it's wrapped as { "data": base64 or hex encrypted }
            if let Some(data) = json_val.get("data").and_then(|d| d.as_str()) {
               if let Ok(inner) = hex::decode(data.trim_start_matches("0x")) {
                  if let Ok(inner_json) = serde_json::from_slice::<serde_json::Value>(&inner) {
                     if let Ok(dec) = crate::encryption::aes_gcm_decrypt(&inner_json, response_key)
                     {
                        if let Ok(resp) = serde_json::from_value::<WakuTransactResponse>(dec) {
                           return Ok(Some(resp));
                        }
                     }
                  }
               }
            }
         }
      }
      Ok(None)
   }

   fn poi_list_ok(&self, required: &[String]) -> bool {
      if self.poi_active_list_keys.is_empty() {
         // If no POI lists configured, accept everything (dev / early phase)
         return true;
      }
      for key in required {
         if !self.poi_active_list_keys.iter().any(|k| k.eq_ignore_ascii_case(key)) {
            return false;
         }
      }
      true
   }

   /// Wait until we see a peer update with at least `min_mesh` connections, or timeout.
   /// Useful before transact or heavy queries.
   pub async fn wait_for_peers(
      &mut self,
      rx: &mut tokio::sync::mpsc::UnboundedReceiver<SidecarMessage>,
      min_mesh: u32,
      timeout: std::time::Duration,
   ) -> Result<u32> {
      let deadline = std::time::Instant::now() + timeout;
      let mut last_mesh = 0u32;

      while std::time::Instant::now() < deadline {
         // Ask sidecar for status
         let _ = self.get_status().await;

         if let Ok(Some(msg)) = tokio::time::timeout(std::time::Duration::from_millis(1500), rx.recv()).await {
            match msg {
               SidecarMessage::PeerUpdate { mesh, .. } => {
                  last_mesh = mesh;
                  if mesh >= min_mesh {
                     return Ok(mesh);
                  }
               }
               SidecarMessage::Status { mesh_peers, .. } => {
                  last_mesh = mesh_peers;
                  if mesh_peers >= min_mesh {
                     return Ok(mesh_peers);
                  }
               }
               SidecarMessage::Message { .. } => {
                  if last_mesh >= 1 || min_mesh <= 1 {
                     return Ok(last_mesh.max(1));
                  }
               }
               _ => {}
            }
         }
         tokio::time::sleep(std::time::Duration::from_millis(800)).await;
      }
      Ok(last_mesh)
   }

}

impl Default for WakuSidecarClient {
   fn default() -> Self {
      Self::new(Chain::ETHEREUM_MAINNET)
   }
}

impl Drop for WakuSidecarClient {
   fn drop(&mut self) {
      if let Some(mut child) = self.child.take() {
         let _ = child.start_kill();
      }
   }

   }


fn version_in_range(version: &str, range: &BroadcasterVersionRange) -> bool {
   // Very simple semver-ish comparison.
   // For production we'd use a proper semver crate, but this is sufficient for now.
   let v = parse_version(version);
   let min_v = parse_version(&range.min_version);
   let max_v = parse_version(&range.max_version);

   v >= min_v && v <= max_v
}

fn parse_version(v: &str) -> (u32, u32, u32) {
   let parts: Vec<&str> = v.trim_start_matches('v').split('.').collect();
   let major = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
   let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
   let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
   (major, minor, patch)
}
