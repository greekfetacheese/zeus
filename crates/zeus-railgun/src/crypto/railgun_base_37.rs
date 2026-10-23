//! Railgun Base 37 encoding and decoding implementation in Rust.

const CHARSET: &[u8] = b" 0123456789abcdefghijklmnopqrstuvwxyz";
const BASE: u128 = CHARSET.len() as u128;

#[derive(Debug)]
#[non_exhaustive]
pub enum EncodingError {
   InvalidCharacter(char),
   OutputTooLong(usize),
}

impl std::fmt::Display for EncodingError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::InvalidCharacter(c) => write!(f, "invalid character: {c}"),
         Self::OutputTooLong(len) => write!(f, "output exceeds {len} bytes"),
      }
   }
}

impl std::error::Error for EncodingError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::InvalidCharacter(_) | Self::OutputTooLong(_) => None,
      }
   }
}

/// Encodes a string into a 16-byte array using Railgun's base 37 encoding.
pub fn encode(text: &str) -> Result<[u8; 16], EncodingError> {
   let mut value: u128 = 0;

   for c in text.chars() {
      let idx = CHARSET
         .iter()
         .position(|&b| b == c as u8)
         .ok_or(EncodingError::InvalidCharacter(c))?;

      value = value
         .checked_mul(BASE)
         .and_then(|v| v.checked_add(idx as u128))
         .ok_or(EncodingError::OutputTooLong(16))?;
   }

   Ok(value.to_be_bytes())
}

/// Decodes a 16-byte array back into a string using Railgun's base 37 encoding.
///
/// ? Kept for completeness & testing
#[allow(dead_code)]
fn decode(bytes: &[u8; 16]) -> String {
   let mut value = u128::from_be_bytes(*bytes);

   let mut result = Vec::new();
   while value > 0 {
      let remainder = (value % BASE) as usize;
      result.push(CHARSET[remainder]);
      value /= BASE;
   }

   result.reverse();
   String::from_utf8(result).expect("CHARSET is ASCII")
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn encode_expected() {
      // Expected value sourced from Railgun SDK to ensure compatibility
      let text = "hello world";
      let expected: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 58, 182, 27, 136, 104, 32, 128];
      let encoded = encode(text).unwrap();
      assert_eq!(encoded, expected);
   }

   #[test]
   fn roundtrip() {
      let texts = ["", "hello", "railgun", "0x1234", "test 123"];
      for text in texts {
         let encoded = encode(text).unwrap();
         let decoded = decode(&encoded);
         assert_eq!(decoded, text);
      }
   }

   #[test]
   fn invalid_char() {
      assert!(matches!(
         encode("HELLO"),
         Err(EncodingError::InvalidCharacter('H'))
      ));
   }
}

#[cfg(test)]
mod error_tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_messages() {
      assert_eq!(
         EncodingError::InvalidCharacter('H').to_string(),
         "invalid character: H"
      );
      assert_eq!(
         EncodingError::OutputTooLong(16).to_string(),
         "output exceeds 16 bytes"
      );
   }

   #[test]
   fn the_payload_variants_have_no_source() {
      assert!(EncodingError::InvalidCharacter('H').source().is_none());
      assert!(EncodingError::OutputTooLong(16).source().is_none());
   }
}
