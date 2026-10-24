//! Value envelope for the events snapshot (`events-snapshot:{chain}.db`).
//!
//! A `c:{index}` value is either bare bincode (`Vec<SyncEvent>`, the v1 shape) or
//! `[CHUNK_ESCAPE, codec, ..payload]`. The escape byte is unambiguous: a bincode
//! chunk starts with the event-count varint, whose markers are `0xFB..=0xFD` and
//! which rejects `0xFF`. Sealed chunks are compressed, the open tail is not.

use anyhow::anyhow;
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use std::io::{Read, Write};

/// Escape byte that marks a compressed envelope. A bincode chunk can never start
/// with it (see the module docs).
pub const CHUNK_ESCAPE: u8 = 0xFF;

const CODEC_DEFLATE: u8 = 1;

/// Below this a compressed envelope only adds bytes (short tail chunks).
const MIN_COMPRESS_LEN: usize = 1024;

/// Wrap an already-encoded `Vec<SyncEvent>` payload for storage.
///
/// `compress == false` reproduces the v1 value byte for byte. Compression is also
/// skipped when it would not shrink the payload, so an incompressible chunk is
/// stored raw rather than 2 bytes larger.
pub fn seal_payload(raw: Vec<u8>, compress: bool) -> Vec<u8> {
   if !compress || raw.len() < MIN_COMPRESS_LEN {
      return raw;
   }
   let Ok(packed) = deflate(&raw) else {
      return raw;
   };
   if packed.len() + 2 >= raw.len() {
      return raw;
   }
   let mut out = Vec::with_capacity(packed.len() + 2);
   out.push(CHUNK_ESCAPE);
   out.push(CODEC_DEFLATE);
   out.extend_from_slice(&packed);
   out
}

/// Undo [`seal_payload`], returning the bincode bytes.
pub fn open_payload(value: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   match value.split_first() {
      Some((&CHUNK_ESCAPE, rest)) => {
         let Some((&codec, body)) = rest.split_first() else {
            return Err(anyhow!("truncated snapshot chunk envelope"));
         };
         match codec {
            CODEC_DEFLATE => inflate(body),
            other => Err(anyhow!("unknown snapshot chunk codec {other}")),
         }
      }
      _ => Ok(value.to_vec()),
   }
}

fn deflate(input: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
   encoder.write_all(input).map_err(|e| anyhow!("snapshot chunk compress: {e}"))?;
   encoder.finish().map_err(|e| anyhow!("snapshot chunk compress: {e}"))
}

fn inflate(input: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   let mut out = Vec::new();
   ZlibDecoder::new(input)
      .read_to_end(&mut out)
      .map_err(|e| anyhow!("snapshot chunk decompress: {e}"))?;
   Ok(out)
}

#[cfg(test)]
mod tests {
   use super::*;

   /// Deterministic xorshift bytes: high entropy, no compressor can shrink it.
   fn xorshift_bytes(len: usize) -> Vec<u8> {
      let mut state = 0x9E37_79B9_u32;
      let mut out = Vec::with_capacity(len);
      while out.len() < len {
         state ^= state << 13;
         state ^= state >> 17;
         state ^= state << 5;
         out.extend_from_slice(&state.to_le_bytes());
      }
      out.truncate(len);
      out
   }

   #[test]
   fn unsealed_payload_is_returned_unchanged() {
      let raw = xorshift_bytes(4096);
      assert_eq!(seal_payload(raw.clone(), false), raw);
   }

   #[test]
   fn sealed_payload_round_trips() {
      // Highly repetitive payload: compression must actually win.
      let raw = vec![0u8; 64 * 1024];
      let sealed = seal_payload(raw.clone(), true);
      assert_eq!(sealed.first(), Some(&CHUNK_ESCAPE));
      assert!(sealed.len() < raw.len());
      assert_eq!(open_payload(&sealed).unwrap(), raw);
   }

   #[test]
   fn incompressible_payload_is_stored_raw() {
      let raw = xorshift_bytes(4096);
      assert_eq!(seal_payload(raw.clone(), true), raw);
   }

   #[test]
   fn short_payload_is_never_sealed() {
      let raw = vec![0u8; 64];
      assert_eq!(seal_payload(raw.clone(), true), raw);
   }

   #[test]
   fn bare_bincode_value_opens_as_is() {
      // A v1 chunk starts with the element-count varint, never `CHUNK_ESCAPE`.
      let raw = vec![0x02, 0x01, 0x02, 0x03];
      assert_eq!(open_payload(&raw).unwrap(), raw);
   }

   #[test]
   fn unknown_codec_is_an_error() {
      let err = open_payload(&[CHUNK_ESCAPE, 9, 1, 2, 3]).unwrap_err();
      assert!(
         err.to_string().contains("unknown snapshot chunk codec 9"),
         "unexpected error: {err}"
      );
   }

   #[test]
   fn truncated_envelope_is_an_error() {
      let err = open_payload(&[CHUNK_ESCAPE]).unwrap_err();
      assert!(
         err.to_string().contains("truncated"),
         "unexpected error: {err}"
      );
   }
}
