use std::{
   collections::{BTreeSet, HashMap, VecDeque},
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
   /// Circuits fully available from the binary (`include_bytes!` embeds).
   /// Prefetch does **not** write these to disk.
   pub embedded: Vec<String>,
   /// Circuits that already had a full valid on-disk set.
   pub already_cached: Vec<String>,
   /// Circuits that were downloaded (or re-fetched) successfully this run.
   pub downloaded: Vec<String>,
   /// Circuits that failed (404 / invalid / IO). Not fatal to the whole run.
   pub failed: Vec<(String, String)>,
}

impl PrefetchReport {
   pub fn ok_count(&self) -> usize {
      self.embedded.len() + self.already_cached.len() + self.downloaded.len()
   }

   pub fn failed_count(&self) -> usize {
      self.failed.len()
   }
}

/// Compile-time / binary-backed circuit artifacts.
///
/// Host apps (e.g. Zeus) build these with `include_bytes!` and register them on
/// [`RemoteArtifactLoader`]. Bytes stay in the binary — they are **not** copied
/// into the disk cache on load.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedCircuit {
   /// Circuit path as used by the host, e.g. `railgun/01x01`.
   pub name: &'static str,
   pub wasm_br: &'static [u8],
   pub proving_key_br: &'static [u8],
   pub matrices_br: &'static [u8],
}

impl EmbeddedCircuit {
   pub const fn new(
      name: &'static str,
      wasm_br: &'static [u8],
      proving_key_br: &'static [u8],
      matrices_br: &'static [u8],
   ) -> Self {
      Self {
         name,
         wasm_br,
         proving_key_br,
         matrices_br,
      }
   }

   fn file(&self, filename: &str) -> Option<&'static [u8]> {
      match filename {
         "wasm.br" => Some(self.wasm_br),
         "proving_key.bin.br" => Some(self.proving_key_br),
         "matrices.bin.br" => Some(self.matrices_br),
         _ => None,
      }
   }

   fn is_complete(&self) -> bool {
      validate_compressed_artifact(self.wasm_br).is_ok()
         && validate_compressed_artifact(self.proving_key_br).is_ok()
         && validate_compressed_artifact(self.matrices_br).is_ok()
   }
}

/// Snapshot of which transact circuits are usable offline
/// (binary embeds and/or on-disk cache).
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

   /// Largest `nullifiers` such that `railgun/{N}x{outputs}` is available.
   /// Returns `0` when none are available.
   pub fn max_inputs_for_outputs(&self, outputs: usize) -> usize {
      (1..=ARTIFACT_MAX_INPUTS)
         .rev()
         .find(|&n| self.contains(n, outputs))
         .unwrap_or(0)
   }

   /// Largest `commitments` such that `railgun/{inputs}x{M}` is available.
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

   /// Circuits embedded in the host binary. Looked up before disk/network and
   /// never auto-written to the disk cache.
   embedded: Arc<HashMap<&'static str, EmbeddedCircuit>>,
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
         embedded: Arc::new(HashMap::new()),
      }
   }

   pub fn with_cache_dir(self, dir: Option<PathBuf>) -> Self {
      Self {
         cache_dir: dir,
         ..self
      }
   }

   /// Register host-binary circuit embeds (`include_bytes!`).
   ///
   /// Incomplete entries (tiny / HTML-looking) are skipped with a warning.
   /// Existing embeds with the same name are replaced.
   pub fn with_embedded_circuits(
      mut self,
      circuits: impl IntoIterator<Item = EmbeddedCircuit>,
   ) -> Self {
      let map = Arc::make_mut(&mut self.embedded);
      for circuit in circuits {
         if !circuit.is_complete() {
            warn!(
               "Skipping incomplete embedded circuit {} (artifact validation failed)",
               circuit.name
            );
            continue;
         }
         map.insert(circuit.name, circuit);
      }
      self
   }

   pub fn cache_dir(&self) -> Option<&PathBuf> {
      self.cache_dir.as_ref()
   }

   pub fn clear_mem_cache(&self) {
      let mut cache = self.cache.lock().unwrap();
      *cache = Cache::new(64 * 1024 * 1024);
   }

   /// Names of circuits registered from the host binary.
   pub fn embedded_circuit_names(&self) -> Vec<&'static str> {
      let mut names: Vec<_> = self.embedded.keys().copied().collect();
      names.sort_unstable();
      names
   }

   pub fn is_circuit_embedded(&self, circuit_name: &str) -> bool {
      self.embedded.contains_key(circuit_name)
   }

   pub async fn load_wasm(&self, circuit_name: &str) -> Result<Vec<u8>, RemoteArtifactLoaderError> {
      info!("Loading WASM: {}", circuit_name);
      let compressed = self.load_compressed(circuit_name, "wasm.br").await?;
      Ok(decompress(&compressed)?)
   }

   pub async fn load_proving_key(
      &self,
      circuit_name: &str,
   ) -> Result<ProvingKey<ark_bn254::Bn254>, RemoteArtifactLoaderError> {
      info!("Loading proving key: {}", circuit_name);
      let compressed = self.load_compressed(circuit_name, "proving_key.bin.br").await?;
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
      let compressed = self.load_compressed(circuit_name, "matrices.bin.br").await?;
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

   /// True when the circuit can be loaded without the network
   /// (binary embed and/or complete disk cache).
   pub fn is_circuit_available(&self, circuit_name: &str) -> bool {
      self.is_circuit_embedded(circuit_name) || self.is_circuit_on_disk(circuit_name)
   }

   /// Offline-available circuits in the supported pack (embeds ∪ disk).
   pub fn available_circuits(&self) -> AvailableCircuits {
      let mut names = BTreeSet::new();
      for name in all_transact_circuit_names() {
         if self.is_circuit_available(&name) {
            names.insert(name);
         }
      }
      // Also surface any embedded names outside the pack scan (defensive).
      for &name in self.embedded.keys() {
         names.insert(name.to_string());
      }
      AvailableCircuits { names }
   }

   /// Max nullifiers with an available `railgun/{N}x{outputs}` circuit. `0` if none.
   pub fn max_cached_inputs_for_outputs(&self, outputs: usize) -> usize {
      self.available_circuits().max_inputs_for_outputs(outputs)
   }

   /// Ensure every compressed artifact for `circuit_name` is obtainable.
   ///
   /// Embedded circuits are a no-op success (nothing is written to disk).
   /// Otherwise downloads into the disk cache when configured.
   pub async fn ensure_circuit_cached(
      &self,
      circuit_name: &str,
   ) -> Result<(), RemoteArtifactLoaderError> {
      if self.is_circuit_embedded(circuit_name) {
         return Ok(());
      }
      if self.cache_dir.is_none() {
         return Err(RemoteArtifactLoaderError::NoCacheDir);
      }
      for file in TRANSACT_ARTIFACT_FILES {
         let _ = self.load_compressed(circuit_name, file).await?;
      }
      Ok(())
   }

   /// Iterate all supported transact circuits (`01x01` ..= `05x05`).
   ///
   /// - Embedded → counted in [`PrefetchReport::embedded`], **not** written to disk
   /// - Already on disk → [`PrefetchReport::already_cached`]
   /// - Else download into the disk cache
   ///
   /// Failures for individual circuits are recorded and do not abort the pass.
   pub async fn prefetch_all_circuits(&self) -> Result<PrefetchReport, RemoteArtifactLoaderError> {
      if self.cache_dir.is_none() && self.embedded.is_empty() {
         return Err(RemoteArtifactLoaderError::NoCacheDir);
      }

      let mut report = PrefetchReport::default();
      let names = all_transact_circuit_names();
      debug!(
         "Prefetching {} transact circuits (cache_dir={:?}, embedded={})",
         names.len(),
         self.cache_dir,
         self.embedded.len()
      );

      for name in names {
         if self.is_circuit_embedded(&name) {
            debug!("Circuit available from binary embed: {}", name);
            report.embedded.push(name);
            continue;
         }

         if self.is_circuit_on_disk(&name) {
            debug!("Circuit already cached on disk: {}", name);
            report.already_cached.push(name);
            continue;
         }

         if self.cache_dir.is_none() {
            report.failed.push((
               name,
               "no disk cache directory and circuit is not embedded".into(),
            ));
            continue;
         }

         match self.ensure_circuit_cached(&name).await {
            Ok(()) => {
               debug!("Downloaded circuit artifacts: {}", name);
               report.downloaded.push(name);
            }
            Err(e) => {
               warn!("Failed to prefetch {}: {}", name, e);
               report.failed.push((name, e.to_string()));
            }
         }
      }

      debug!(
         "Circuit prefetch done: {} embedded, {} disk-cached, {} downloaded, {} failed",
         report.embedded.len(),
         report.already_cached.len(),
         report.downloaded.len(),
         report.failed.len()
      );
      Ok(report)
   }

   /// Load one compressed artifact file for a circuit.
   ///
   /// Order: memory → **binary embed** → disk → remote download.
   /// Embeds are never written to disk.
   async fn load_compressed(
      &self,
      circuit_name: &str,
      filename: &str,
   ) -> Result<Vec<u8>, RemoteArtifactLoaderError> {
      let url = format!("{}/{}/{}", self.base_url, circuit_name, filename);
      let disk_path = self.artifact_path(circuit_name, filename);

      // 1. Memory cache (L1)
      if let Some(cached) = self.cache.lock().unwrap().get(&url) {
         debug!("Artifact served from memory cache: {}", url);
         return Ok(cached);
      }

      // 2. Binary embed available anytime, do not spill to disk.
      if let Some(data) = self.embedded_bytes(circuit_name, filename) {
         debug!(
            "Artifact served from binary embed: {}/{}",
            circuit_name, filename
         );
         let owned = data.to_vec();
         self.cache.lock().unwrap().insert(url, owned.clone());
         return Ok(owned);
      }

      // 3. Disk cache (L2)
      if let Some(ref path) = disk_path {
         if path.exists() {
            debug!(
               "Loading artifact from disk cache: {}",
               path.display()
            );
            let data = fs::read(path).await?;
            if let Err(reason) = validate_compressed_artifact(&data) {
               debug!(
                  "Discarding invalid disk cache entry {}: {}",
                  path.display(),
                  reason
               );
               let _ = fs::remove_file(path).await;
            } else {
               self.cache.lock().unwrap().insert(url.clone(), data.clone());
               return Ok(data);
            }
         }
      }

      // 4. Remote download
      debug!("Downloading from remote: {}", url);
      let response = self.client.get(&url).send().await?;
      let status = response.status();
      if status.as_u16() == 404 || status.as_u16() == 410 {
         return Err(RemoteArtifactLoaderError::NotFound {
            url: url.clone(),
            status: status.as_u16(),
         });
      }
      if !status.is_success() {
         return Err(RemoteArtifactLoaderError::InvalidArtifact {
            url: url.clone(),
            reason: format!("HTTP {status}"),
         });
      }

      if let Some(ct) = response
         .headers()
         .get(reqwest::header::CONTENT_TYPE)
         .and_then(|v| v.to_str().ok())
      {
         let ct = ct.to_ascii_lowercase();
         if ct.contains("text/html") {
            return Err(RemoteArtifactLoaderError::InvalidArtifact {
               url: url.clone(),
               reason: format!("unexpected Content-Type: {ct}"),
            });
         }
      }

      let data = response.bytes().await?.to_vec();
      if let Err(reason) = validate_compressed_artifact(&data) {
         return Err(RemoteArtifactLoaderError::InvalidArtifact {
            url: url.clone(),
            reason,
         });
      }

      if let Some(ref path) = disk_path {
         if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
         }
         fs::write(path, &data).await?;
         debug!("Saved artifact to disk: {}", path.display());
      }

      self.cache.lock().unwrap().insert(url, data.clone());
      Ok(data)
   }

   fn embedded_bytes(&self, circuit_name: &str, filename: &str) -> Option<&'static [u8]> {
      self.embedded.get(circuit_name).and_then(|c| c.file(filename))
   }

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

   #[test]
   fn embedded_counts_as_available_not_on_disk() {
      // Minimal fake "valid" blobs (> MIN_ARTIFACT_BYTES, not HTML).
      static BLOB: [u8; 300] = [0xAB; 300];
      let loader = RemoteArtifactLoader::new("https://example.invalid/artifacts", None)
         .with_embedded_circuits([EmbeddedCircuit::new("railgun/01x01", &BLOB, &BLOB, &BLOB)]);

      assert!(loader.is_circuit_embedded("railgun/01x01"));
      assert!(!loader.is_circuit_on_disk("railgun/01x01"));
      assert!(loader.is_circuit_available("railgun/01x01"));
      assert!(loader.available_circuits().contains(1, 1));
      assert_eq!(loader.max_cached_inputs_for_outputs(1), 1);
   }
}
