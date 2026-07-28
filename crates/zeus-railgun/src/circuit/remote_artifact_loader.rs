use std::{
   collections::{BTreeSet, VecDeque},
   io::Cursor,
   path::PathBuf,
   sync::{Arc, Mutex},
};

use ark_bn254::Fr;
use ark_circom::index::NPIndex;
use ark_groth16::ProvingKey;
use ark_serialize::CanonicalDeserialize;
use tokio::fs;
use tracing::{debug, info, warn};

use crate::crypto::serializable_np_index::SerializableNpIndex;

/// Minimum compressed size we accept for a remote artifact. 404 HTML pages
/// are typically a few KB of text; real `.br` proving keys are hundreds of KB+.
const MIN_ARTIFACT_BYTES: usize = 256;

/// Max nullifiers/commitments in the artifact pack Zeus ships against.
/// Protocol max is 13; larger circuits are simply not published in this pack.
pub const ARTIFACT_MAX_INPUTS: usize = 5;
pub const ARTIFACT_MAX_OUTPUTS: usize = 5;

/// Compressed files required for a complete transact circuit on disk.
const TRANSACT_ARTIFACT_FILES: &[&str] = &["wasm.br", "proving_key.bin.br", "matrices.bin.br"];

/// Format a transact circuit path used by the artifact host.
/// Example: `railgun/04x01`.
pub fn transact_circuit_name(nullifiers: usize, commitments: usize) -> String {
   format!("railgun/{:02}x{:02}", nullifiers, commitments)
}

/// All transact circuit names in the supported pack (`01x01` ..= `05x05`).
pub fn all_transact_circuit_names() -> Vec<String> {
   let mut names = Vec::with_capacity(ARTIFACT_MAX_INPUTS * ARTIFACT_MAX_OUTPUTS);
   for n in 1..=ARTIFACT_MAX_INPUTS {
      for m in 1..=ARTIFACT_MAX_OUTPUTS {
         names.push(transact_circuit_name(n, m));
      }
   }
   names
}

/// Result of a bulk prefetch pass.
#[derive(Debug, Clone, Default)]
pub struct PrefetchReport {
   /// Circuits that already had a full valid on-disk set.
   pub already_cached: Vec<String>,
   /// Circuits that were downloaded (or re-fetched) successfully this run.
   pub downloaded: Vec<String>,
   /// Circuits that failed (404 / invalid / IO). Not fatal to the whole run.
   pub failed: Vec<(String, String)>,
}

impl PrefetchReport {
   pub fn ok_count(&self) -> usize {
      self.already_cached.len() + self.downloaded.len()
   }

   pub fn failed_count(&self) -> usize {
      self.failed.len()
   }
}

/// Snapshot of which transact circuits are fully present on disk.
#[derive(Debug, Clone, Default)]
pub struct AvailableCircuits {
   /// Circuit names like `railgun/01x02`.
   pub names: BTreeSet<String>,
}

impl AvailableCircuits {
   pub fn contains(&self, nullifiers: usize, commitments: usize) -> bool {
      self.names.contains(&transact_circuit_name(nullifiers, commitments))
   }

   pub fn contains_name(&self, name: &str) -> bool {
      self.names.contains(name)
   }

   /// Largest `nullifiers` such that `railgun/{N}x{outputs}` is cached.
   /// Returns `0` when none are available.
   pub fn max_inputs_for_outputs(&self, outputs: usize) -> usize {
      (1..=ARTIFACT_MAX_INPUTS)
         .rev()
         .find(|&n| self.contains(n, outputs))
         .unwrap_or(0)
   }

   /// Largest `commitments` such that `railgun/{inputs}x{M}` is cached.
   pub fn max_outputs_for_inputs(&self, inputs: usize) -> usize {
      (1..=ARTIFACT_MAX_OUTPUTS)
         .rev()
         .find(|&m| self.contains(inputs, m))
         .unwrap_or(0)
   }

   pub fn is_empty(&self) -> bool {
      self.names.is_empty()
   }

   pub fn len(&self) -> usize {
      self.names.len()
   }
}

#[derive(Clone)]
pub struct RemoteArtifactLoader {
   base_url: String,
   client: reqwest::Client,
   cache: Arc<Mutex<Cache>>,

   /// Optional on-disk cache directory.
   /// When set, downloaded .br files are persisted here so we don't re-download.
   /// Recommended structure: {cache_dir}/{circuit_name}/{artifact}.br
   cache_dir: Option<PathBuf>,
}

struct Cache {
   entries: VecDeque<(String, Vec<u8>)>,
   total_bytes: usize,
   max_bytes: usize,
}

impl Cache {
   fn new(max_bytes: usize) -> Self {
      Self {
         entries: VecDeque::new(),
         total_bytes: 0,
         max_bytes,
      }
   }

   fn get(&self, url: &str) -> Option<Vec<u8>> {
      self.entries.iter().find(|(k, _)| k == url).map(|(_, v)| v.clone())
   }

   fn insert(&mut self, url: String, data: Vec<u8>) {
      let size = data.len();
      self.entries.push_back((url, data));
      self.total_bytes += size;
      while self.total_bytes > self.max_bytes {
         if let Some((_, evicted)) = self.entries.pop_front() {
            self.total_bytes -= evicted.len();
         } else {
            break;
         }
      }
   }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteArtifactLoaderError {
   #[error("HTTP error: {0}")]
   HttpError(#[from] reqwest::Error),
   #[error("Artifact not found (HTTP {status}): {url}")]
   NotFound { url: String, status: u16 },
   #[error("Invalid artifact at {url}: {reason}")]
   InvalidArtifact { url: String, reason: String },
   #[error("Deserialization error: {0}")]
   DeserializationError(#[from] ark_serialize::SerializationError),
   #[error("Decompression error: {0}")]
   DecompressionError(#[from] std::io::Error),
   #[error("No on-disk cache directory configured")]
   NoCacheDir,
}

impl Default for RemoteArtifactLoader {
   fn default() -> Self {
      Self::new(
         "https://github.com/Robert-MacWha/privacy-protocol-artifacts/raw/refs/heads/main/artifacts/",
         None,
      )
   }
}

impl RemoteArtifactLoader {
   pub fn new(base_url: &str, cache_dir: Option<PathBuf>) -> Self {
      Self {
         base_url: base_url.trim_end_matches('/').to_string(),
         client: reqwest::Client::new(),
         cache: Arc::new(Mutex::new(Cache::new(64 * 1024 * 1024))),
         cache_dir,
      }
   }

   pub fn with_cache_dir(self, dir: Option<PathBuf>) -> Self {
      Self {
         cache_dir: dir,
         ..self
      }
   }

   pub fn cache_dir(&self) -> Option<&PathBuf> {
      self.cache_dir.as_ref()
   }

   pub async fn load_wasm(&self, circuit_name: &str) -> Result<Vec<u8>, RemoteArtifactLoaderError> {
      info!("Loading WASM: {}", circuit_name);
      let url = format!("{}/{}/wasm.br", self.base_url, circuit_name);
      let disk_path = self.artifact_path(circuit_name, "wasm.br");
      let compressed = self.fetch(&url, disk_path).await?;
      Ok(decompress(&compressed)?)
   }

   pub async fn load_proving_key(
      &self,
      circuit_name: &str,
   ) -> Result<ProvingKey<ark_bn254::Bn254>, RemoteArtifactLoaderError> {
      info!("Loading proving key: {}", circuit_name);
      let url = format!(
         "{}/{}/proving_key.bin.br",
         self.base_url, circuit_name
      );
      let disk_path = self.artifact_path(circuit_name, "proving_key.bin.br");
      let compressed = self.fetch(&url, disk_path).await?;
      let bytes = decompress(&compressed)?;
      let pk =
         ProvingKey::<ark_bn254::Bn254>::deserialize_uncompressed_unchecked(Cursor::new(bytes))?;
      Ok(pk)
   }

   pub async fn load_matrices(
      &self,
      circuit_name: &str,
   ) -> Result<NPIndex<Fr>, RemoteArtifactLoaderError> {
      info!("Loading matrices: {}", circuit_name);
      let url = format!(
         "{}/{}/matrices.bin.br",
         self.base_url, circuit_name
      );
      let disk_path = self.artifact_path(circuit_name, "matrices.bin.br");
      let compressed = self.fetch(&url, disk_path).await?;
      let bytes = decompress(&compressed)?;
      let matrices =
         SerializableNpIndex::<Fr>::deserialize_uncompressed_unchecked(Cursor::new(bytes))?;
      Ok(matrices.into())
   }

   /// True when all required compressed files for `circuit_name` exist on disk
   /// and pass basic validation (size / not-HTML). Does not hit the network.
   pub fn is_circuit_on_disk(&self, circuit_name: &str) -> bool {
      for file in TRANSACT_ARTIFACT_FILES {
         let Some(path) = self.artifact_path(circuit_name, file) else {
            return false;
         };
         if !path.is_file() {
            return false;
         }
         match std::fs::read(&path) {
            Ok(data) if validate_compressed_artifact(&data).is_ok() => {}
            _ => return false,
         }
      }
      true
   }

   /// Scan the disk cache for complete transact circuits in the supported pack.
   pub fn available_circuits(&self) -> AvailableCircuits {
      let mut names = BTreeSet::new();
      for name in all_transact_circuit_names() {
         if self.is_circuit_on_disk(&name) {
            names.insert(name);
         }
      }
      AvailableCircuits { names }
   }

   /// Max nullifiers with a cached `railgun/{N}x{outputs}` circuit. `0` if none.
   pub fn max_cached_inputs_for_outputs(&self, outputs: usize) -> usize {
      self.available_circuits().max_inputs_for_outputs(outputs)
   }

   /// Ensure every compressed artifact for `circuit_name` is on disk (download if missing).
   /// Does **not** deserialize proving keys / matrices cheap enough for bulk prefetch.
   pub async fn ensure_circuit_cached(
      &self,
      circuit_name: &str,
   ) -> Result<(), RemoteArtifactLoaderError> {
      if self.cache_dir.is_none() {
         return Err(RemoteArtifactLoaderError::NoCacheDir);
      }
      for file in TRANSACT_ARTIFACT_FILES {
         let url = format!("{}/{}/{}", self.base_url, circuit_name, file);
         let disk_path = self.artifact_path(circuit_name, file);
         self.fetch(&url, disk_path).await?;
      }
      Ok(())
   }

   /// Iterate all supported transact circuits (`01x01` ..= `05x05`), skip those
   /// already complete on disk, and download the rest.
   ///
   /// Failures for individual circuits are recorded in the report and do not abort
   /// the whole pass useful at app startup.
   pub async fn prefetch_all_circuits(&self) -> Result<PrefetchReport, RemoteArtifactLoaderError> {
      if self.cache_dir.is_none() {
         return Err(RemoteArtifactLoaderError::NoCacheDir);
      }

      let mut report = PrefetchReport::default();
      let names = all_transact_circuit_names();
      info!(
         "Prefetching {} transact circuits into {:?}",
         names.len(),
         self.cache_dir
      );

      for name in names {
         if self.is_circuit_on_disk(&name) {
            debug!("Circuit already cached: {}", name);
            report.already_cached.push(name);
            continue;
         }

         match self.ensure_circuit_cached(&name).await {
            Ok(()) => {
               info!("Downloaded circuit artifacts: {}", name);
               report.downloaded.push(name);
            }
            Err(e) => {
               warn!("Failed to prefetch {}: {}", name, e);
               report.failed.push((name, e.to_string()));
            }
         }
      }

      info!(
         "Circuit prefetch done: {} cached, {} downloaded, {} failed",
         report.already_cached.len(),
         report.downloaded.len(),
         report.failed.len()
      );
      Ok(report)
   }

   /// Core fetch logic with disk persistence.
   ///
   /// Order of preference:
   /// 1. In-memory cache (fast)
   /// 2. Disk cache (if configured)
   /// 3. Remote download (then save to disk if configured)
   async fn fetch(
      &self,
      url: &str,
      disk_path: Option<PathBuf>,
   ) -> Result<Vec<u8>, RemoteArtifactLoaderError> {
      // 1. Memory cache (L1)
      if let Some(cached) = self.cache.lock().unwrap().get(url) {
         debug!("Artifact served from memory cache: {}", url);
         return Ok(cached);
      }

      // 2. Disk cache (L2) — if configured
      if let Some(ref path) = disk_path {
         if path.exists() {
            debug!(
               "Loading artifact from disk cache: {}",
               path.display()
            );
            let data = fs::read(path).await?;
            if let Err(reason) = validate_compressed_artifact(&data) {
               // Stale/corrupt disk entry (e.g. previously cached 404 HTML) — drop it.
               debug!(
                  "Discarding invalid disk cache entry {}: {}",
                  path.display(),
                  reason
               );
               let _ = fs::remove_file(path).await;
            } else {
               self.cache.lock().unwrap().insert(url.to_string(), data.clone());
               return Ok(data);
            }
         }
      }

      // 3. Remote download
      debug!("Downloading from remote: {}", url);
      let response = self.client.get(url).send().await?;
      let status = response.status();
      if status.as_u16() == 404 || status.as_u16() == 410 {
         return Err(RemoteArtifactLoaderError::NotFound {
            url: url.to_string(),
            status: status.as_u16(),
         });
      }
      if !status.is_success() {
         return Err(RemoteArtifactLoaderError::InvalidArtifact {
            url: url.to_string(),
            reason: format!("HTTP {status}"),
         });
      }

      // Reject obvious HTML error pages even if the host returned 200.
      if let Some(ct) = response
         .headers()
         .get(reqwest::header::CONTENT_TYPE)
         .and_then(|v| v.to_str().ok())
      {
         let ct = ct.to_ascii_lowercase();
         if ct.contains("text/html") {
            return Err(RemoteArtifactLoaderError::InvalidArtifact {
               url: url.to_string(),
               reason: format!("unexpected Content-Type: {ct}"),
            });
         }
      }

      let data = response.bytes().await?.to_vec();
      if let Err(reason) = validate_compressed_artifact(&data) {
         return Err(RemoteArtifactLoaderError::InvalidArtifact {
            url: url.to_string(),
            reason,
         });
      }

      // Save to disk if we have a cache dir
      if let Some(ref path) = disk_path {
         if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
         }
         fs::write(path, &data).await?;
         debug!("Saved artifact to disk: {}", path.display());
      }

      // Populate memory cache
      self.cache.lock().unwrap().insert(url.to_string(), data.clone());

      Ok(data)
   }

   /// Computes the on-disk path for a given artifact.
   fn artifact_path(&self, circuit_name: &str, filename: &str) -> Option<PathBuf> {
      self.cache_dir.as_ref().map(|dir| dir.join(circuit_name).join(filename))
   }
}

fn validate_compressed_artifact(data: &[u8]) -> Result<(), String> {
   if data.len() < MIN_ARTIFACT_BYTES {
      return Err(format!(
         "response too small ({} bytes); circuit artifact is likely missing",
         data.len()
      ));
   }
   // Brotli streams don't have a universal magic header, but HTML almost always
   // starts with '<'. Catch the common 404 body case early.
   let head = &data[..data.len().min(64)];
   if head.iter().any(|&b| b == b'<')
      && (starts_with_ignore_ws(head, b"<!DOCTYPE")
         || starts_with_ignore_ws(head, b"<html")
         || starts_with_ignore_ws(head, b"<HTML"))
   {
      return Err("response looks like HTML, not a brotli artifact".into());
   }
   Ok(())
}

fn starts_with_ignore_ws(data: &[u8], prefix: &[u8]) -> bool {
   let trimmed = data
      .iter()
      .position(|&b| !b.is_ascii_whitespace())
      .map(|i| &data[i..])
      .unwrap_or(&[]);
   trimmed.len() >= prefix.len() && trimmed[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn decompress(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {
   let mut out = Vec::new();
   brotli::BrotliDecompress(&mut &data[..], &mut out)?;
   Ok(out)
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn circuit_name_padding() {
      assert_eq!(transact_circuit_name(1, 2), "railgun/01x02");
      assert_eq!(transact_circuit_name(5, 5), "railgun/05x05");
   }

   #[test]
   fn all_transact_count() {
      assert_eq!(
         all_transact_circuit_names().len(),
         ARTIFACT_MAX_INPUTS * ARTIFACT_MAX_OUTPUTS
      );
   }

   #[test]
   fn available_max_inputs() {
      let mut a = AvailableCircuits::default();
      a.names.insert("railgun/02x01".into());
      a.names.insert("railgun/04x01".into());
      a.names.insert("railgun/03x02".into());
      assert_eq!(a.max_inputs_for_outputs(1), 4);
      assert_eq!(a.max_inputs_for_outputs(2), 3);
      assert_eq!(a.max_inputs_for_outputs(5), 0);
   }
}
