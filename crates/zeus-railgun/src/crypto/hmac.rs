use hmac::{Hmac, KeyInit, Mac};
use secure_types::Zeroize;
use sha2::Sha512;

use crate::types::{ChainCode, Key32};

pub fn hmac_sha512(key: &[u8], data: &[u8]) -> (Key32, ChainCode) {
   let mut mac = Hmac::<Sha512>::new_from_slice(key).expect("HMAC accepts any key length");
   mac.update(data);

   let mut result = mac.finalize().into_bytes();

   let mut left = [0u8; 32];
   let mut right = [0u8; 32];
   left.copy_from_slice(&result[..32]);
   right.copy_from_slice(&result[32..]);

   result.zeroize();

   let key = Key32::from_slice_mut(&mut left).expect("left is 32 bytes");
   let chain_code = Key32::from_slice_mut(&mut right).expect("right is 32 bytes");

   (key, ChainCode::new(chain_code))
}
