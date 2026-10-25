//! Decoder for the brotli-packed ERC-7730 registry the host installs.
//!
//! Archive (then brotli): magic `Z773`, version `1`, little-endian `u32` count, then
//! `(u16 path_len, path, u32 json_len, json)*`. Contains `index.*.json`, `registry/**/*.json`, and
//! `ercs/**/*.json`.
//!
//! The pack bytes are a build artefact of the binary that ships them, so core keeps only the
//! format here: the bounds, the parser and the one-time cache. [`get`] reads the installed bytes
//! through [`crate::assets`]; nothing is baked into this crate.

use brotli_decompressor::BrotliDecompress;
use std::collections::HashMap;
use std::sync::OnceLock;

const MAGIC: &[u8; 4] = b"Z773";
const VERSION: u8 = 1;
const MAX_DECODED_PACK: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const MAX_PATH: usize = 256;

static FILES: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();

/// Uncompressed JSON bytes for a registry path, if present in the installed snapshot.
///
/// `None` means either that the host installed no pack or that the path is not in it; neither is
/// an error. A corrupt pack is logged once and then behaves like an empty one.
pub fn get(path: &str) -> Option<&'static [u8]> {
   let compressed = crate::assets::installed().clear_signing_pack()?;
   files(compressed).get(path).map(Vec::as_slice)
}

fn files(compressed: &'static [u8]) -> &'static HashMap<String, Vec<u8>> {
   FILES.get_or_init(|| load_pack(compressed))
}

fn load_pack(compressed: &[u8]) -> HashMap<String, Vec<u8>> {
   match decode_pack(compressed) {
      Ok(map) => map,
      Err(e) => {
         tracing::error!("ERC-7730 embedded registry pack is corrupt: {e}");
         HashMap::new()
      }
   }
}

/// Decode one brotli-packed archive into its path → JSON map.
///
/// This is the format-level entry point: the host can validate the pack it is about to ship, while
/// core's own consumers take [`get`], which reads the installed bytes through [`crate::assets`].
/// Every bound below is part of the archive format and an error means the pack is malformed.
pub fn decode_pack(compressed: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
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
