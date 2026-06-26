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
    info!("Waiting for Waku 'started' confirmation from sidecar (this can take time for peer discovery)...");
    let mut started_ok = false;
    let start_wait = Duration::from_secs(60);
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
        info!("Did not receive successful 'started' within {:?}. Proceeding anyway (we now dial known Railgun relays + use DNS discovery).", start_wait);
    }

    // Railgun fees topic for this chain
    let fees_topic = format!("/railgun/v2/{}-{}-fees/json", chain.type_, chain.id);
    info!("Subscribing to fees topic: {}", fees_topic);

    let sub_id = client.subscribe(vec![fees_topic.clone()]).await?;
    info!("Subscribe command sent (id={})", sub_id);

    info!("Now listening for incoming Waku messages...");
    info!("(You should start seeing 'message' events if broadcasters are active)");
    info!("Running for 900 seconds. Press Ctrl+C to stop early.");

    let listen_duration = Duration::from_secs(900);
    let start_time = std::time::Instant::now();
    let mut message_count = 0u32;

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
                    SidecarMessage::Message { content_topic, payload, timestamp, .. } => {
                        message_count += 1;
                        info!(
                            "📨 MESSAGE #{} | topic={} | ts={} | payload_len={} (base64)",
                            message_count,
                            content_topic,
                            timestamp,
                            payload.len()
                        );

                        // Attempt to parse as real Railgun fee message (core protocol work)
                        let preview = if payload.len() > 120 {
                            format!("{}...", &payload[..120])
                        } else {
                            payload.clone()
                        };
                        info!("   payload preview: {}", preview);

                        // TODO later: base64 decode → JSON → parse as fee message
                    }
                    SidecarMessage::PeerUpdate { mesh, pubsub } => {
                        info!("Peer update: mesh_peers={}, pubsub_peers={}", mesh, pubsub);
                    }
                    SidecarMessage::Status { started, mesh_peers, pubsub_peers, .. } => {
                        info!("Status: started={}, mesh={}, pubsub={}", started, mesh_peers, pubsub_peers);
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
