use anyhow::{anyhow, Result};
use kanal::{AsyncReceiver, unbounded};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

#[derive(Debug, Serialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ProverCommand {
    Start { id: u64 },
    Prove { id: u64, params: ProveParams },
    Stop { id: u64 },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProveParams {
    pub witness: serde_json::Value,
    pub circuit_variant: String,
}

#[derive(Debug, Deserialize)]
pub struct ProverEvent {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub proof: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

pub struct RailgunProverClient {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Arc<Mutex<Child>>,
    next_id: Arc<Mutex<u64>>,
    response_rx: AsyncReceiver<ProverEvent>,
}

impl RailgunProverClient {
    pub async fn start(sidecar_path: &str) -> Result<Self> {
        let mut cmd = Command::new("node");
        cmd.arg("src/index.js")
            .current_dir(sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("failed to open stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("failed to open stdout"))?;

        let (tx, rx) = unbounded::<ProverEvent>(); let rx = rx.to_async();

        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() { continue; }
                if let Ok(event) = serde_json::from_str::<ProverEvent>(&line) {
                    let _ = tx.send(event);
                } else {
                    warn!("Failed to parse prover event");
                }
            }
        });

        let client = Self {
            stdin: Arc::new(Mutex::new(stdin)),
            child: Arc::new(Mutex::new(child)),
            next_id: Arc::new(Mutex::new(1)),
            response_rx: rx,
        };

        client.send_command(ProverCommand::Start { id: 0 }).await?;
        info!("Railgun prover sidecar started");
        Ok(client)
    }

    async fn next_id(&self) -> u64 {
        let mut id = self.next_id.lock().await;
        let cur = *id; *id += 1; cur
    }

    async fn send_command(&self, cmd: ProverCommand) -> Result<()> {
        let json = serde_json::to_string(&cmd)? + "\n";
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(json.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    pub async fn prove(&self, witness: serde_json::Value, circuit_variant: &str) -> Result<serde_json::Value> {
        let id = self.next_id().await;
        let cmd = ProverCommand::Prove {
            id,
            params: ProveParams { witness, circuit_variant: circuit_variant.to_string() },
        };
        self.send_command(cmd).await?;
        let _ = timeout(Duration::from_secs(2), self.response_rx.recv()).await;

        // Placeholder proof for scaffold
        Ok(serde_json::json!({
            "pi_a": ["0","0"],
            "pi_b": [["0","0"],["0","0"]],
            "pi_c": ["0","0"]
        }))
    }

    pub async fn stop(&self) -> Result<()> {
        let id = self.next_id().await;
        let _ = self.send_command(ProverCommand::Stop { id }).await;
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
        Ok(())
    }
}
