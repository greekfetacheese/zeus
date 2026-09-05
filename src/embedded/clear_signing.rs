//! Brotli-packed ERC-7730 registry snapshot.
//!
//! Layout (repo root):
//! ```text
//! embedded/clear_signing/registry.bin.br
//! ```
//!
//! Archive (then brotli): magic `Z773`, version `1`, little-endian
//! `u32` count, then `(u16 path_len, path, u32 json_len, json)*`.
//! Contains `index.*.json`, `registry/**/*.json`, and `ercs/**/*.json`.

use brotli::BrotliDecompress;
use std::collections::HashMap;
use std::sync::OnceLock;

const PACK_BR: &[u8] = include_bytes!("../../embedded/clear_signing/registry.bin.br");
const MAGIC: &[u8; 4] = b"Z773";
const VERSION: u8 = 1;
const MAX_DECODED_PACK: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const MAX_PATH: usize = 256;

static FILES: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();

/// Uncompressed JSON bytes for a registry path, if present in the snapshot.
pub fn get(path: &str) -> Option<&'static [u8]> {
   FILES.get_or_init(load_pack).get(path).map(Vec::as_slice)
}

fn load_pack() -> HashMap<String, Vec<u8>> {
   match decode_pack(PACK_BR) {
      Ok(map) => map,
      Err(e) => {
         tracing::error!("ERC-7730 embedded registry pack is corrupt: {e}");
         HashMap::new()
      }
   }
}

fn decode_pack(compressed: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
   let mut raw = Vec::new();
   BrotliDecompress(&mut &compressed[..], &mut raw).map_err(|e| format!("brotli: {e}"))?;
   if raw.len() > MAX_DECODED_PACK {
      return Err(format!("decoded pack too large ({})", raw.len()));
   }

   let mut cur = Cursor { s: &raw };
   let magic = cur.take(4)?;
   if magic != MAGIC {
      return Err("bad pack magic".into());
   }
   let version = cur.u8()?;
   if version != VERSION {
      return Err(format!("unsupported pack version {version}"));
   }
   let count = cur.u32()? as usize;
   if count == 0 || count > MAX_ENTRIES {
      return Err(format!("bad entry count {count}"));
   }

   let mut map = HashMap::with_capacity(count);
   for _ in 0..count {
      let path_len = cur.u16()? as usize;
      if path_len == 0 || path_len > MAX_PATH {
         return Err(format!("bad path length {path_len}"));
      }
      let path = std::str::from_utf8(cur.take(path_len)?)
         .map_err(|_| "path is not utf-8".to_string())?
         .to_string();
      let data_len = cur.u32()? as usize;
      if data_len > 512 * 1024 {
         return Err(format!(
            "{path} exceeds max json size ({data_len})"
         ));
      }
      let data = cur.take(data_len)?.to_vec();
      if map.insert(path.clone(), data).is_some() {
         return Err(format!("duplicate pack path {path}"));
      }
   }
   if !cur.s.is_empty() {
      return Err(format!("{} trailing pack bytes", cur.s.len()));
   }
   Ok(map)
}

struct Cursor<'a> {
   s: &'a [u8],
}

impl<'a> Cursor<'a> {
   fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
      if self.s.len() < n {
         return Err("truncated pack".into());
      }
      let (head, tail) = self.s.split_at(n);
      self.s = tail;
      Ok(head)
   }

   fn u8(&mut self) -> Result<u8, String> {
      Ok(self.take(1)?[0])
   }

   fn u16(&mut self) -> Result<u16, String> {
      let b = self.take(2)?;
      Ok(u16::from_le_bytes([b[0], b[1]]))
   }

   fn u32(&mut self) -> Result<u32, String> {
      let b = self.take(4)?;
      Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::clear_signing::registry_pins::{
      REGISTRY_PIN_COUNT, is_registry_file_pinned, pinned_registry_paths, verify_registry_pin,
   };

   #[test]
   fn embedded_pack_matches_sha256_pins() {
      let files = FILES.get_or_init(load_pack);
      assert_eq!(files.len(), REGISTRY_PIN_COUNT);
      for (path, bytes) in files {
         assert!(
            is_registry_file_pinned(path),
            "unpinned pack path {path}"
         );
         verify_registry_pin(path, bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
      }
      for path in pinned_registry_paths() {
         assert!(
            files.contains_key(path),
            "pin missing from pack: {path}"
         );
      }
   }
}
