//! Sidecar client for communicating with the Node.js Waku process.
//!
//! Architecture:
//! - Node.js sidecar (js-sidecar/src/index.js) handles ONLY Waku networking using @waku/sdk.
//! - Rust owns all Railgun logic (fees, encryption, broadcaster selection, transact requests).
//! - Communication is line-delimited JSON over stdin/stdout.
//!
//! This gives us reliable, maintained Waku while keeping the majority of code in Rust.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tracing::{debug, info};

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

/// Simple chain identifier (matches Railgun/TS).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chain {
    #[serde(rename = "type")]
    pub type_: u8,
    pub id: u64,
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
    },
    Message {
        #[serde(rename = "contentTopic")]
        content_topic: String,
        /// Base64-encoded
        payload: String,
        timestamp: u64,
        #[serde(rename = "pubsubTopic", default)]
        pubsub_topic: Option<String>,
    },
    PeerUpdate {
        mesh: u32,
        pubsub: u32,
    },
    Stopped {
        id: Option<u64>,
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
}

impl WakuSidecarClient {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            child: None,
            stdin: None,
            next_id: 1,
            event_tx: tx,
            event_rx: Some(rx),
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

    pub async fn start_waku(&mut self, chain: Chain, options: Option<SidecarOptions>) -> Result<u64> {
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
}

impl Default for WakuSidecarClient {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WakuSidecarClient {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}
