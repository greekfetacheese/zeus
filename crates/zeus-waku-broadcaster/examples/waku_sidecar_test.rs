//! Simple integration example to verify the Waku sidecar runs and can receive Railgun messages.
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
//! Expectation: first cold start can still take 1-5+ minutes to see mesh_peers > 0.
//! Subsequent runs are usually faster once the peer store has data.

use std::time::Duration;

use tokio::time::sleep;
use tracing::info;
use zeus_waku_broadcaster::sidecar::{Chain, SidecarMessage, WakuSidecarClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Simple logging setup (no extra dev-dep needed beyond what's there)
    // For nicer output you can run with RUST_LOG=info cargo ...
    tracing_subscriber::fmt()
        .with_env_filter("info,zeus_waku_broadcaster=debug")
        .init();

    info!("=== Zeus Waku Sidecar Test ===");

    let mut client = WakuSidecarClient::new();

    // Path is relative to the crate root when running via `cargo run --example`
    let sidecar_path = "crates/zeus-waku-broadcaster/js-sidecar/src/index.js";

    info!("Spawning sidecar: {}", sidecar_path);
    let mut rx = match client.start_sidecar(sidecar_path).await {
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
            Some(msg) = rx.recv() => {
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
    let chain = Chain { type_: 0, id: 1 };

    info!("Starting Waku light node on chain type={} id={}", chain.type_, chain.id);
    let start_id = client.start_waku(chain.clone(), None).await?;
    info!("Sent start command (id={})", start_id);

    // Wait for the 'started' event from sidecar (event-driven, not fixed sleep)
    info!("Waiting for Waku 'started' confirmation from sidecar (this can take time for peer discovery + store waits)...");
    let mut started_ok = false;
    let start_wait = Duration::from_secs(180);  // sidecar does long store waits before sending 'started'
    let start_deadline = std::time::Instant::now() + start_wait;

    while std::time::Instant::now() < start_deadline {
        if let Ok(Some(msg)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            match msg {
                SidecarMessage::Started { success, peer_id, error, .. } => {
                    if success {
                        info!("Waku started successfully. Peer ID: {:?}", peer_id);
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
        info!("Did not receive 'started' within {:?}. Proceeding anyway (sidecar is doing long store waits before started).", start_wait);
    }

    // Railgun fees topic for this chain
    let fees_topic = format!("/railgun/v2/{}-{}-fees/json", chain.type_, chain.id);
    info!("Subscribing to fees topic: {}", fees_topic);

    let sub_id = client.subscribe(vec![fees_topic.clone()]).await?;
    info!("Subscribe command sent (id={})", sub_id);

    // Wait for some peers before querying historical (Store needs connected peers)
    info!("Waiting for peers before issuing historical Store query (Store protocol needs mesh peers)...");
    let wait_for_peers = Duration::from_secs(180);
    let peer_wait_start = std::time::Instant::now();
    let mut peers_ready = false;
    let mut last_mesh = 0u32;

    while peer_wait_start.elapsed() < wait_for_peers {
        // Poll status and wait for peer updates
        let _ = client.get_status().await;
        if let Ok(Some(msg)) = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            match msg {
                SidecarMessage::PeerUpdate { mesh, .. } => {
                    last_mesh = mesh;
                    info!("Peer update during wait: mesh={}", mesh);
                    if mesh >= 3 {
                        peers_ready = true;
                        break;
                    }
                }
                SidecarMessage::Status { mesh_peers, .. } => {
                    last_mesh = mesh_peers;
                    if mesh_peers >= 3 {
                        peers_ready = true;
                        break;
                    }
                }
                SidecarMessage::Message { .. } => {
                    // If we get any message early, great
                    peers_ready = true;
                    break;
                }
                _ => {}
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    if peers_ready {
        info!("Peers ready (mesh ~{}), now querying historical fee messages (5m window)...", last_mesh);
    } else {
        info!("No strong peer count after {}s (last mesh={}), will still try historical query (5m window with multi-peer logic) + rely on live.", 
              wait_for_peers.as_secs(), last_mesh);
    }

    // Always use a small recent window for fee quotes (broadcasters republish often)
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let ago = now_ms.saturating_sub(1000 * 60 * 5); // 5 min default (fees republish often)

    let hist_id = client.query_historical(
        vec![fees_topic.clone()],
        Some(ago),
        Some(now_ms),
    ).await?;
    info!("Historical query command sent (id={})", hist_id);

    info!("Now listening for incoming Waku messages (live + historical)...");
    info!("(Fee messages may arrive via historical query or live. We will also retry historical periodically if needed.)");
    info!("Running for 900 seconds. Press Ctrl+C to stop early.");

    let listen_duration = Duration::from_secs(900);
    let start_time = std::time::Instant::now();
    let mut message_count = 0u32;
    let mut last_historical_query = std::time::Instant::now();

    while start_time.elapsed() < listen_duration {
        tokio::select! {
            Some(msg) = rx.recv() => {
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
                        message_count += 1;
                        let src = source.as_deref().unwrap_or("live");
                        info!(
                            "📨 MESSAGE #{} | source={} | topic={} | ts={} | len={}",
                            message_count,
                            src,
                            content_topic,
                            timestamp,
                            payload.len()
                        );

                        // Try to parse as Railgun fee message
                        use zeus_waku_broadcaster::models::SignedBroadcasterFeeMessage;

                        // payload here is base64 string from sidecar
                        use base64::Engine;
                        let decoded = base64::engine::general_purpose::STANDARD.decode(&payload).unwrap_or_default();
                        match SignedBroadcasterFeeMessage::from_waku_payload(&decoded) {
                            Ok(signed) => {
                                if let Ok(fee) = signed.parse_inner_data() {
                                    info!("   ✅ Fee from {} | railgun={} | version={} | tokens={}",
                                        src,
                                        fee.railgun_address,
                                        fee.version,
                                        fee.fees.len()
                                    );
                                }
                            }
                            Err(_) => {
                                let preview = if payload.len() > 80 { format!("{}...", &payload[..80]) } else { payload.clone() };
                                info!("   (non-fee) preview: {}", preview);
                            }
                        }
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

                // Periodically retry historical query if we have peers but very few messages
                // (fee announcements are not frequent; Store can surface older ones)
                if message_count < 3 && last_historical_query.elapsed() > Duration::from_secs(30) {
                    info!("Few messages so far — re-issuing historical query (last 5m) to catch fee announcements...");
                    let hist_start = (std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 5))
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

    info!("=== Test complete. Received {} messages ===", message_count);
    info!("If you saw 'message' events above, the sidecar + Waku connection is working!");

    Ok(())
}
