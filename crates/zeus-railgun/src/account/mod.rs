pub mod address;
pub mod keys;
pub mod signer;

use crate::crypto::{hmac_sha512, babyjubjub::CURVE_SEED};
use crate::types::Key32;

use secure_types::Zeroize;

pub fn derive_private_key(seed: &[u8], path: &str) -> Result<Key32, anyhow::Error> {
   let (mut current_key, mut current_chain_code) = hmac_sha512(CURVE_SEED, seed);

   // Parse and derive for each segment in path (skip 'm')
   let segments: Vec<&str> = path.split('/').skip(1).collect();
   for segment in segments {
      let index_str = segment.trim_end_matches("'");
      let mut index: u32 = index_str.parse()?;

      index += 0x80000000;

      let index_bytes = index.to_be_bytes();

      // Data = 0x00 + current_key + index_bytes
      let mut data = vec![0u8];
      current_key.unlock(|slice| data.extend_from_slice(slice));
      data.extend_from_slice(&index_bytes);

      let (key, chain_code) = current_chain_code.unlock(|code| {
         let (new_key, new_code) = hmac_sha512(code, &data);
         (new_key, new_code)
      });

      data.zeroize();

      current_key = key;
      current_chain_code = chain_code;
   }

   Ok(current_key)
}
