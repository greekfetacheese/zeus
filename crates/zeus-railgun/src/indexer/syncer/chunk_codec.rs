//! Value envelope for the events snapshot (`events-snapshot:{chain}.db`).
//!
//! A `c:{index}` value is either bare bincode (`Vec<SyncEvent>`, the v1 shape) or
//! `[CHUNK_ESCAPE, codec, ..payload]`. The escape byte is unambiguous: a bincode
//! chunk starts with the event-count varint, whose markers are `0xFB..=0xFD` and
//! which rejects `0xFF`. Sealed chunks are compressed, the open tail is not.
//!
//! A sealed chunk whose payload would not fit one page-sized value is split into
//! parts of at most [`PIECE_MAX`] bytes: part 0 keeps the header, and the
//! continuations are stored at `c:{index}:{j}`.

use anyhow::anyhow;
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use std::io::{Read, Write};

/// Escape byte that marks a compressed envelope. A bincode chunk can never start
/// with it (see the module docs).
pub const CHUNK_ESCAPE: u8 = 0xFF;

/// Payload bytes a single stored value may hold. redb allocates a value a buddy
/// page run of the next power of two, so a value must stay under 16 KiB - 24 B
/// (its per-value overhead) to cost a 16 KiB run. Measured on redb 4.2.0 (64
/// values, key `c:{i}`): the 16 KiB allocation class holds up to 16,368 B, and
/// 16,369 B doubles to ~31 KiB per value - so this leaves 8 bytes of headroom.
pub const PIECE_MAX: usize = 16_360;

const CODEC_DEFLATE: u8 = 1;
const CODEC_DEFLATE_PIECED: u8 = 2;

/// Below this a compressed envelope only adds bytes (short tail chunks).
const MIN_COMPRESS_LEN: usize = 1024;

/// A sealed chunk: the header goes in front of part 0, the rest are continuation parts.
pub struct SealedChunk {
   pub parts: Vec<Vec<u8>>,
}

/// Compress `raw` and split it into parts of at most [`PIECE_MAX`] bytes.
///
/// `compress == false` keeps the v1 shape: one part, just the bincode bytes.
pub fn seal_chunk(raw: Vec<u8>, compress: bool) -> SealedChunk {
   let packed = if compress && raw.len() >= MIN_COMPRESS_LEN {
      deflate(&raw).ok().filter(|p| p.len() + 2 < raw.len())
   } else {
      None
   };
   let Some(packed) = packed else {
      return SealedChunk { parts: vec![raw] };
   };
   if packed.len() + 2 <= PIECE_MAX {
      let mut value = Vec::with_capacity(packed.len() + 2);
      value.push(CHUNK_ESCAPE);
      value.push(CODEC_DEFLATE);
      value.extend_from_slice(&packed);
      return SealedChunk { parts: vec![value] };
   }
   pieced_body(packed)
}

/// Envelope + payload, split into parts of at most [`PIECE_MAX`] bytes.
fn pieced_body(packed: Vec<u8>) -> SealedChunk {
   let count = pieced_count(packed.len());
   let mut body = Vec::with_capacity(packed.len() + 2 + varint_len(count));
   body.push(CHUNK_ESCAPE);
   body.push(CODEC_DEFLATE_PIECED);
   push_varint(&mut body, count);
   body.extend_from_slice(&packed);
   debug_assert_eq!(body.len(), packed.len() + 2 + varint_len(count));
   let parts = split_evenly(body, PIECE_MAX);
   debug_assert_eq!(parts.len(), count as usize);
   SealedChunk { parts }
}

/// Reassemble a chunk from `parts` (part 0 first) and return the bincode bytes.
pub fn open_chunk(parts: &[Vec<u8>]) -> Result<Vec<u8>, anyhow::Error> {
   let Some(first) = parts.first() else {
      return Err(anyhow!("empty snapshot chunk"));
   };
   match first.split_first() {
      Some((&CHUNK_ESCAPE, rest)) => {
         let Some((&codec, body)) = rest.split_first() else {
            return Err(anyhow!("truncated snapshot chunk envelope"));
         };
         match codec {
            CODEC_DEFLATE => inflate(body),
            CODEC_DEFLATE_PIECED => {
               let (count, tail) = read_varint(body)?;
               let count = count as usize;
               if count != parts.len() {
                  return Err(anyhow!(
                     "snapshot chunk has {count} parts but {} were read",
                     parts.len()
                  ));
               }
               let mut packed = Vec::new();
               packed.extend_from_slice(tail);
               for part in &parts[1..] {
                  packed.extend_from_slice(part);
               }
               inflate(&packed)
            }
            other => Err(anyhow!("unknown snapshot chunk codec {other}")),
         }
      }
      _ => {
         if parts.len() != 1 {
            return Err(anyhow!(
               "legacy snapshot chunk has {} parts",
               parts.len()
            ));
         }
         Ok(first.clone())
      }
   }
}

/// Part count a pieced `c:{index}` value declares, or `None` when the value is a
/// single-part value (bare bincode, or [`CODEC_DEFLATE`]) needing no continuations.
pub(crate) fn pieced_part_count(first: &[u8]) -> Result<Option<usize>, anyhow::Error> {
   let Some(rest) = first.strip_prefix(&[CHUNK_ESCAPE]) else {
      return Ok(None);
   };
   let Some((&codec, body)) = rest.split_first() else {
      return Ok(None);
   };
   if codec != CODEC_DEFLATE_PIECED {
      return Ok(None);
   }
   let (count, _) = read_varint(body)?;
   Ok(Some(count as usize))
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

/// Bytes a bincode-next varint occupies for `value`.
fn varint_len(value: u64) -> usize {
   if value <= 0xFA {
      1
   } else if value <= u64::from(u16::MAX) {
      3
   } else if value <= u64::from(u32::MAX) {
      5
   } else {
      9
   }
}

/// Part count such that `header + packed` splits into exactly this many parts.
fn pieced_count(packed_len: usize) -> u64 {
   let mut count = packed_len.div_ceil(PIECE_MAX) as u64;
   loop {
      let actual = (packed_len + 2 + varint_len(count)).div_ceil(PIECE_MAX) as u64;
      if actual == count {
         return count;
      }
      count = actual;
   }
}

/// bincode-next's varint: `<= 0xFA` is a literal, then `0xFB`/`0xFC`/`0xFD`
/// introduce a little-endian u16/u32/u64.
fn push_varint(out: &mut Vec<u8>, value: u64) {
   if value <= 0xFA {
      out.push(value as u8);
   } else if value <= u64::from(u16::MAX) {
      out.push(0xFB);
      out.extend_from_slice(&(value as u16).to_le_bytes());
   } else if value <= u64::from(u32::MAX) {
      out.push(0xFC);
      out.extend_from_slice(&(value as u32).to_le_bytes());
   } else {
      out.push(0xFD);
      out.extend_from_slice(&value.to_le_bytes());
   }
}

/// Inverse of [`push_varint`]: the value plus the bytes that follow it.
fn read_varint(input: &[u8]) -> Result<(u64, &[u8]), anyhow::Error> {
   let Some((&marker, rest)) = input.split_first() else {
      return Err(anyhow!("truncated snapshot chunk part count"));
   };
   let (width, value) = match marker {
      0x00..=0xFA => return Ok((u64::from(marker), rest)),
      0xFB => (
         2,
         u64::from(u16::from_le_bytes(read_array(rest)?)),
      ),
      0xFC => (
         4,
         u64::from(u32::from_le_bytes(read_array(rest)?)),
      ),
      0xFD => (8, u64::from_le_bytes(read_array(rest)?)),
      other => {
         return Err(anyhow!(
            "invalid snapshot chunk part count marker {other:#04x}"
         ));
      }
   };
   Ok((value, &rest[width..]))
}

fn read_array<const N: usize>(input: &[u8]) -> Result<[u8; N], anyhow::Error> {
   input
      .get(..N)
      .and_then(|bytes| bytes.try_into().ok())
      .ok_or_else(|| anyhow!("truncated snapshot chunk part count"))
}

fn ceil_div(value: usize, divisor: usize) -> usize {
   value.div_ceil(divisor)
}

/// Split `body` into the fewest even parts of at most `max` bytes each.
fn split_evenly(body: Vec<u8>, max: usize) -> Vec<Vec<u8>> {
   let count = ceil_div(body.len(), max);
   let per = ceil_div(body.len(), count);
   body.chunks(per).map(<[u8]>::to_vec).collect()
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
      assert_eq!(seal_chunk(raw.clone(), false).parts, vec![raw]);
   }

   #[test]
   fn sealed_payload_round_trips() {
      // Highly repetitive payload: compression must actually win.
      let raw = vec![0u8; 64 * 1024];
      let sealed = seal_chunk(raw.clone(), true);
      assert_eq!(sealed.parts.len(), 1);
      assert_eq!(sealed.parts[0].first(), Some(&CHUNK_ESCAPE));
      assert!(sealed.parts[0].len() < raw.len());
      assert_eq!(open_chunk(&sealed.parts).unwrap(), raw);
   }

   #[test]
   fn incompressible_payload_is_stored_raw() {
      let raw = xorshift_bytes(4096);
      assert_eq!(seal_chunk(raw.clone(), true).parts, vec![raw]);
   }

   #[test]
   fn short_payload_is_never_sealed() {
      let raw = vec![0u8; 64];
      assert_eq!(seal_chunk(raw.clone(), true).parts, vec![raw]);
   }

   #[test]
   fn bare_bincode_value_opens_as_is() {
      // A v1 chunk starts with the element-count varint, never `CHUNK_ESCAPE`.
      let raw = vec![0x02, 0x01, 0x02, 0x03];
      assert_eq!(open_chunk(&[raw.clone()]).unwrap(), raw);
   }

   #[test]
   fn unknown_codec_is_an_error() {
      let err = open_chunk(&[vec![CHUNK_ESCAPE, 9, 1, 2, 3]]).unwrap_err();
      assert!(
         err.to_string().contains("unknown snapshot chunk codec 9"),
         "unexpected error: {err}"
      );
   }

   #[test]
   fn truncated_envelope_is_an_error() {
      let err = open_chunk(&[vec![CHUNK_ESCAPE]]).unwrap_err();
      assert!(
         err.to_string().contains("truncated"),
         "unexpected error: {err}"
      );
   }

   #[test]
   fn pieced_round_trip() {
      // Half high-entropy, half zeros: compression wins, yet the packed payload is
      // still far larger than a single page-sized value.
      let mut raw = xorshift_bytes(24 * 1024);
      raw.extend(std::iter::repeat_n(0u8, 24 * 1024));

      let sealed = seal_chunk(raw.clone(), true);
      assert!(sealed.parts.len() > 1, "expected a pieced chunk");
      assert!(
         sealed.parts.iter().all(|part| part.len() <= PIECE_MAX),
         "every part must fit in one 16 KiB page run"
      );
      assert_eq!(
         pieced_part_count(&sealed.parts[0]).unwrap(),
         Some(sealed.parts.len())
      );
      assert_eq!(open_chunk(&sealed.parts).unwrap(), raw);
   }

   #[test]
   fn header_part_count_matches_the_split() {
      // Framing sweep over the whole boundary range, no compression: the declared
      // count must always equal the number of parts the body actually splits into.
      for n in 0..=200_000 {
         let sealed = pieced_body(vec![0u8; n]);
         assert!(
            sealed.parts.iter().all(|part| part.len() <= PIECE_MAX),
            "payload {n}: a part exceeds PIECE_MAX"
         );
         assert_eq!(
            pieced_part_count(&sealed.parts[0]).unwrap(),
            Some(sealed.parts.len()),
            "payload {n}: declared count disagrees with the split"
         );
      }
   }
}
