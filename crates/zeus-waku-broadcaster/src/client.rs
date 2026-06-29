//! Sidecar client for communicating with the Node.js Waku process.
//!
//! Architecture:
//! - Node.js sidecar (js-sidecar/src/index.js) handles ONLY Waku networking using @waku/sdk.
//! - Rust owns all Railgun logic (fees, encryption, broadcaster selection, transact requests).
//! - Communication is line-delimited JSON over stdin/stdout.
//!
//! This gives us reliable, maintained Waku while keeping the majority of code in Rust.

use anyhow::{Result, anyhow};
use kanal::{AsyncReceiver, AsyncSender, unbounded};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};

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
   GetPeers {
      id: u64,
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

/// Detailed peer information returned by get_peers().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
   #[serde(rename = "peerId")]
   pub peer_id: String,
   #[serde(rename = "multiaddr", default)]
   pub multiaddr: Option<String>,
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
   Peers {
      id: Option<u64>,
      peers: Vec<PeerInfo>,
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
///
/// Thread-safe and cheap to clone (shares the sidecar process via Arc<Mutex>).
/// Each clone gets its own clone of the event receiver (kanal supports this).
#[derive(Clone)]
pub struct WakuSidecarClient {
   inner: Arc<Mutex<ClientInner>>,
   event_rx: AsyncReceiver<SidecarMessage>,
   chain: Chain,
}

/// Shared mutable state behind the client.
struct ClientInner {
   child: Option<Child>,
   stdin: Option<ChildStdin>,
   next_id: u64,
   event_tx: AsyncSender<SidecarMessage>,
   fee_cache: BroadcasterFeeCache,
   chain: Chain,
   version_range: Option<BroadcasterVersionRange>,
   poi_active_list_keys: Vec<String>,
   transact_response_buffer: Vec<(String, Vec<u8>)>, // (topic, raw payload bytes)
}

impl WakuSidecarClient {
   pub fn new(chain: Chain) -> Self {
      let (tx, rx) = unbounded();
      let inner = ClientInner {
         child: None,
         stdin: None,
         next_id: 1,
         event_tx: tx.to_async(),
         fee_cache: BroadcasterFeeCache::new(),
         chain,
         version_range: None,
         poi_active_list_keys: vec![],
         transact_response_buffer: vec![],
      };
      Self {
         inner: Arc::new(Mutex::new(inner)),
         event_rx: rx.to_async(),
         chain,
      }
   }

   /// Spawn the sidecar Node process and start reading stdout.
   pub async fn start_sidecar(&self, sidecar_entry: &str) -> Result<AsyncReceiver<SidecarMessage>> {
      {
         let inner = self.inner.lock().await;
         if inner.child.is_some() {
            return Err(anyhow!("Sidecar already running"));
         }
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

      {
         let mut inner = self.inner.lock().await;
         inner.child = Some(child);
         inner.stdin = Some(stdin);
      }

      let tx = {
         let inner = self.inner.lock().await;
         inner.event_tx.clone()
      };

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
               Ok(msg) => match tx.send(msg).await {
                  Ok(_) => {}
                  Err(e) => {
                     debug!("Sidecar send error: {}", e);
                     break;
                  }
               },
               Err(e) => {
                  debug!("Sidecar parse error: {} | line: {}", e, line);
               }
            }
         }
         debug!("Sidecar stdout reader finished");
      });

      let rx = self.event_rx.clone();
      Ok(rx)
   }

   async fn next_id(&self) -> u64 {
      let mut inner = self.inner.lock().await;
      let id = inner.next_id;
      inner.next_id += 1;
      id
   }

   async fn send(&self, cmd: &SidecarCommand) -> Result<()> {
      let mut inner = self.inner.lock().await;
      let stdin = inner.stdin.as_mut().ok_or_else(|| anyhow!("sidecar not started"))?;
      let json = serde_json::to_string(cmd)?;
      stdin.write_all(json.as_bytes()).await?;
      stdin.write_all(b"\n").await?;
      stdin.flush().await?;
      Ok(())
   }

   pub async fn start_waku(&self, chain: Chain, options: Option<SidecarOptions>) -> Result<u64> {
      let id = self.next_id().await;
      let cmd = SidecarCommand::Start {
         id,
         params: StartParams { chain, options },
      };
      self.send(&cmd).await?;
      Ok(id)
   }

   pub async fn subscribe(&self, content_topics: Vec<String>) -> Result<u64> {
      let id = self.next_id().await;
      let cmd = SidecarCommand::Subscribe {
         id,
         params: SubscribeParams { content_topics },
      };
      self.send(&cmd).await?;
      Ok(id)
   }

   pub async fn publish(
      &self,
      content_topic: String,
      payload: Vec<u8>,
      pubsub_topic: Option<String>,
   ) -> Result<u64> {
      let id = self.next_id().await;
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

   pub async fn get_status(&self) -> Result<u64> {
      let id = self.next_id().await;
      self.send(&SidecarCommand::GetStatus { id }).await?;
      Ok(id)
   }

   pub async fn query_historical(
      &self,
      content_topics: Vec<String>,
      time_start_ms: Option<u64>,
      time_end_ms: Option<u64>,
   ) -> Result<u64> {
      let id = self.next_id().await;
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

   pub async fn stop_sidecar(&self) -> Result<()> {
      // First get ids and take ownership without holding across awaits if possible
      let (has_stdin, stop_id, child_opt) = {
         let mut inner = self.inner.lock().await;
         let has = inner.stdin.is_some();
         let sid = inner.next_id;
         let ch = inner.child.take();
         inner.stdin = None;
         (has, sid, ch)
      };
      if has_stdin {
         let _ = self.send(&SidecarCommand::Stop { id: stop_id }).await;
      }
      if let Some(mut child) = child_opt {
         let _ = child.kill().await;
         let _ = child.wait().await;
      }
      Ok(())
   }

   /// Set the accepted version range for broadcasters (e.g. "8.0.0" to "8.99.99").
   /// Fees outside this range will be rejected on add.
   pub async fn set_version_range(&self, min: &str, max: &str) {
      let mut inner = self.inner.lock().await;
      inner.version_range = Some(BroadcasterVersionRange {
         min_version: min.to_string(),
         max_version: max.to_string(),
      });
   }

   /// Set active POI list keys. Broadcasters that require a POI list key
   /// not in this list will have their fees rejected.
   pub async fn set_poi_active_list_keys(&self, keys: Vec<String>) {
      let mut inner = self.inner.lock().await;
      inner.poi_active_list_keys = keys;
   }

   /// Feed a parsed fee message into the cache (async because of locks).
   /// Applies version range, POI filtering.
   pub async fn add_fee_message(&self, data: BroadcasterFeeMessageData) {
      if data.version.is_empty() {
         return;
      }
      let (version_range, poi_ok, chain) = {
         let inner = self.inner.lock().await;
         let poi_ok = if inner.poi_active_list_keys.is_empty() {
            true
         } else {
            data
               .required_poi_list_keys
               .iter()
               .any(|k| inner.poi_active_list_keys.contains(k))
         };
         (inner.version_range.clone(), poi_ok, inner.chain)
      };
      if let Some(range) = &version_range {
         if !version_in_range(&data.version, range) {
            debug!(
               "Broadcaster version {} outside allowed range",
               data.version
            );
            return;
         }
      }
      if !poi_ok {
         debug!(
            "Broadcaster {} requires unavailable POI list",
            data.railgun_address
         );
         return;
      }
      let mut inner = self.inner.lock().await;
      inner.fee_cache.add_token_fees(&chain, &data);
   }

   /// Request current connected peers from the sidecar (libp2p connections).
   /// Returns detailed PeerInfo (peerId + multiaddr when available).
   pub async fn get_peers(&self) -> Result<Vec<PeerInfo>> {
      let id = self.next_id().await;
      let cmd = SidecarCommand::GetPeers { id };
      self.send(&cmd).await?;

      // Wait briefly for the peers response (diagnostic API)
      let rx = self.event_rx.clone();
      let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
      while tokio::time::Instant::now() < deadline {
         if let Ok(Ok(msg)) = timeout(Duration::from_millis(300), rx.recv()).await {
            if let SidecarMessage::Peers {
               id: Some(resp_id),
               peers,
            } = msg
            {
               if resp_id == id {
                  return Ok(peers);
               }
            }
         }
      }
      Err(anyhow!("timeout waiting for peers response"))
   }

   pub fn chain(&self) -> Chain {
      self.chain
   }

   /// Returns the best (lowest fee) usable broadcaster for the token.
   pub async fn get_best_fee_quote(&self, token_address: &str) -> Option<SelectedBroadcaster> {
      let inner = self.inner.lock().await;
      find_best_broadcaster(
         &inner.fee_cache,
         &inner.chain,
         token_address,
         false,
      )
   }

   /// Returns all usable broadcasters for a token, sorted by fee (lowest first).
   pub async fn get_all_fee_quotes(&self, token_address: &str) -> Vec<SelectedBroadcaster> {
      let inner = self.inner.lock().await;
      find_broadcasters_for_token(
         &inner.fee_cache,
         &inner.chain,
         token_address,
         false,
      )
   }

   /// Access the underlying cache if you need advanced queries.
   pub async fn fee_cache(&self) -> BroadcasterFeeCache {
      let inner = self.inner.lock().await;
      inner.fee_cache.clone()
   }

   /// Clear the fee cache for this chain.
   pub async fn clear_fee_cache(&self) {
      let mut inner = self.inner.lock().await;
      let ch = inner.chain;
      inner.fee_cache.clear_for_chain(&ch);
   }

   /// Clear any fees from the cache that are expired.
   pub async fn clear_expired_fees(&self) -> usize {
      let mut inner = self.inner.lock().await;
      let ch = inner.chain;
      inner.fee_cache.clear_expired_fees(&ch)
   }

   /// Get last time we received any fee data (ms since epoch).
   pub async fn last_received_at(&self) -> Option<u64> {
      let inner = self.inner.lock().await;
      inner.fee_cache.last_received_at()
   }

   /// Feed an incoming SidecarMessage into the client (for fees + transact response buffering).
   /// Call this from your event loop when you receive SidecarMessage::Message.
   pub async fn feed_message(&self, msg: &SidecarMessage) {
      if let SidecarMessage::Message {
         content_topic,
         payload,
         ..
      } = msg
      {
         if content_topic.contains("transact-response") {
            use base64::Engine;
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(payload) {
               let mut inner = self.inner.lock().await;
               inner.transact_response_buffer.push((content_topic.clone(), decoded));
               // keep bounded
               if inner.transact_response_buffer.len() > 50 {
                  inner.transact_response_buffer.remove(0);
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
      let buffer = {
         let inner = self.inner.lock().await;
         inner.transact_response_buffer.clone()
      };
      for (_topic, raw) in &buffer {
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

   pub async fn poi_list_ok(&self, required: &[String]) -> bool {
      let inner = self.inner.lock().await;
      if inner.poi_active_list_keys.is_empty() {
         return true;
      }
      required
         .iter()
         .any(|k| inner.poi_active_list_keys.iter().any(|ik| ik.eq_ignore_ascii_case(k)))
   }

   /// Wait until we see a peer update with at least `min_mesh` connections, or timeout.
   /// Useful before transact or heavy queries.
   pub async fn wait_for_peers(&self, min_mesh: u32, timeout_dur: Duration) -> Result<u32> {
      let deadline = Instant::now() + timeout_dur;
      let mut last_mesh = 0u32;
      let rx = self.event_rx.clone();

      while Instant::now() < deadline {
         // Ask sidecar for status
         let _ = self.get_status().await;

         if let Ok(msg) = timeout(Duration::from_millis(1500), rx.recv()).await {
            match msg {
               Ok(SidecarMessage::PeerUpdate { mesh, .. }) => {
                  last_mesh = mesh;
                  if mesh >= min_mesh {
                     return Ok(mesh);
                  }
               }
               Ok(SidecarMessage::Status { mesh_peers, .. }) => {
                  last_mesh = mesh_peers;
                  if mesh_peers >= min_mesh {
                     return Ok(mesh_peers);
                  }
               }
               Ok(SidecarMessage::Message { .. }) => {
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
      // Best effort: try to take the child without blocking
      if let Ok(mut inner) = self.inner.try_lock() {
         if let Some(mut child) = inner.child.take() {
            let _ = child.start_kill();
         }
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
