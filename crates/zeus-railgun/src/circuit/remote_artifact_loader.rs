use std::{
   collections::VecDeque,
   io::Cursor,
   path::PathBuf,
   sync::{Arc, Mutex},
};

use ark_bn254::Fr;
use ark_circom::index::NPIndex;
use ark_groth16::ProvingKey;
use ark_serialize::CanonicalDeserialize;
use tokio::fs;
use tracing::{debug, info};

use crate::crypto::serializable_np_index::SerializableNpIndex;

/// Minimum compressed size we accept for a remote artifact. 404 HTML pages
/// are typically a few KB of text; real `.br` proving keys are hundreds of KB+.
const MIN_ARTIFACT_BYTES: usize = 256;

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
