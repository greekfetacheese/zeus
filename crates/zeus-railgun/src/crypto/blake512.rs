//! Classic BLAKE-512 (SHA-3 finalist), circomlib-compatible.
//!
//! One-shot only. Replaces `blake-hash` 0.2 so Railgun does not pull
//! `digest` 0.8 / `generic-array` 0.12.

const BLOCK: usize = 128;
const ROUNDS: usize = 16;

#[rustfmt::skip]
const SIGMA: [[u8; 16]; 16] = [
   [ 0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15],
   [14, 10,  4,  8,  9, 15, 13,  6,  1, 12,  0,  2, 11,  7,  5,  3],
   [11,  8, 12,  0,  5,  2, 15, 13, 10, 14,  3,  6,  7,  1,  9,  4],
   [ 7,  9,  3,  1, 13, 12, 11, 14,  2,  6,  5, 10,  4,  0, 15,  8],
   [ 9,  0,  5,  7,  2,  4, 10, 15, 14,  1, 11, 12,  6,  8,  3, 13],
   [ 2, 12,  6, 10,  0, 11,  8,  3,  4, 13,  7,  5, 15, 14,  1,  9],
   [12,  5,  1, 15, 14, 13,  4, 10,  0,  7,  6,  3,  9,  2,  8, 11],
   [13, 11,  7, 14, 12,  1,  3,  9,  5,  0, 15,  4,  8,  6,  2, 10],
   [ 6, 15, 14,  9, 11,  3,  0,  8, 12,  2, 13,  7,  1,  4, 10,  5],
   [10,  2,  8,  4,  7,  6,  1,  5, 15, 11,  9, 14,  3, 12, 13,  0],
   [ 0,  1,  2,  3,  4,  5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15],
   [14, 10,  4,  8,  9, 15, 13,  6,  1, 12,  0,  2, 11,  7,  5,  3],
   [11,  8, 12,  0,  5,  2, 15, 13, 10, 14,  3,  6,  7,  1,  9,  4],
   [ 7,  9,  3,  1, 13, 12, 11, 14,  2,  6,  5, 10,  4,  0, 15,  8],
   [ 9,  0,  5,  7,  2,  4, 10, 15, 14,  1, 11, 12,  6,  8,  3, 13],
   [ 2, 12,  6, 10,  0, 11,  8,  3,  4, 13,  7,  5, 15, 14,  1,  9],
];

const U: [u64; 16] = [
   0x243f_6a88_85a3_08d3,
   0x1319_8a2e_0370_7344,
   0xa409_3822_299f_31d0,
   0x082e_fa98_ec4e_6c89,
   0x4528_21e6_38d0_1377,
   0xbe54_66cf_34e9_0c6c,
   0xc0ac_29b7_c97c_50dd,
   0x3f84_d5b5_b547_0917,
   0x9216_d5d9_8979_fb1b,
   0xd131_0ba6_98df_b5ac,
   0x2ffd_72db_d01a_dfb7,
   0xb8e1_afed_6a26_7e96,
   0xba7c_9045_f12c_7f99,
   0x24a1_9947_b391_6cf7,
   0x0801_f2e2_858e_fc16,
   0x6369_20d8_7157_4e69,
];

const IV: [u64; 8] = [
   0x6a09_e667_f3bc_c908,
   0xbb67_ae85_84ca_a73b,
   0x3c6e_f372_fe94_f82b,
   0xa54f_f53a_5f1d_36f1,
   0x510e_527f_ade6_82d1,
   0x9b05_688c_2b3e_6c1f,
   0x1f83_d9ab_fb41_bd6b,
   0x5be0_cd19_137e_2179,
];

const PADDING: [u8; 129] = {
   let mut p = [0u8; 129];
   p[0] = 0x80;
   p
};

/// BLAKE-512 digest of `data`. Unkeyed; salt and counter start at zero.
pub fn blake512(data: &[u8]) -> [u8; 64] {
   let mut h = IV;
   let mut t = [0u64; 2];

   let mut chunks = data.chunks_exact(BLOCK);
   for chunk in chunks.by_ref() {
      bump_count(&mut t, BLOCK as u64);
      let block: [u8; BLOCK] = chunk.try_into().expect("exact block");
      compress(&mut h, &block, t, false);
   }

   let rest = chunks.remainder();
   bump_count(&mut t, rest.len() as u64);

   let mut block = [0u8; BLOCK];
   block[..rest.len()].copy_from_slice(rest);
   let pos = rest.len();

   const FOOTER: usize = 1 + 2 * 8;
   let exactfit = if pos + FOOTER == BLOCK { 0x80 } else { 0 };
   // Low bit marks the full-length (512-bit) variant.
   let magic = 1u8 | exactfit;
   let extra_block = pos + FOOTER > BLOCK;

   if extra_block {
      block[pos..].copy_from_slice(&PADDING[..BLOCK - pos]);
      compress(&mut h, &block, t, false);

      let mut last = [0u8; BLOCK];
      // Previous block already started padding; skip the 0x80 byte.
      let pad_len = BLOCK - FOOTER;
      last[..pad_len].copy_from_slice(&PADDING[1..1 + pad_len]);
      last[pad_len] = magic;
      write_msglen(&mut last[pad_len + 1..], t);
      compress(&mut h, &last, t, true);
   } else {
      let pad_len = BLOCK - FOOTER - pos;
      block[pos..pos + pad_len].copy_from_slice(&PADDING[..pad_len]);
      block[pos + pad_len] = magic;
      write_msglen(&mut block[pos + pad_len + 1..], t);
      compress(&mut h, &block, t, pos == 0);
   }

   let mut out = [0u8; 64];
   for (i, word) in h.iter().enumerate() {
      out[i * 8..(i + 1) * 8].copy_from_slice(&word.to_be_bytes());
   }
   out
}

fn bump_count(t: &mut [u64; 2], count_bytes: u64) {
   let (new_t0, carry) = t[0].overflowing_add(count_bytes.wrapping_mul(8));
   t[0] = new_t0;
   if carry {
      t[1] += 1;
   }
}

fn write_msglen(dst: &mut [u8], t: [u64; 2]) {
   dst[..8].copy_from_slice(&t[1].to_be_bytes());
   dst[8..16].copy_from_slice(&t[0].to_be_bytes());
}

fn compress(h: &mut [u64; 8], block: &[u8; BLOCK], t: [u64; 2], nullt: bool) {
   let mut m = [0u64; 16];
   for (i, chunk) in block.chunks_exact(8).enumerate() {
      m[i] = u64::from_be_bytes(chunk.try_into().expect("8-byte word"));
   }

   let mut v = [0u64; 16];
   v[..8].copy_from_slice(h);
   v[8..].copy_from_slice(&U[..8]);
   if !nullt {
      v[12] ^= t[0];
      v[13] ^= t[0];
      v[14] ^= t[1];
      v[15] ^= t[1];
   }

   for sigma in &SIGMA[..ROUNDS] {
      g(&mut v, &m, sigma, 0, 4, 8, 12, 0);
      g(&mut v, &m, sigma, 1, 5, 9, 13, 2);
      g(&mut v, &m, sigma, 2, 6, 10, 14, 4);
      g(&mut v, &m, sigma, 3, 7, 11, 15, 6);
      g(&mut v, &m, sigma, 0, 5, 10, 15, 8);
      g(&mut v, &m, sigma, 1, 6, 11, 12, 10);
      g(&mut v, &m, sigma, 2, 7, 8, 13, 12);
      g(&mut v, &m, sigma, 3, 4, 9, 14, 14);
   }

   for (i, vx) in v.iter().enumerate() {
      h[i % 8] ^= *vx;
   }
}

#[inline(always)]
fn g(
   v: &mut [u64; 16],
   m: &[u64; 16],
   sigma: &[u8; 16],
   a: usize,
   b: usize,
   c: usize,
   d: usize,
   e: usize,
) {
   v[a] = v[a]
      .wrapping_add(m[sigma[e] as usize] ^ U[sigma[e + 1] as usize])
      .wrapping_add(v[b]);
   v[d] = (v[d] ^ v[a]).rotate_right(32);
   v[c] = v[c].wrapping_add(v[d]);
   v[b] = (v[b] ^ v[c]).rotate_right(25);
   v[a] = v[a]
      .wrapping_add(m[sigma[e + 1] as usize] ^ U[sigma[e] as usize])
      .wrapping_add(v[b]);
   v[d] = (v[d] ^ v[a]).rotate_right(16);
   v[c] = v[c].wrapping_add(v[d]);
   v[b] = (v[b] ^ v[c]).rotate_right(11);
}

#[cfg(test)]
mod tests {
   use super::*;

   fn hex(s: &str) -> [u8; 64] {
      let s = s.trim();
      let mut out = [0u8; 64];
      for i in 0..64 {
         out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
      }
      out
   }

   #[test]
   fn blake512_empty() {
      // Matches blake-hash 0.2 / circomlib BLAKE-512("").
      assert_eq!(
         blake512(b""),
         hex(
            "a8cfbbd73726062df0c6864dda65defe58ef0cc52a5625090fa17601e1eecd1b\
             628e94f396ae402a00acc9eab77b4d4c2e852aaaa25a636d80af3fc7913ef5b8"
         )
      );
   }

   #[test]
   fn blake512_abc() {
      assert_eq!(
         blake512(b"abc"),
         hex(
            "14266c7c704a3b58fb421ee69fd005fcc6eeff742136be67435df995b7c986e7\
             cbde4dbde135e7689c354d2bc5b8d260536c554b4f84c118e61efc576fed7cd3"
         )
      );
   }

   #[test]
   fn blake512_padding_boundaries() {
      // 111 bytes + 17-byte footer fills one block; 112 needs a second.
      assert_eq!(
         blake512(&[0x11; 111]),
         hex(
            "a32f1f3be179d056dbb11a94678b9de46f2ea9c18a07f51f2abfcbadab16ba84\
             c9d3b926aa66cfede708c82b19e2759bd96b86a86f1cf67d68907ecc55514662"
         )
      );
      assert_eq!(
         blake512(&[0x22; 112]),
         hex(
            "f674554c29ca202fcd0abca226d7ef4bcb5c7749460d0e3a400031a7883c1528\
             8d254d269ddfbee73dd9671a31d98d0b2755c77340d2f692112c9fab1167a319"
         )
      );
      assert_eq!(
         blake512(&[0x5a; 200]),
         hex(
            "e7f00aebe4d7f0e983bf4de4516841b20c0417e184248b87e728e9bc821de266\
             3263dba4a73de8734bbb9c8e89cb53deea38e812e310471cd02789cba11ffbc6"
         )
      );
   }
}
