//! Simple integration example to verify the Waku sidecar runs and can receive Railgun messages (live + historical via Store).
//!
//! Run with:
//!   cargo run -p zeus-waku-broadcaster --example waku_sidecar_test
//!
//! Prerequisites:
//!   cd crates/zeus-waku-broadcaster/js-sidecar && npm install
//!
//! This will:
//! - Spawn the Node.js sidecar
//! - Start a Waku light node on Ethereum mainnet using the *real* Railgun discovery
//!   (ENR tree + peer exchange) **plus** hardcoded known Railgun relays for fast bootstrap.
//! - Subscribe to the fee announcement topic (/railgun/v2/0-1-fees/json)
//! - Print peer counts (via PeerUpdate + periodic Status) and any received messages
//! - Run for a long time (Ctrl+C to stop)
//!
//! The sidecar now:
//!   - Dials the 3 known Railgun relays immediately (relay-a, relay-b, client-edge)
//!   - Uses DNS ENR tree + Peer Exchange (same as official @railgun-community client)
//!   - Falls back to defaultBootstrap
//!
//! Expectation (2026-06-28 continued fixes):
//! - Explicit `discovery: {dns, peerExchange, peerCache}` passed + peerDiscovery in libp2p.
//! - Bootstrap dials are now non-blocking with 5s timeout per peer (no more long blocking delays).
//! - Redundant historical queries heavily reduced (guard in sidecar + example only does one initial + rare fallback).
//! - Usable mesh (2-3) often fast with good peers (WSS recommended).
//! - Historical delivers quickly.
//! - Live still the focus: after subscribe you should see filter peers logged. If live fees appear, great. If not, next iteration may need more relay/filter tuning.
//! - Sidecar now logs filter peer count after subscribe for diagnostics.

use std::time::Duration;

use tokio::time::sleep;
use tracing::info;
use zeus_waku_broadcaster::client::{SidecarMessage, WakuSidecarClient};
use zeus_waku_broadcaster::{Chain, fees_topic};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
   // Simple logging setup (no extra dev-dep needed beyond what's there)
   // For nicer output you can run with RUST_LOG=info cargo ...
   tracing_subscriber::fmt()
      .with_env_filter("info,zeus_waku_broadcaster=debug")
      .init();

   info!("=== Zeus Waku Sidecar Test ===");

   let client = WakuSidecarClient::new(Chain::ETHEREUM_MAINNET);

   // Path is relative to the crate root when running via `cargo run --example`
   let sidecar_path = "crates/zeus-waku-broadcaster/js-sidecar/src/index.js";

   info!("Spawning sidecar: {}", sidecar_path);
   let rx = match client.start_sidecar(sidecar_path).await {
      Ok(rx) => rx,
      Err(e) => {
         eprintln!("Failed to start sidecar. Make sure you ran `npm install` in js-sidecar/");
         eprintln!("Error: {}", e);
         return Err(e);
      }
   };

   info!("Sidecar process started. Waiting for 'ready' event...");

   // Wait for ready (with timeout)
   let ready_timeout = sleep(Duration::from_secs(5));
   tokio::pin!(ready_timeout);

   loop {
      tokio::select! {
          Ok(msg) = rx.recv() => {
              if let SidecarMessage::Ready { version } = msg {
                  info!("Sidecar ready (version {})", version);
                  break;
              }
          }
          _ = &mut ready_timeout => {
              info!("No ready message yet, continuing anyway...");
              break;
          }
      }
   }

   // Use Ethereum mainnet (matches Railgun)
   let sidecar_chain = Chain { type_: 0, id: 1 };

   info!(
      "Starting Waku light node on chain type={} id={}",
      sidecar_chain.type_, sidecar_chain.id
   );
   let start_id = client.start_waku(sidecar_chain.clone(), None).await?;
   info!("Sent start command (id={})", start_id);

   // Wait for the 'started' event from sidecar (event-driven, not fixed sleep)
   info!(
      "Waiting for Waku 'started' confirmation from sidecar (this can take time for peer discovery + store waits)..."
   );
   let mut started_ok = false;
   let start_wait = Duration::from_secs(60); // sidecar does long store waits before sending 'started'
   let start_deadline = std::time::Instant::now() + start_wait;

   while std::time::Instant::now() < start_deadline {
      if let Ok(msg) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
         match msg {
            Ok(SidecarMessage::Started {
               success,
               peer_id,
               error,
               ..
            }) => {
               if success {
                  info!(
                     "Waku started successfully. Peer ID: {:?}",
                     peer_id
                  );
                  started_ok = true;
               } else {
                  tracing::error!("Waku start failed: {:?}", error);
               }
               break;
            }
            other => {
               // Print other early events
               info!("Early event while waiting for start: {:?}", other);
            }
         }
      }
   }

   if !started_ok {
      info!(
         "Did not receive 'started' within {:?}. Proceeding anyway (sidecar is doing long store waits before started).",
         start_wait
      );
   }

   // Railgun fees topic for this chain
   let fees_topic = fees_topic(sidecar_chain);
   info!("Subscribing to fees topic: {}", fees_topic);

   let sub_id = client.subscribe(vec![fees_topic.clone()]).await?;
   info!("Subscribe command sent (id={})", sub_id);

   // Use the client's helper to wait for usable mesh (improves reliability)
   info!("Waiting for usable peer mesh (using client.wait_for_peers helper)...");
   let mesh = client.wait_for_peers(2, Duration::from_secs(90)).await?;
   info!(
      "Mesh after wait: {} (proceeding to historical query + live feed)",
      mesh
   );

   if let Ok(peers) = client.get_peers().await {
      info!("get_peers() -> {} connected peers", peers.len());
      for p in peers.iter().take(3) {
         info!("  peer: {} @ {:?}", p.peer_id, p.multiaddr);
      }
   }

   if mesh < 2 {
      info!(
         "Low mesh ({}); historical may be slow but multi-peer fallback + live should still work. Discovery continues in background.",
         mesh
      );
   }

   // Always use a small recent window for fee quotes (broadcasters republish often)
   let now_ms = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_millis() as u64;
   let ago = now_ms.saturating_sub(1000 * 60 * 1); // 1 min default (fees republish often)

   let hist_id = client
      .query_historical(vec![fees_topic.clone()], Some(ago), Some(now_ms))
      .await?;
   info!("Historical query command sent (id={})", hist_id);

   info!("Now listening for incoming Waku messages (live + historical)...");
   info!(
      "(Fee messages may arrive via historical query or live. We will also retry historical periodically if needed.)"
   );
   info!("Running for 900 seconds. Press Ctrl+C to stop early.");

   let listen_duration = Duration::from_secs(900);
   let start_time = std::time::Instant::now();
   let mut message_count = 0u32;
   let mut last_historical_query = std::time::Instant::now();

   while start_time.elapsed() < listen_duration {
      tokio::select! {
          Ok(msg) = rx.recv() => {
              match msg {
                  SidecarMessage::Started { success, peer_id, error, .. } => {
                      if success {
                          info!("Waku started successfully. Peer ID: {:?}", peer_id);
                      } else {
                          tracing::error!("Waku start failed: {:?}", error);
                      }
                  }
                  SidecarMessage::Subscribed { success, content_topics, error, .. } => {
                      if success {
                          info!("Successfully subscribed to: {:?}", content_topics);
                      } else {
                          tracing::warn!("Subscribe failed: {:?}", error);
                      }
                  }
                  SidecarMessage::Message { content_topic, payload, timestamp, source, .. } => {
                  let src = source.clone().unwrap_or_else(|| "live".to_string());
                      message_count += 1;
                      // src defaulted above

                      // Avoid spamming the log with historical messages
                      if src == "live" {
                      info!(
                          "📨 MESSAGE #{} | source={} | topic={} | ts={} | len={}",
                          message_count,
                          src,
                          content_topic,
                          timestamp,
                          payload.len()
                      );
                     }

                      // Try to parse as Railgun fee message
                      use zeus_waku_broadcaster::models::SignedBroadcasterFeeMessage;
                      use base64::Engine;
                      let decoded = base64::engine::general_purpose::STANDARD.decode(&payload).unwrap_or_default();
                      match SignedBroadcasterFeeMessage::from_waku_payload(&decoded) {
                          Ok(signed) => {
                              if let Ok(fee_data) = signed.parse_inner_data() {
                                  client.add_fee_message(fee_data.clone()).await;

                                  if message_count <= 8 {
                                      info!(
                                          "   ✅ Fee from {} | railgun={} | version={} | tokens={}",
                                          src,
                                          fee_data.railgun_address,
                                          fee_data.version,
                                          fee_data.fees.len()
                                      );
                                  }

                                  if message_count % 50 == 0 && message_count > 0 {
                                      let usdc = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
                                      if let Some(best) = client.get_best_fee_quote(usdc).await {
                                          info!(
                                              "   📊 Best broadcaster for USDC (summary): {} @ {} gas units",
                                              &best.railgun_address[..30], best.token_fee.fee_per_unit_gas
                                          );
                                      }
                                  }
                              }
                          }
                          Err(_) => {
                              let preview = if payload.len() > 80 { format!("{}...", &payload[..80]) } else { payload.clone() };
                              info!("   (non-fee) preview: {}", preview);
                          }
                      }

                      // IMPORTANT: Feed every message to the client for transact response buffering
                      client.feed_message(&SidecarMessage::Message {
                          content_topic: content_topic.clone(),
                          payload: payload.clone(),
                          timestamp,
                          pubsub_topic: None,
                          source: source.clone(),
                      }).await;
                  }
                  SidecarMessage::PeerUpdate { mesh, pubsub } => {
                      info!("Peer update: mesh_peers={}, pubsub_peers={}", mesh, pubsub);
                  }
                  SidecarMessage::Status { started, mesh_peers, pubsub_peers, store_peers, .. } => {
                      info!("Status: started={}, mesh={}, pubsub={}, store={}", started, mesh_peers, pubsub_peers, store_peers);
                  }
                  SidecarMessage::HistoricalQueried { success, count, error, .. } => {
                      if success {
                          info!("Historical query completed. Messages delivered: {:?}", count);
                      } else {
                          tracing::warn!("Historical query failed: {:?}", error);
                      }
                  }
                  SidecarMessage::Error { message, .. } => {
                      tracing::error!("Sidecar error: {:?}", message);
                  }
                  _ => {
                      // Ignore other events for this test
                  }
              }
          }
          _ = sleep(Duration::from_secs(1)) => {
              // Periodic heartbeat + status poll
              let secs = start_time.elapsed().as_secs();
              if secs % 15 == 0 {
                  info!("... still listening ({}s elapsed, {} messages so far)", secs, message_count);
                  let _ = client.get_status().await;
              }

              let fees_removed = client.clear_expired_fees().await;
              if fees_removed > 0 {
                  info!("Removed {} expired fees from cache", fees_removed);
              }

              // Periodically retry historical query if we have peers but very few messages
              // (fee announcements are not frequent; Store can surface older ones)
              if message_count < 3 && last_historical_query.elapsed() > Duration::from_secs(180) {
                  info!("Few messages so far — re-issuing historical query (last 1m) to catch fee announcements...");
                  let hist_start = (std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 1))
                      .duration_since(std::time::UNIX_EPOCH)
                      .unwrap()
                      .as_millis() as u64;
                  if let Err(e) = client.query_historical(vec![fees_topic.clone()], Some(hist_start), None).await {
                      tracing::warn!("Periodic historical query failed: {}", e);
                  } else {
                      last_historical_query = std::time::Instant::now();
                  }
              }
          }
      }
   }

   info!("Test time expired. Stopping sidecar...");
   if let Err(e) = client.stop_sidecar().await {
      tracing::warn!("Error stopping sidecar: {}", e);
   }

   info!(
      "=== Test complete. Received {} messages ===",
      message_count
   );
   info!(
      "Fee cache last updated: {:?}",
      client.last_received_at().await
   );

   // Final summary: show best for a few common tokens
   let tokens = [
      (
         "USDC",
         "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
      ),
      (
         "WETH",
         "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
      ),
   ];
   for (name, addr) in tokens {
      if let Some(best) = client.get_best_fee_quote(addr).await {
         info!(
            "Best for {}: {} fee={}",
            name,
            &best.railgun_address.chars().take(30).collect::<String>(),
            best.token_fee.fee_per_unit_gas
         );
      } else {
         info!(
            "No usable broadcaster found for {} in cache",
            name
         );
      }
   }
   info!("If you saw 'message' events above, the sidecar + Waku connection is working!");

   Ok(())
}
