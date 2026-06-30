use anyhow::{Result, anyhow};
use kanal::{AsyncReceiver, unbounded};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;
use tokio::time::{Duration, timeout};
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

/// Events coming back from the JS sidecar, including progress.
#[derive(Debug, Deserialize)]
pub struct ProverEvent {
   #[serde(default)]
   pub id: Option<u64>,
   #[serde(default)]
   pub r#type: Option<String>, // "started", "proof_generated", "progress", "error"
   #[serde(default)]
   pub success: bool,
   #[serde(default)]
   pub proof: Option<serde_json::Value>,
   #[serde(default)]
   pub error: Option<String>,
   // Progress fields
   #[serde(default)]
   pub stage: Option<String>,
   #[serde(default)]
   pub percent: Option<u8>,
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

      let (tx, rx) = unbounded::<ProverEvent>();
      let rx = rx.to_async();

      tokio::spawn(async move {
         let reader = BufReader::new(stdout);
         let mut lines = reader.lines();

         while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
               continue;
            }
            if let Ok(event) = serde_json::from_str::<ProverEvent>(&line) {
               let _ = tx.send(event);
            } else {
               warn!("Failed to parse prover event: {}", line);
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

      // Wait briefly for "started" confirmation
      let deadline = Duration::from_secs(5);
      match timeout(deadline, client.response_rx.recv()).await {
         Ok(Ok(event)) => {
            if event.r#type.as_deref() == Some("started") || event.success {
               info!("Railgun prover sidecar started successfully");
            } else if let Some(err) = &event.error {
               warn!("Sidecar reported error on start: {}", err);
            }
         }
         _ => {
            info!("Railgun prover sidecar spawned (no explicit 'started' confirmation)");
         }
      }

      Ok(client)
   }

   async fn next_id(&self) -> u64 {
      let mut id = self.next_id.lock().await;
      let cur = *id;
      *id += 1;
      cur
   }

   async fn send_command(&self, cmd: ProverCommand) -> Result<()> {
      let json = serde_json::to_string(&cmd)? + "\n";
      let mut stdin = self.stdin.lock().await;
      stdin.write_all(json.as_bytes()).await?;
      stdin.flush().await?;
      Ok(())
   }

   /// Request a proof.
   /// `witness` should be a `FormattedCircuitInputsRailgun` serialized as JSON value.
   ///
   /// This method will block until the sidecar returns a "proof_generated" event
   /// or an error. It properly handles progress messages in between.
   /// First run can take a long time because it downloads large zkey/wasm files.
   pub async fn prove(
      &self,
      witness: serde_json::Value,
      circuit_variant: &str,
   ) -> Result<serde_json::Value> {
      let id = self.next_id().await;
      let cmd = ProverCommand::Prove {
         id,
         params: ProveParams {
            witness,
            circuit_variant: circuit_variant.to_string(),
         },
      };
      self.send_command(cmd).await?;
      println!(
         "[rust] Sent prove request id={} for circuit {}",
         id, circuit_variant
      );

      let overall_deadline = Duration::from_secs(600); // 10 minutes for first-time artifact download + prove
      let start = std::time::Instant::now();

      loop {
         if start.elapsed() > overall_deadline {
            return Err(anyhow!(
               "Timeout after {}s waiting for sidecar to finish download + proof",
               overall_deadline.as_secs()
            ));
         }

         // Wait up to 30s for the next message, then check overall timeout
         match timeout(Duration::from_secs(30), self.response_rx.recv()).await {
            Ok(Ok(event)) => {
               // Log progress for visibility
               if event.r#type.as_deref() == Some("progress") {
                  if let (Some(stage), Some(pct)) = (&event.stage, event.percent) {
                     println!("[rust] Prover progress: {} {}%", stage, pct);
                  }
                  continue;
               }

               if let Some(proof) = event.proof {
                  println!("[rust] Received proof_generated from sidecar");
                  return Ok(proof);
               }

               if let Some(err) = event.error {
                  println!("[rust] Received error from sidecar: {}", err);
                  return Err(anyhow!("Prover error: {}", err));
               }

               // If we got a "started" or other event, keep waiting
               if let Some(et) = &event.r#type {
                  println!(
                     "[rust] Received event type '{}' while waiting for proof, continuing...",
                     et
                  );
               }
               continue;
            }
            Ok(Err(_)) => {
               // channel closed
               println!("[rust] Response channel closed while waiting for proof");
               return Err(anyhow!("Sidecar channel closed"));
            }
            Err(_) => {
               // 30s inner timeout, loop again (overall deadline will catch it)
               println!(
                  "[rust] Still waiting for proof from sidecar... ({}s elapsed)",
                  start.elapsed().as_secs()
               );
               continue;
            }
         }
      }
   }

   /// Convenience method using our strongly-typed inputs.
   pub async fn prove_with_inputs(
      &self,
      request: crate::models::ProofRequest,
   ) -> Result<serde_json::Value> {
      let formatted = crate::models::FormattedCircuitInputsRailgun::from_parts(
         &request.public_inputs,
         &request.private_inputs,
         &request.signature,
      );
      let witness = serde_json::to_value(&formatted)?;
      self.prove(witness, &request.circuit_variant).await
   }

   pub async fn stop(&self) -> Result<()> {
      let id = self.next_id().await;
      let _ = self.send_command(ProverCommand::Stop { id }).await;
      let mut child = self.child.lock().await;
      let _ = child.kill().await;
      Ok(())
   }
}
