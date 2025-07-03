use bincode::{Decode, Encode};

pub const TOKEN_DATA: &[u8] = include_bytes!("../token_data.data");

#[derive(Clone, Encode, Decode)]
pub struct TokenData {
   pub chain_id: u64,
   pub address: String,
   pub name: String,
   pub symbol: String,
   pub decimals: u8,
   pub icon_data: Vec<u8>,
}

impl TokenData {
   pub fn new(
      chain_id: u64,
      address: String,
      name: String,
      symbol: String,
      decimals: u8,
      icon_data: Vec<u8>,
   ) -> Self {
      Self {
         chain_id,
         address,
         name,
         symbol,
         decimals,
         icon_data,
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use bincode::{config::standard, decode_from_slice};
   use alloy_primitives::Address;
   use std::str::FromStr;

   #[test]
   fn test_token_data() {
      let (token_data, _bytes_read): (Vec<TokenData>, usize) =
         decode_from_slice(TOKEN_DATA, standard()).unwrap();

      for token in token_data {
         let _address = Address::from_str(&token.address).unwrap();
      }
   }
}
